use anyhow::Context;
use chrono::{DateTime, Utc};
use pumphound_domain::{
    ChainCursor, ChainEvent, ChainEventPayload, DurableBatch, MarketSnapshot, RawChainEnvelope,
    Slot,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone)]
pub struct Stores {
    pub postgres: PgPool,
    pub clickhouse: clickhouse::Client,
    pub nats: async_nats::Client,
    pub jetstream: async_nats::jetstream::Context,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("postgres: {0}")]
    Postgres(#[from] sqlx::Error),
    #[error("clickhouse: {0}")]
    Clickhouse(String),
    #[error("nats: {0}")]
    Nats(String),
    #[error("config: {0}")]
    Config(String),
}

impl Stores {
    pub async fn connect() -> anyhow::Result<Self> {
        let pg_url = std::env::var("PUMPHOUND_POSTGRES_DSN")
            .unwrap_or_else(|_| "postgres://pumphound:pumphound@127.0.0.1:5432/pumphound".into());
        let postgres = PgPoolOptions::new()
            .max_connections(8)
            .connect(&pg_url)
            .await
            .context("postgres connect")?;

        let ch_url = std::env::var("PUMPHOUND_CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8123".into());
        let ch_user =
            std::env::var("PUMPHOUND_CLICKHOUSE_USER").unwrap_or_else(|_| "pumphound".into());
        let ch_pass =
            std::env::var("PUMPHOUND_CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "pumphound".into());
        let clickhouse = clickhouse::Client::default()
            .with_url(ch_url)
            .with_user(ch_user)
            .with_password(ch_pass)
            .with_database("pumphound");

        let nats_url =
            std::env::var("PUMPHOUND_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".into());
        let nats = async_nats::connect(nats_url)
            .await
            .map_err(|e| anyhow::anyhow!("nats: {e}"))?;
        let jetstream = async_nats::jetstream::new(nats.clone());
        let _ = jetstream
            .get_or_create_stream(async_nats::jetstream::stream::Config {
                name: "PUMPHOUND".into(),
                subjects: vec![
                    "chain.>".into(),
                    "market.>".into(),
                    "feature.>".into(),
                    "prediction.>".into(),
                    "decision.>".into(),
                    "paper.>".into(),
                    "graph.>".into(),
                    "health.>".into(),
                ],
                max_messages: 10_000_000,
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow::anyhow!("jetstream stream: {e}"))?;

        Ok(Self {
            postgres,
            clickhouse,
            nats,
            jetstream,
        })
    }

    pub async fn persist_raw_batch(&self, batch: &DurableBatch) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.ingestion_batches
            (batch_id, source, first_slot, last_slot, journal_uri, content_sha256, row_count, status, created_at, updated_at)
            VALUES ($1,$2,$3,$4,$5,$6,$7,'persisted', now(), now())
            ON CONFLICT (batch_id) DO UPDATE SET status='persisted', updated_at=now()"#,
        )
        .bind(batch.batch_id)
        .bind(&batch.cursor.source)
        .bind(batch.rows.first().map(|r| r.slot.0 as i64))
        .bind(batch.rows.last().map(|r| r.slot.0 as i64))
        .bind(&batch.journal_uri)
        .bind(&batch.content_sha256)
        .bind(batch.rows.len() as i64)
        .execute(&self.postgres)
        .await?;

        if !batch.rows.is_empty() {
            // Writes go through the Buffer-engine table so many small per-batch
            // inserts coalesce into fewer, larger MergeTree parts instead of
            // burying the background merge executor (see
            // db/clickhouse/002_insert_buffering.sql). Reads still hit the
            // underlying table directly.
            let mut insert = self
                .clickhouse
                .insert("raw_sightings_buffer")
                .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
            for row in &batch.rows {
                insert
                    .write(&RawSightingRow {
                        sighting_id: row.sighting_id.clone(),
                        batch_id: batch.batch_id,
                        source: row.source.clone(),
                        source_cursor: row.source_cursor.clone(),
                        slot: row.slot.0,
                        signature: row
                            .payload
                            .get("signature")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        observed_at: row.observed_at,
                        commitment: format!("{:?}", row.commitment).to_lowercase(),
                        payload_sha256: row.payload_sha256.clone(),
                        payload_codec: "json".into(),
                        raw_payload: row.payload.to_string(),
                        raw_partition_uri: batch.journal_uri.clone(),
                    })
                    .await
                    .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
            }
            insert
                .end()
                .await
                .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        }
        Ok(())
    }

    pub async fn persist_canonical(&self, events: &[ChainEvent]) -> Result<(), StorageError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut insert = self
            .clickhouse
            .insert("canonical_event_revisions_buffer")
            .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        for ev in events {
            let (kind, mint, market, trader, quote_mint, base, quote) = flatten(ev);
            insert
                .write(&CanonicalRow {
                    event_id: ev.event_id.clone(),
                    revision: ev.revision,
                    action: match ev.action {
                        pumphound_domain::EventAction::Upsert => 1,
                        pumphound_domain::EventAction::Retract => 2,
                    },
                    protocol: match ev.protocol {
                        pumphound_domain::Protocol::PumpCurve => "pump_curve".into(),
                        pumphound_domain::Protocol::PumpSwap => "pump_swap".into(),
                    },
                    kind,
                    program_id: ev.program_id.to_string(),
                    signature: ev.signature.clone(),
                    slot: ev.slot.0,
                    tx_index: ev.tx_index,
                    instruction_path: ev.instruction_path.clone(),
                    event_index: ev.event_index,
                    block_time: ev.block_time,
                    observed_at: ev.observed_at,
                    computed_at: ev.computed_at,
                    known_at: ev.observed_at,
                    commitment: format!("{:?}", ev.commitment).to_lowercase(),
                    canonicality: format!("{:?}", ev.canonicality).to_lowercase(),
                    transaction_succeeded: ev.transaction_succeeded,
                    mint,
                    market,
                    trader,
                    quote_mint,
                    base_amount_atomic: base,
                    quote_amount_atomic: quote,
                    decoder_version: ev.provenance.decoder_version.clone(),
                    idl_sha256: ev.provenance.idl_sha256.clone().unwrap_or_default(),
                    payload_json: serde_json::to_string(&ev.payload).unwrap_or_default(),
                    raw_partition_uri: ev.provenance.raw_partition_uri.clone(),
                })
                .await
                .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        }
        insert
            .end()
            .await
            .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        Ok(())
    }

    pub async fn persist_market_snapshots(
        &self,
        snapshots: &[MarketSnapshot],
    ) -> Result<(), StorageError> {
        if snapshots.is_empty() {
            return Ok(());
        }
        let computed_at = Utc::now();
        let mut insert = self
            .clickhouse
            .insert("market_snapshots_buffer")
            .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        for snapshot in snapshots {
            let mut snapshot_json = serde_json::to_value(snapshot).unwrap_or_default();
            if let Some(object) = snapshot_json.as_object_mut() {
                object.insert("computed_at".into(), json!(computed_at));
            }
            insert
                .write(&MarketSnapshotRow {
                    snapshot_id: snapshot.snapshot_id.clone(),
                    market: snapshot.market.to_string(),
                    mint: snapshot.mint.to_string(),
                    quote_mint: snapshot.quote_mint.to_string(),
                    protocol: match snapshot.protocol {
                        pumphound_domain::Protocol::PumpCurve => "pump_curve".into(),
                        pumphound_domain::Protocol::PumpSwap => "pump_swap".into(),
                    },
                    slot: snapshot.as_of.slot.0,
                    tx_index: snapshot.as_of.tx_index,
                    event_index: snapshot.as_of.event_index,
                    observed_at: snapshot.as_of.observed_at,
                    knowledge_cutoff: snapshot.as_of.knowledge_cutoff,
                    computed_at,
                    canonicality: format!("{:?}", snapshot.canonicality).to_lowercase(),
                    virtual_base_atomic: snapshot.virtual_base_atomic.0,
                    virtual_quote_atomic: snapshot.virtual_quote_atomic.0,
                    real_base_atomic: snapshot.real_base_atomic.0,
                    real_quote_atomic: snapshot.real_quote_atomic.0,
                    effective_base_atomic: snapshot.effective_base_atomic.0,
                    effective_quote_atomic: snapshot.effective_quote_atomic.0,
                    config_snapshot_id: snapshot.config_snapshot_id.clone(),
                    quality_json: serde_json::to_string(&snapshot.quality).unwrap_or_default(),
                    snapshot_json: serde_json::to_string(&snapshot_json).unwrap_or_default(),
                })
                .await
                .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        }
        insert
            .end()
            .await
            .map_err(|e| StorageError::Clickhouse(e.to_string()))?;
        Ok(())
    }

    pub async fn load_hot_mints(&self) -> Result<Vec<String>, StorageError> {
        Ok(sqlx::query_scalar::<_, String>(
            r#"WITH valid_hot AS (
                 SELECT hot.mint
                 FROM pumphound.hot_set_memberships hot
                 LEFT JOIN pumphound.launches l ON l.launch_id=hot.launch_id
                 WHERE hot.expires_at > now()
                   AND hot.mint NOT IN (
                     'So11111111111111111111111111111111111111112',
                     'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
                   )
                   AND COALESCE(l.attributes->>'integrity_state','')<>'QUARANTINED'
                   AND NOT EXISTS (
                     SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                     WHERE iq.entity_type='LAUNCH'
                       AND iq.entity_id=hot.launch_id::text
                       AND iq.status='ACTIVE'
                   )
               )
               SELECT mint FROM valid_hot
               UNION
               SELECT markets.market
               FROM valid_hot hot
               JOIN pumphound.markets ON markets.base_mint=hot.mint"#,
        )
        .fetch_all(&self.postgres)
        .await?)
    }

    pub async fn record_checkpoint(&self, cursor: &ChainCursor) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.source_checkpoints
            (source, subscription_id, cursor, completed_slot, updated_at)
            VALUES ($1,$2,$3,$4, now())
            ON CONFLICT (source, subscription_id) DO UPDATE
            SET cursor=EXCLUDED.cursor, completed_slot=EXCLUDED.completed_slot, updated_at=now()"#,
        )
        .bind(&cursor.source)
        .bind(&cursor.subscription_id)
        .bind(serde_json::to_value(cursor).unwrap_or(serde_json::json!({})))
        .bind(cursor.slot.map(|s| s.0 as i64))
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    pub async fn load_checkpoint(
        &self,
        source: &str,
        subscription_id: &str,
    ) -> Result<Option<ChainCursor>, StorageError> {
        let row = sqlx::query(
            "SELECT cursor FROM pumphound.source_checkpoints WHERE source=$1 AND subscription_id=$2",
        )
        .bind(source)
        .bind(subscription_id)
        .fetch_optional(&self.postgres)
        .await?;
        Ok(row.and_then(|r| {
            let v: serde_json::Value = r.get("cursor");
            serde_json::from_value(v).ok()
        }))
    }

    pub async fn publish(
        &self,
        subject: &str,
        payload: &impl Serialize,
    ) -> Result<(), StorageError> {
        let bytes = serde_json::to_vec(payload).map_err(|e| StorageError::Nats(e.to_string()))?;
        self.jetstream
            .publish(subject.to_string(), bytes.into())
            .await
            .map_err(|e| StorageError::Nats(e.to_string()))?
            .await
            .map_err(|e| StorageError::Nats(e.to_string()))?;
        Ok(())
    }

    async fn ensure_asset(
        &self,
        mint: &str,
        decimals: i16,
        symbol: &str,
        slot: i64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.assets (mint, decimals, symbol, name, token_program, first_seen_slot, known_at)
            VALUES ($1,$2,$3,$3,'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',$4, now())
            ON CONFLICT (mint) DO NOTHING"#,
        )
        .bind(mint)
        .bind(decimals)
        .bind(symbol)
        .bind(slot)
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    pub async fn upsert_launch_from_event(&self, ev: &ChainEvent) -> Result<(), StorageError> {
        if let ChainEventPayload::TradeExecuted(t) = &ev.payload {
            let mint = t.mint.to_string();
            let quote = t.quote_mint.to_string();
            let slot = ev.slot.0 as i64;
            let base_decimals = t
                .raw_event
                .get("token_decimals")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| i16::try_from(value).ok())
                .unwrap_or(6);
            let quote_decimals = t
                .raw_event
                .get("quote_decimals")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| i16::try_from(value).ok())
                .unwrap_or_else(|| {
                    if quote == pumphound_domain::USDC_MINT {
                        6
                    } else {
                        9
                    }
                });
            self.ensure_asset(&mint, base_decimals, "UNK", slot).await?;
            let qsym = if quote == pumphound_domain::USDC_MINT {
                "USDC"
            } else {
                "SOL"
            };
            self.ensure_asset(&quote, quote_decimals, qsym, slot)
                .await?;
            let creator = t
                .raw_event
                .get("creator")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // Falling back to the executing trader as a provisional creator is a reasonable
            // guess when this trade arrived before we ever saw the launch's create event. It
            // stops being reasonable when the trader identity is itself unresolved (PumpSwap
            // logs-only decode without the full transaction leaves `trader` as the zeroed
            // Pubkey, which base58-encodes to the System Program address) — that produced
            // launches whose "creator" was really just "11111111111111111111111111111111111111112",
            // a real-looking address for a wallet that never touched this token. The UI already
            // renders an empty creator as Unknown, so fall through to that instead of fabricating one.
            const UNRESOLVED_PUBKEY: &str = "11111111111111111111111111111111111111112";
            let trader_str = t.trader.to_string();
            let creator = if creator.is_empty() && trader_str != UNRESOLVED_PUBKEY {
                trader_str
            } else {
                creator
            };
            let launch_id = Uuid::now_v7();
            sqlx::query(
                r#"INSERT INTO pumphound.launches
                (launch_id, mint, protocol, program_id, creator, creation_event_id, creation_signature,
                 creation_slot, creation_time, known_at, quote_mint, canonicality, attributes)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, now(),$10,'provisional',$11)
                ON CONFLICT (program_id, mint) DO NOTHING"#,
            )
            .bind(launch_id)
            .bind(&mint)
            .bind(match ev.protocol {
                pumphound_domain::Protocol::PumpCurve => "pump_curve",
                pumphound_domain::Protocol::PumpSwap => "pump_swap",
            })
            .bind(ev.program_id.to_string())
            .bind(&creator)
            .bind(&ev.event_id)
            .bind(&ev.signature)
            .bind(slot)
            .bind(ev.block_time)
            .bind(&quote)
            .bind(serde_json::json!({
                "observed_from": "trade",
                "create_event": "missing_in_session",
                "symbol": null,
                "identity_source": t.raw_event.get("identity_source"),
            }))
            .execute(&self.postgres)
            .await?;
            let verified_pumpswap_identity = ev.protocol == pumphound_domain::Protocol::PumpSwap
                && t.raw_event.get("identity_source").and_then(|v| v.as_str())
                    == Some("transaction_instruction_accounts");
            if verified_pumpswap_identity && mint != t.market.to_string() {
                sqlx::query(
                    r#"WITH affected AS (
                         UPDATE pumphound.launches
                         SET attributes=attributes || $1::jsonb
                         WHERE program_id=$2 AND mint=$3
                           AND attributes->>'observed_from'='trade'
                         RETURNING launch_id,mint
                       )
                       INSERT INTO pumphound.entity_integrity_quarantines
                         (quarantine_id,entity_type,entity_id,reason_code,evidence)
                       SELECT
                         md5('LAUNCH:'||launch_id::text||':POOL_ALIAS_AS_MINT')::uuid,
                         'LAUNCH',
                         launch_id::text,
                         'POOL_ALIAS_AS_MINT',
                         jsonb_build_object(
                           'launch_mint',mint,
                           'market',$3::text,
                           'superseded_by_mint',$4::text,
                           'source_rows_preserved',true
                         )
                       FROM affected
                       ON CONFLICT (entity_type,entity_id,reason_code) DO NOTHING"#,
                )
                .bind(serde_json::json!({
                    "integrity_state": "QUARANTINED",
                    "integrity_reason": "pool_alias_misclassified_as_mint",
                    "superseded_by_mint": mint,
                }))
                .bind(ev.program_id.to_string())
                .bind(t.market.to_string())
                .bind(&mint)
                .execute(&self.postgres)
                .await?;
            }
            sqlx::query(
                r#"INSERT INTO pumphound.markets
                (market, launch_id, base_mint, quote_mint, protocol, created_slot, known_at)
                VALUES ($1,(SELECT launch_id FROM pumphound.launches WHERE program_id=$2 AND mint=$3),$3,$4,$5,$6, now())
                ON CONFLICT (market) DO UPDATE SET
                  launch_id=EXCLUDED.launch_id,
                  base_mint=EXCLUDED.base_mint,
                  quote_mint=EXCLUDED.quote_mint,
                  protocol=EXCLUDED.protocol,
                  known_at=EXCLUDED.known_at
                WHERE pumphound.markets.base_mint=pumphound.markets.market
                  AND EXCLUDED.base_mint<>EXCLUDED.market"#,
            )
            .bind(t.market.to_string())
            .bind(ev.program_id.to_string())
            .bind(&mint)
            .bind(&quote)
            .bind(match ev.protocol {
                pumphound_domain::Protocol::PumpCurve => "pump_curve",
                pumphound_domain::Protocol::PumpSwap => "pump_swap",
            })
            .bind(slot)
            .execute(&self.postgres)
            .await?;
            return Ok(());
        }
        if let ChainEventPayload::LaunchCreated(c) = &ev.payload {
            sqlx::query(
                r#"INSERT INTO pumphound.assets (mint, decimals, symbol, name, token_program, metadata_uri, first_seen_slot, known_at)
                VALUES ($1,$2,$3,$4,'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',$5,$6, now())
                ON CONFLICT (mint) DO UPDATE SET symbol=COALESCE(EXCLUDED.symbol, pumphound.assets.symbol),
                name=COALESCE(EXCLUDED.name, pumphound.assets.name)"#,
            )
            .bind(c.mint.to_string())
            .bind(c.base_decimals as i16)
            .bind(&c.symbol)
            .bind(&c.name)
            .bind(&c.metadata_uri)
            .bind(ev.slot.0 as i64)
            .execute(&self.postgres)
            .await?;

            sqlx::query(
                r#"INSERT INTO pumphound.assets (mint, decimals, symbol, name, token_program, first_seen_slot, known_at)
                VALUES ($1,$2,$3,$3,'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',$4, now())
                ON CONFLICT (mint) DO NOTHING"#,
            )
            .bind(c.quote_mint.to_string())
            .bind(c.quote_decimals as i16)
            .bind(if c.quote_mint.to_string() == pumphound_domain::USDC_MINT {
                "USDC"
            } else {
                "SOL"
            })
            .bind(ev.slot.0 as i64)
            .execute(&self.postgres)
            .await?;

            let launch_id = Uuid::parse_str(&c.launch_id).unwrap_or_else(|_| Uuid::now_v7());
            sqlx::query(
                r#"INSERT INTO pumphound.launches
                (launch_id, mint, protocol, program_id, creator, creation_event_id, creation_signature,
                 creation_slot, creation_time, known_at, quote_mint, canonicality, attributes)
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, now(),$10,$11,$12)
                ON CONFLICT (program_id, mint) DO NOTHING"#,
            )
            .bind(launch_id)
            .bind(c.mint.to_string())
            .bind(match ev.protocol {
                pumphound_domain::Protocol::PumpCurve => "pump_curve",
                pumphound_domain::Protocol::PumpSwap => "pump_swap",
            })
            .bind(ev.program_id.to_string())
            .bind(c.creator.map(|creator| creator.to_string()))
            .bind(&ev.event_id)
            .bind(&ev.signature)
            .bind(ev.slot.0 as i64)
            .bind(ev.block_time)
            .bind(c.quote_mint.to_string())
            .bind("provisional")
            .bind(serde_json::json!({
                "flags": c.protocol_flags,
                "name": c.name,
                "symbol": c.symbol,
                "uri": c.metadata_uri,
                "observed_from": "create",
            }))
            .execute(&self.postgres)
            .await?;
            sqlx::query(
                r#"UPDATE pumphound.launches
                SET creator=$1,
                    attributes = attributes || $2::jsonb,
                    canonicality = 'canonical'
                WHERE program_id=$3 AND mint=$4 AND attributes->>'observed_from' = 'trade'"#,
            )
            .bind(c.creator.map(|creator| creator.to_string()))
            .bind(serde_json::json!({
                "flags": c.protocol_flags,
                "name": c.name,
                "symbol": c.symbol,
                "uri": c.metadata_uri,
                "observed_from": "create",
            }))
            .bind(ev.program_id.to_string())
            .bind(c.mint.to_string())
            .execute(&self.postgres)
            .await?;

            sqlx::query(
                r#"INSERT INTO pumphound.markets
                (market, launch_id, base_mint, quote_mint, protocol, created_slot, known_at)
                VALUES ($1,(SELECT launch_id FROM pumphound.launches WHERE program_id=$2 AND mint=$3),$3,$4,$5,$6, now())
                ON CONFLICT (market) DO UPDATE SET
                  launch_id=EXCLUDED.launch_id,
                  base_mint=EXCLUDED.base_mint,
                  quote_mint=EXCLUDED.quote_mint,
                  protocol=EXCLUDED.protocol,
                  created_slot=EXCLUDED.created_slot,
                  known_at=EXCLUDED.known_at"#,
            )
            .bind(c.market.to_string())
            .bind(ev.program_id.to_string())
            .bind(c.mint.to_string())
            .bind(c.quote_mint.to_string())
            .bind(match ev.protocol {
                pumphound_domain::Protocol::PumpCurve => "pump_curve",
                pumphound_domain::Protocol::PumpSwap => "pump_swap",
            })
            .bind(ev.slot.0 as i64)
            .execute(&self.postgres)
            .await?;
        }
        Ok(())
    }

    pub async fn upsert_graph_from_event(&self, ev: &ChainEvent) -> Result<(), StorageError> {
        let slot = ev.slot.0 as i64;
        match &ev.payload {
            ChainEventPayload::LaunchCreated(c) => {
                let mint = c.mint.to_string();
                let launch = format!("launch:{}", c.launch_id);
                let token = format!("token:{mint}");
                let market = format!("market:{}", c.market);
                self.ensure_graph_node(
                    &token,
                    "Token",
                    Some(&mint),
                    slot,
                    json!({"quote_mint": c.quote_mint.to_string()}),
                )
                .await?;
                self.ensure_graph_node(
                    &launch,
                    "Launch",
                    None,
                    slot,
                    json!({"mint": mint, "creator": c.creator.map(|creator| creator.to_string())}),
                )
                .await?;
                self.ensure_graph_node(
                    &market,
                    "Market",
                    Some(&c.market.to_string()),
                    slot,
                    json!({}),
                )
                .await?;
                if let Some(creator) = c.creator {
                    let creator = creator.to_string();
                    let wallet = format!("wallet:{creator}");
                    self.ensure_graph_node(
                        &wallet,
                        "Wallet",
                        Some(&creator),
                        slot,
                        json!({"role":"creator"}),
                    )
                    .await?;
                    self.insert_graph_fact(&wallet, &launch, "CREATED", ev, Some(&mint), None)
                        .await?;
                }
                self.insert_graph_fact(&launch, &token, "ISSUED", ev, Some(&mint), None)
                    .await?;
                self.insert_graph_fact(&token, &market, "TRADED_ON", ev, Some(&mint), None)
                    .await?;
            }
            ChainEventPayload::TradeExecuted(t) => {
                let trader = t.trader.to_string();
                let mint = t.mint.to_string();
                let wallet = format!("wallet:{trader}");
                let token = format!("token:{mint}");
                let edge = if t.side == pumphound_domain::TradeSide::Buy {
                    "BOUGHT"
                } else {
                    "SOLD"
                };
                self.ensure_graph_node(&wallet, "Wallet", Some(&trader), slot, json!({}))
                    .await?;
                self.ensure_graph_node(&token, "Token", Some(&mint), slot, json!({}))
                    .await?;
                self.insert_graph_fact(
                    &wallet,
                    &token,
                    edge,
                    ev,
                    Some(&mint),
                    Some(t.quote_amount_atomic.0),
                )
                .await?;
            }
            ChainEventPayload::LaunchMigrated(c) => {
                let mint = c.mint.to_string();
                let launch = format!("launch:{}", c.launch_id);
                let token = format!("token:{mint}");
                let old_m = format!("market:{}", c.old_market);
                let new_m = format!("market:{}", c.new_market);
                self.ensure_graph_node(
                    &launch,
                    "Launch",
                    None,
                    slot,
                    json!({"mint": mint, "migrated": true}),
                )
                .await?;
                self.ensure_graph_node(&token, "Token", Some(&mint), slot, json!({}))
                    .await?;
                self.ensure_graph_node(
                    &old_m,
                    "Market",
                    Some(&c.old_market.to_string()),
                    slot,
                    json!({}),
                )
                .await?;
                self.ensure_graph_node(
                    &new_m,
                    "Market",
                    Some(&c.new_market.to_string()),
                    slot,
                    json!({"migrated": true}),
                )
                .await?;
                self.insert_graph_fact(&launch, &new_m, "MIGRATED_TO", ev, Some(&mint), None)
                    .await?;
                self.insert_graph_fact(&token, &new_m, "TRADED_ON", ev, Some(&mint), None)
                    .await?;
            }
            ChainEventPayload::LaunchCompleted(c) => {
                let mint = c.mint.to_string();
                let launch = format!("launch:{}", c.launch_id);
                let market = format!("market:{}", c.market);
                self.ensure_graph_node(
                    &launch,
                    "Launch",
                    None,
                    slot,
                    json!({"mint": mint, "completed": true}),
                )
                .await?;
                self.ensure_graph_node(
                    &market,
                    "Market",
                    Some(&c.market.to_string()),
                    slot,
                    json!({}),
                )
                .await?;
                self.insert_graph_fact(&launch, &market, "COMPLETED", ev, Some(&mint), None)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn ensure_graph_node(
        &self,
        node_id: &str,
        node_type: &str,
        public_key: Option<&str>,
        slot: i64,
        attributes: serde_json::Value,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.graph_nodes
            (node_id, node_type, public_key, first_seen_slot, known_at, attributes)
            VALUES ($1,$2,$3,$4, now(), $5)
            ON CONFLICT (node_id) DO UPDATE SET
              attributes = pumphound.graph_nodes.attributes || EXCLUDED.attributes"#,
        )
        .bind(node_id)
        .bind(node_type)
        .bind(public_key)
        .bind(slot)
        .bind(attributes)
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    async fn insert_graph_fact(
        &self,
        source: &str,
        target: &str,
        edge_type: &str,
        ev: &ChainEvent,
        mint: Option<&str>,
        amount: Option<u128>,
    ) -> Result<(), StorageError> {
        let fact_id = format!("{}:{}:{}:{}", ev.event_id, edge_type, source, target);
        sqlx::query(
            r#"INSERT INTO pumphound.graph_facts
            (fact_id, source_node, target_node, edge_type, event_id, slot, known_at, canonicality, asset_mint, amount_atomic, attributes)
            VALUES ($1,$2,$3,$4,$5,$6, now(), 'provisional', $7, $8::numeric, $9)
            ON CONFLICT (event_id, source_node, target_node, edge_type) DO NOTHING"#,
        )
        .bind(&fact_id)
        .bind(source)
        .bind(target)
        .bind(edge_type)
        .bind(&ev.event_id)
        .bind(ev.slot.0 as i64)
        .bind(mint)
        .bind(amount.map(|a| a.to_string()))
        .bind(serde_json::json!({"signature": ev.signature}))
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    pub async fn claim_event_key(&self, ev: &ChainEvent) -> Result<bool, StorageError> {
        let (kind, _, _, _, _, _, _) = flatten(ev);
        let ix = ev.instruction_path.first().copied().unwrap_or(0);
        let inner = ev
            .instruction_path
            .get(1)
            .copied()
            .unwrap_or(ev.event_index);
        let economic = pumphound_domain::economic_event_key(&ev.signature, ix, inner, &kind);
        let action = format!("{:?}", ev.action).to_uppercase();
        let res = sqlx::query(
            r#"INSERT INTO pumphound.canonical_event_keys
            (event_id, economic_key, signature, event_kind, slot, highest_revision, last_action, last_seen_at)
            VALUES ($1,$2,$3,$4,$5,$6,$7,now())
            ON CONFLICT (event_id) DO UPDATE SET
              economic_key=EXCLUDED.economic_key,
              signature=EXCLUDED.signature,
              event_kind=EXCLUDED.event_kind,
              slot=EXCLUDED.slot,
              highest_revision=EXCLUDED.highest_revision,
              last_action=EXCLUDED.last_action,
              last_seen_at=now()
            WHERE EXCLUDED.highest_revision>pumphound.canonical_event_keys.highest_revision"#,
        )
        .bind(&ev.event_id)
        .bind(&economic)
        .bind(&ev.signature)
        .bind(&kind)
        .bind(ev.slot.0 as i64)
        .bind(ev.revision as i64)
        .bind(action)
        .execute(&self.postgres)
        .await;
        match res {
            Ok(r) => Ok(r.rows_affected() > 0),
            Err(e) => {
                if e.as_database_error()
                    .and_then(|d| d.code())
                    .is_some_and(|c| c == "23505")
                {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    pub async fn archive_raw(
        &self,
        data_dir: &str,
        envelope: &RawChainEnvelope,
        tx: &pumphound_domain::RawTransaction,
    ) -> Result<String, StorageError> {
        let day = envelope.observed_at.format("%Y-%m-%d").to_string();
        let rel = format!("archive/raw/date={day}/{}.json", tx.signature);
        let path = format!("{data_dir}/{rel}");
        if let Some(parent) = std::path::Path::new(&path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::Config(e.to_string()))?;
        }
        let body = serde_json::json!({
            "sighting_id": envelope.sighting_id,
            "source": envelope.source,
            "source_cursor": envelope.source_cursor,
            "slot": envelope.slot.to_string(),
            "commitment": format!("{:?}", envelope.commitment).to_lowercase(),
            "observed_at": envelope.observed_at,
            "payload_sha256": envelope.payload_sha256,
            "decoder_version": pumphound_domain::DECODER_VERSION,
            "transaction": tx,
        });
        tokio::fs::write(&path, serde_json::to_vec_pretty(&body).unwrap_or_default())
            .await
            .map_err(|e| StorageError::Config(e.to_string()))?;
        sqlx::query(
            r#"INSERT INTO pumphound.raw_archive_index
            (signature, slot, payload_sha256, archive_uri, source, decoder_version)
            VALUES ($1,$2,$3,$4,$5,$6)
            ON CONFLICT (signature) DO NOTHING"#,
        )
        .bind(&tx.signature)
        .bind(tx.slot.0 as i64)
        .bind(&envelope.payload_sha256)
        .bind(&rel)
        .bind(&envelope.source)
        .bind(pumphound_domain::DECODER_VERSION)
        .execute(&self.postgres)
        .await?;
        Ok(rel)
    }

    pub async fn refresh_intelligence(&self) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.wallet_profiles
            (wallet, first_seen_slot, last_seen_slot, launches_entered, first10_appearances, buys, sells,
             creator_linked_launches, pnl_status, updated_at)
            SELECT replace(source_node, 'wallet:', ''),
                   MIN(slot), MAX(slot),
                   COUNT(DISTINCT asset_mint) FILTER (WHERE edge_type IN ('BOUGHT','SOLD')),
                   0,
                   COUNT(*) FILTER (WHERE edge_type='BOUGHT'),
                   COUNT(*) FILTER (WHERE edge_type='SOLD'),
                   0,
                   'INCOMPLETE',
                   now()
            FROM pumphound.graph_facts
            WHERE source_node LIKE 'wallet:%'
            GROUP BY source_node
            ON CONFLICT (wallet) DO UPDATE SET
              last_seen_slot = EXCLUDED.last_seen_slot,
              launches_entered = EXCLUDED.launches_entered,
              buys = EXCLUDED.buys,
              sells = EXCLUDED.sells,
              updated_at = now()"#,
        )
        .execute(&self.postgres)
        .await?;
        sqlx::query(
            r#"INSERT INTO pumphound.creator_profiles
            (creator, launches, first_seen_slot, last_seen_slot, connected_buyers, pnl_status, updated_at)
            SELECT creator, COUNT(*), MIN(creation_slot), MAX(creation_slot), 0, 'INCOMPLETE', now()
            FROM pumphound.launches
            GROUP BY creator
            ON CONFLICT (creator) DO UPDATE SET
              launches = EXCLUDED.launches,
              last_seen_slot = EXCLUDED.last_seen_slot,
              updated_at = now()"#,
        )
        .execute(&self.postgres)
        .await?;
        sqlx::query(
            r#"INSERT INTO pumphound.funding_queue (wallet, priority, reason, status)
            SELECT creator, 100, 'creator', 'queued' FROM pumphound.launches
            ON CONFLICT (wallet) DO NOTHING"#,
        )
        .execute(&self.postgres)
        .await?;
        sqlx::query(
            r#"INSERT INTO pumphound.funding_queue (wallet, priority, reason, status)
            SELECT replace(source_node, 'wallet:', ''),
                   CASE WHEN rn <= 5 THEN 100 ELSE 80 END,
                   CASE WHEN rn <= 5 THEN 'first5_buyer' ELSE 'buyer_6_10' END,
                   'queued'
            FROM (
              SELECT source_node,
                     ROW_NUMBER() OVER (PARTITION BY asset_mint ORDER BY slot ASC, event_id) AS rn
              FROM pumphound.graph_facts
              WHERE edge_type = 'BOUGHT'
                AND source_node LIKE 'wallet:%'
                AND asset_mint IS NOT NULL
            ) ranked
            WHERE rn <= 10
            ON CONFLICT (wallet) DO NOTHING"#,
        )
        .execute(&self.postgres)
        .await?;
        sqlx::query(
            r#"UPDATE pumphound.creator_profiles c SET connected_buyers = COALESCE((
              SELECT COUNT(DISTINCT f.source_node)
              FROM pumphound.launches l
              JOIN pumphound.graph_facts f ON f.asset_mint = l.mint AND f.edge_type='BOUGHT'
              WHERE l.creator = c.creator
            ),0)"#,
        )
        .execute(&self.postgres)
        .await?;
        // Co-buy pairs are computed from a self-join of graph_facts per mint, which is
        // O(buyers^2) per mint. Sniper/bot-farmed mints can have tens of thousands of
        // repeat BOUGHT events from a couple thousand wallets, which blew this join up
        // to hundreds of millions of intermediate rows and stalled Postgres for 20+
        // minutes at a time. Dedupe to one (wallet, mint) row and cap to the earliest
        // 200 distinct wallets per mint (the coordinated-early-buying signal this edge
        // is meant to capture) so the join stays bounded regardless of bot volume.
        sqlx::query(
            r#"WITH per_wallet AS (
              SELECT source_node, asset_mint, MIN(slot) AS slot,
                     (array_agg(event_id ORDER BY slot ASC))[1] AS event_id
              FROM pumphound.graph_facts
              WHERE edge_type='BOUGHT' AND asset_mint IS NOT NULL
              GROUP BY source_node, asset_mint
            ), capped AS (
              SELECT source_node, asset_mint, slot, event_id,
                     ROW_NUMBER() OVER (PARTITION BY asset_mint ORDER BY slot ASC) AS rn
              FROM per_wallet
            ), bounded AS (
              SELECT source_node, asset_mint, slot, event_id FROM capped WHERE rn <= 200
            )
            INSERT INTO pumphound.derived_edges
            (edge_id, source_node, target_node, edge_type, score, evidence_count,
             first_observed_slot, last_observed_slot, algorithm_version, supporting_signatures)
            SELECT md5(a.source_node || b.source_node || 'CO_BOUGHT_WITH'),
                   a.source_node, b.source_node, 'CO_BOUGHT_WITH',
                   LEAST(1.0, COUNT(DISTINCT a.asset_mint)::float / 5.0),
                   COUNT(DISTINCT a.asset_mint)::int,
                   MIN(LEAST(a.slot, b.slot)), MAX(GREATEST(a.slot, b.slot)),
                   'co-buy-v1',
                   jsonb_agg(DISTINCT a.event_id)
            FROM bounded a
            JOIN bounded b
              ON a.asset_mint = b.asset_mint
             AND a.source_node < b.source_node
            GROUP BY a.source_node, b.source_node
            HAVING COUNT(DISTINCT a.asset_mint) >= 2
            ON CONFLICT (source_node, target_node, edge_type, algorithm_version) DO UPDATE SET
              evidence_count = EXCLUDED.evidence_count,
              last_observed_slot = EXCLUDED.last_observed_slot,
              score = EXCLUDED.score,
              supporting_signatures = EXCLUDED.supporting_signatures"#,
        )
        .execute(&self.postgres)
        .await?;
        self.refresh_creator_coordination_risk().await?;
        self.refresh_holder_concentration().await?;
        self.rebuild_cohorts().await?;
        Ok(())
    }

    /// Writes `creator_profiles.attributes->'risk_score'`, the field
    /// `paper_loop::intelligence_indicators` already reads into `OpportunityInput.creator_risk`
    /// and gates entries against (`maximum_creator_risk` in the autonomous policy) — that gate
    /// has existed the whole time, it just never had anything writing the number it checks, so
    /// every creator silently defaulted to "no signal" and the gate could never fire. This is the
    /// same repeat-early-buyer-concentration analysis the Creator page's coordination panel
    /// already shows a human (routes.rs::creator_coordination), computed here in one batched pass
    /// over every creator instead of one HTTP call per creator, and fed back into the actual
    /// trading decision instead of only ever being read on a page a person opens.
    ///
    /// Score is the repeat-buyer concentration fraction directly (repeat early-buyer wallets ÷
    /// distinct early buyers), already naturally bounded to [0,1] — no extra scaling. Creators
    /// with fewer than 3 non-quarantined launches are left with no score at all (NULL, not 0)
    /// rather than a fabricated "safe" default; `intelligence_indicators` already treats a
    /// missing score as `unwrap_or(1.0)` (maximum caution) on the gating side, so "no evidence"
    /// correctly reads as "unproven", not as "clean".
    async fn refresh_creator_coordination_risk(&self) -> Result<(), StorageError> {
        sqlx::query(
            r#"WITH creator_launches AS (
              SELECT creator, mint FROM pumphound.launches
              WHERE creator <> ''
                AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
            ), ranked AS (
              SELECT cl.creator, gf.asset_mint AS mint, gf.source_node AS wallet,
                     row_number() OVER (PARTITION BY gf.asset_mint ORDER BY gf.slot ASC) AS rn
              FROM pumphound.graph_facts gf
              JOIN creator_launches cl ON cl.mint = gf.asset_mint
              WHERE gf.edge_type = 'BOUGHT'
            ), early AS (
              SELECT creator, mint, wallet FROM ranked WHERE rn <= 10
            ), per_creator AS (
              SELECT creator, count(DISTINCT mint) AS launches, count(DISTINCT wallet) AS distinct_buyers
              FROM early GROUP BY creator
            ), repeat_buyers AS (
              SELECT creator, wallet FROM early
              GROUP BY creator, wallet
              HAVING count(DISTINCT mint) >= 2
            ), repeat_counts AS (
              SELECT creator, count(*) AS repeat_wallets FROM repeat_buyers GROUP BY creator
            )
            UPDATE pumphound.creator_profiles cp
            SET attributes = attributes || jsonb_build_object(
                  'risk_score', ROUND(LEAST(1.0, COALESCE(rc.repeat_wallets,0)::numeric / NULLIF(pc.distinct_buyers,0)), 4),
                  'risk_score_basis', 'repeat_early_buyer_concentration_v1',
                  'risk_score_launches_examined', pc.launches,
                  'risk_score_computed_at', now()
                )
            FROM per_creator pc
            LEFT JOIN repeat_counts rc ON rc.creator = pc.creator
            WHERE cp.creator = pc.creator AND pc.launches >= 3"#,
        )
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    /// Writes `launches.attributes->'holder_top_1_share'`, a field `research::LocalGraphInput`
    /// has read (and gated `evaluate_local_graph` against, via `GRAPH_TOP1_AVOID`) since before
    /// this session — like `risk_score` above, the gate existed but nothing computed the number.
    /// pump.fun's bonding curve mints supply only as buyers pay into the curve (no creator
    /// premine), so summing BOUGHT minus SOLD atomic amounts per (wallet, mint) from graph_facts
    /// reconstructs each wallet's net tracked position without needing a balance oracle or a new
    /// collector decoder. Top-1 share of that tracked total is the classic single-wallet rug/dump
    /// exposure signal: one wallet holding most of a token's circulating supply can crash price
    /// unilaterally regardless of how healthy the rest of the holder base looks.
    ///
    /// Mints with fewer than 5 tracked net-positive holders are left unscored (NULL) rather than
    /// given a fabricated share — with 1-2 holders the "top-1 share" is trivially close to 1.0
    /// and would just be noise, not signal. `holder_concentration_computed_at` is stored
    /// alongside the score (not relying on a table-level `updated_at`, since `launches` has none)
    /// so callers evaluating a historical decision can bound the read to what was known as of
    /// that decision's knowledge cutoff, the same no-lookahead discipline the rest of this
    /// pipeline already enforces.
    async fn refresh_holder_concentration(&self) -> Result<(), StorageError> {
        sqlx::query(
            r#"WITH net_positions AS (
              SELECT source_node AS wallet, asset_mint AS mint,
                     SUM(CASE WHEN edge_type='BOUGHT' THEN amount_atomic ELSE -amount_atomic END) AS net_amount
              FROM pumphound.graph_facts
              WHERE edge_type IN ('BOUGHT','SOLD') AND asset_mint IS NOT NULL
              GROUP BY source_node, asset_mint
              HAVING SUM(CASE WHEN edge_type='BOUGHT' THEN amount_atomic ELSE -amount_atomic END) > 0
            ), ranked AS (
              SELECT mint, wallet, net_amount,
                     row_number() OVER (PARTITION BY mint ORDER BY net_amount DESC) AS rn,
                     SUM(net_amount) OVER (PARTITION BY mint) AS tracked_supply,
                     count(*) OVER (PARTITION BY mint) AS holder_count
              FROM net_positions
            ), top1 AS (
              SELECT mint, net_amount AS top1_amount, tracked_supply, holder_count
              FROM ranked WHERE rn = 1
            )
            UPDATE pumphound.launches l
            SET attributes = attributes || jsonb_build_object(
                  'holder_top_1_share', ROUND(LEAST(1.0, top1.top1_amount::numeric / NULLIF(top1.tracked_supply,0)), 4),
                  'holder_concentration_basis', 'top1_net_tracked_share_v1',
                  'holder_concentration_holders_tracked', top1.holder_count,
                  'holder_concentration_computed_at', now()
                )
            FROM top1
            WHERE l.mint = top1.mint AND top1.holder_count >= 5"#,
        )
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    pub async fn enqueue_funding_if_absent(
        &self,
        wallet: &str,
        priority: i32,
        reason: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"INSERT INTO pumphound.funding_queue (wallet, priority, reason, status)
            VALUES ($1,$2,$3,'queued')
            ON CONFLICT (wallet) DO NOTHING"#,
        )
        .bind(wallet)
        .bind(priority)
        .bind(reason)
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    pub async fn claim_funding_batch(&self, limit: i64) -> Result<Vec<String>, StorageError> {
        let mut tx = self.postgres.begin().await?;
        let rows = sqlx::query(
            r#"UPDATE pumphound.funding_queue q
            SET status = 'running', attempts = attempts + 1, updated_at = now()
            WHERE q.wallet IN (
              SELECT wallet FROM pumphound.funding_queue
              WHERE status = 'queued'
              ORDER BY priority DESC, updated_at ASC
              LIMIT $1
              FOR UPDATE SKIP LOCKED
            )
            RETURNING q.wallet"#,
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.iter().map(|r| r.get::<String, _>("wallet")).collect())
    }

    pub async fn mark_funding_status(
        &self,
        wallet: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"UPDATE pumphound.funding_queue
            SET status = $2, last_error = $3, updated_at = now()
            WHERE wallet = $1"#,
        )
        .bind(wallet)
        .bind(status)
        .bind(last_error)
        .execute(&self.postgres)
        .await?;
        Ok(())
    }

    /// Durable pair components from CO_BOUGHT_WITH (evidence_count >= 2).
    /// Cohort membership is behavioural, not ownership.
    pub async fn rebuild_cohorts(&self) -> Result<u64, StorageError> {
        let edges = sqlx::query(
            r#"SELECT source_node, target_node, score, evidence_count,
                      first_observed_slot, last_observed_slot, supporting_signatures
               FROM pumphound.derived_edges
               WHERE edge_type = 'CO_BOUGHT_WITH' AND evidence_count >= 2"#,
        )
        .fetch_all(&self.postgres)
        .await?;

        let mut uf = UnionFind::default();
        let mut pairs: Vec<CohortPair> = Vec::new();
        for r in &edges {
            let src: String = r.get("source_node");
            let tgt: String = r.get("target_node");
            let Some(a) = wallet_from_node(&src) else {
                continue;
            };
            let Some(b) = wallet_from_node(&tgt) else {
                continue;
            };
            uf.union(&a, &b);
            pairs.push(CohortPair {
                a,
                b,
                score: r.try_get::<f64, _>("score").unwrap_or(0.0),
                evidence_count: r.try_get::<i32, _>("evidence_count").unwrap_or(0),
                first_slot: r.try_get::<i64, _>("first_observed_slot").unwrap_or(0),
                last_slot: r.try_get::<i64, _>("last_observed_slot").unwrap_or(0),
                signatures: r
                    .try_get::<serde_json::Value, _>("supporting_signatures")
                    .unwrap_or(json!([])),
            });
        }

        let groups = uf.groups();
        let mut tx = self.postgres.begin().await?;
        sqlx::query(
            "DELETE FROM pumphound.wallet_cohorts WHERE algorithm_version = 'union-find-v1'",
        )
        .execute(&mut *tx)
        .await?;

        let mut written = 0u64;
        for members in groups.into_values() {
            if members.len() < 2 {
                continue;
            }
            let mut members = members;
            members.sort();
            let seed = members.join("|");
            let cohort_id = format!("uf1:{}", pumphound_domain::sha256_hex(seed.as_bytes()));
            let mut first_slot = i64::MAX;
            let mut last_slot = 0i64;
            let mut score_sum = 0.0;
            let mut score_n = 0u32;
            let mut sigs: Vec<serde_json::Value> = Vec::new();
            for p in &pairs {
                if members.binary_search(&p.a).is_ok() && members.binary_search(&p.b).is_ok() {
                    first_slot = first_slot.min(p.first_slot);
                    last_slot = last_slot.max(p.last_slot);
                    score_sum += p.score;
                    score_n += 1;
                    if let Some(arr) = p.signatures.as_array() {
                        sigs.extend(arr.iter().cloned());
                    }
                }
            }
            if first_slot == i64::MAX {
                first_slot = 0;
            }
            let score = if score_n == 0 {
                0.0
            } else {
                score_sum / f64::from(score_n)
            };
            let evidence = dedupe_json_list(sigs);
            sqlx::query(
                r#"INSERT INTO pumphound.wallet_cohorts
                (cohort_id, algorithm_version, member_count, score, first_slot, last_slot,
                 evidence_signatures, attributes)
                VALUES ($1,'union-find-v1',$2,$3,$4,$5,$6,$7)
                ON CONFLICT (cohort_id) DO UPDATE SET
                  algorithm_version = EXCLUDED.algorithm_version,
                  member_count = EXCLUDED.member_count,
                  score = EXCLUDED.score,
                  first_slot = EXCLUDED.first_slot,
                  last_slot = EXCLUDED.last_slot,
                  evidence_signatures = EXCLUDED.evidence_signatures,
                  attributes = EXCLUDED.attributes"#,
            )
            .bind(&cohort_id)
            .bind(members.len() as i32)
            .bind(score)
            .bind(first_slot)
            .bind(last_slot)
            .bind(&evidence)
            .bind(json!({
                "kind": "co_bought_component",
                "not_ownership": true,
                "algorithm": "union-find-v1"
            }))
            .execute(&mut *tx)
            .await?;
            for w in &members {
                sqlx::query(
                    r#"INSERT INTO pumphound.cohort_members (cohort_id, wallet)
                    VALUES ($1,$2) ON CONFLICT DO NOTHING"#,
                )
                .bind(&cohort_id)
                .bind(w)
                .execute(&mut *tx)
                .await?;
            }
            written += 1;
        }

        sqlx::query(
            r#"UPDATE pumphound.wallet_profiles p
            SET cohort_ids = COALESCE((
              SELECT jsonb_agg(m.cohort_id ORDER BY m.cohort_id)
              FROM pumphound.cohort_members m
              WHERE m.wallet = p.wallet
            ), '[]'::jsonb)"#,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(written)
    }

    pub async fn capture_metrics(&self) -> Result<serde_json::Value, StorageError> {
        let archived: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.raw_archive_index")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);
        let unique_events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.canonical_event_keys")
                .fetch_one(&self.postgres)
                .await
                .unwrap_or(0);
        let wallets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.wallet_profiles")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);
        let queued: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pumphound.funding_queue WHERE status='queued'",
        )
        .fetch_one(&self.postgres)
        .await
        .unwrap_or(0);
        let cohorts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.wallet_cohorts")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);
        let derived: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.derived_edges")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);
        Ok(json!({
            "archive_rows": archived,
            "unique_events": unique_events,
            "wallet_profiles": wallets,
            "funding_queued": queued,
            "cohorts": cohorts,
            "derived_edges": derived,
            "model": "MODEL NOT TRAINED"
        }))
    }
}

#[derive(clickhouse::Row, Serialize, Deserialize)]
struct MarketSnapshotRow {
    snapshot_id: String,
    market: String,
    mint: String,
    quote_mint: String,
    protocol: String,
    slot: u64,
    tx_index: Option<u32>,
    event_index: Option<u32>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    observed_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    knowledge_cutoff: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    computed_at: DateTime<Utc>,
    canonicality: String,
    virtual_base_atomic: u128,
    virtual_quote_atomic: u128,
    real_base_atomic: u128,
    real_quote_atomic: u128,
    effective_base_atomic: u128,
    effective_quote_atomic: u128,
    config_snapshot_id: String,
    quality_json: String,
    snapshot_json: String,
}

#[derive(clickhouse::Row, Serialize, Deserialize)]
struct RawSightingRow {
    sighting_id: String,
    #[serde(with = "clickhouse::serde::uuid")]
    batch_id: Uuid,
    source: String,
    source_cursor: String,
    slot: u64,
    signature: String,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    observed_at: DateTime<Utc>,
    commitment: String,
    payload_sha256: String,
    payload_codec: String,
    raw_payload: String,
    raw_partition_uri: String,
}

#[derive(clickhouse::Row, Serialize, Deserialize)]
struct CanonicalRow {
    event_id: String,
    revision: u64,
    action: u8,
    protocol: String,
    kind: String,
    program_id: String,
    signature: String,
    slot: u64,
    tx_index: Option<u32>,
    instruction_path: Vec<u32>,
    event_index: u32,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros::option")]
    block_time: Option<DateTime<Utc>>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    observed_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    computed_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
    known_at: DateTime<Utc>,
    commitment: String,
    canonicality: String,
    transaction_succeeded: bool,
    mint: String,
    market: String,
    trader: String,
    quote_mint: String,
    base_amount_atomic: u128,
    quote_amount_atomic: u128,
    decoder_version: String,
    idl_sha256: String,
    payload_json: String,
    raw_partition_uri: String,
}

fn flatten(ev: &ChainEvent) -> (String, String, String, String, String, u128, u128) {
    match &ev.payload {
        ChainEventPayload::LaunchCreated(c) => (
            "launch.created".into(),
            c.mint.to_string(),
            c.market.to_string(),
            c.creator
                .map(|creator| creator.to_string())
                .unwrap_or_default(),
            c.quote_mint.to_string(),
            0,
            0,
        ),
        ChainEventPayload::TradeExecuted(t) => (
            "trade.executed".into(),
            t.mint.to_string(),
            t.market.to_string(),
            t.trader.to_string(),
            t.quote_mint.to_string(),
            t.base_amount_atomic.0,
            t.quote_amount_atomic.0,
        ),
        ChainEventPayload::LaunchMigrated(m) => (
            "launch.migrated".into(),
            m.mint.to_string(),
            m.new_market.to_string(),
            String::new(),
            m.quote_mint.to_string(),
            0,
            0,
        ),
        ChainEventPayload::LaunchCompleted(c) => (
            "launch.completed".into(),
            c.mint.to_string(),
            c.market.to_string(),
            String::new(),
            String::new(),
            0,
            0,
        ),
        ChainEventPayload::LiquidityChanged(l) => (
            "liquidity.changed".into(),
            l.mint.to_string(),
            l.market.to_string(),
            String::new(),
            String::new(),
            l.base_delta_atomic.0,
            l.quote_delta_atomic.0,
        ),
        ChainEventPayload::DecodeQuarantined(_) => (
            "decode.quarantined".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            0,
            0,
        ),
        other => (
            format!("{other:?}")
                .split('(')
                .next()
                .unwrap_or("unknown")
                .to_string(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            0,
            0,
        ),
    }
}

pub fn journal_path(data_dir: &str, batch_id: Uuid) -> String {
    format!("{data_dir}/journal/{batch_id}.json")
}

pub async fn append_journal(path: &str, batch: &DurableBatch) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec_pretty(batch)?;
    tokio::fs::write(path, bytes).await?;
    Ok(())
}

#[allow(dead_code)]
pub fn _slot(s: Slot) -> u64 {
    s.0
}

fn wallet_from_node(node: &str) -> Option<String> {
    if let Some(rest) = node.strip_prefix("wallet:") {
        if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        }
    } else if node.contains(':') {
        None
    } else if node.is_empty() {
        None
    } else {
        Some(node.to_string())
    }
}

fn dedupe_json_list(items: Vec<serde_json::Value>) -> serde_json::Value {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        let key = item.to_string();
        if seen.insert(key) {
            out.push(item);
        }
    }
    serde_json::Value::Array(out)
}

struct CohortPair {
    a: String,
    b: String,
    score: f64,
    #[allow(dead_code)]
    evidence_count: i32,
    first_slot: i64,
    last_slot: i64,
    signatures: serde_json::Value,
}

#[derive(Default)]
struct UnionFind {
    parent: HashMap<String, String>,
}

impl UnionFind {
    fn add(&mut self, x: &str) {
        self.parent
            .entry(x.to_string())
            .or_insert_with(|| x.to_string());
    }

    fn find(&mut self, x: &str) -> String {
        let mut cur = x.to_string();
        loop {
            let parent = self
                .parent
                .get(&cur)
                .cloned()
                .unwrap_or_else(|| cur.clone());
            if parent == cur {
                self.parent.entry(cur.clone()).or_insert(cur.clone());
                return cur;
            }
            let grand = self
                .parent
                .get(&parent)
                .cloned()
                .unwrap_or_else(|| parent.clone());
            self.parent.insert(cur, grand.clone());
            cur = parent;
        }
    }

    fn union(&mut self, a: &str, b: &str) {
        self.add(a);
        self.add(b);
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }

    fn groups(&mut self) -> HashMap<String, Vec<String>> {
        let keys: Vec<String> = self.parent.keys().cloned().collect();
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for k in keys {
            let root = self.find(&k);
            out.entry(root).or_default().push(k);
        }
        out
    }
}
