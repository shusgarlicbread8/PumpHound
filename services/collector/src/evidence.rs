use anyhow::Context;
use chrono::{DateTime, Duration, Utc};
use pumphound_chain_source::{
    backfill_mint_early_life, envelope_from_tx, fetch_block_times, fetch_transaction,
    MintBackfillCaps,
};
use pumphound_domain::{
    interpolate_slot_time, payload_sha256, sha256_hex, ChainEventPayload, Commitment, DataQuality,
    DurableBatch, FeatureVector, LaunchpadDecoder, ObservationPoint, RawChainEnvelope,
    RawTransaction, Slot,
};
use pumphound_feature_core::{TradeTick, WindowAggregator};
use pumphound_protocol_pump::PumpBondingCurveDecoder;
use pumphound_protocol_pumpswap::{
    remember_from_event, replace_pool_identities, unresolved_pool_from_reason, CachedPoolIdentity,
    PumpSwapDecoder,
};
use pumphound_storage::{EvidenceWriter, Stores};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashSet;
use std::path::Path;
use tracing::{info, warn};
use uuid::Uuid;

pub const DECISION_AGES_SECONDS: [u32; 6] = [0, 1, 3, 10, 30, 60];
pub const DECISION_AGE_SCHEMA: &str = "decision-age-v1";

#[derive(Clone, Debug)]
pub struct RepairConfig {
    pub launch_times: bool,
    pub identities: bool,
    pub pumpswap: bool,
    pub mint_backfill: bool,
    pub snapshots: bool,
    pub import_corpus: bool,
    pub max_launch_time_fetches: usize,
    pub max_unknown_pools: usize,
    pub max_mints: usize,
    pub max_snapshots: usize,
    pub history_days: i64,
    pub corpus_dir: Option<String>,
    pub skip_block_time_rpc: bool,
}

impl Default for RepairConfig {
    fn default() -> Self {
        Self {
            launch_times: true,
            identities: true,
            pumpswap: true,
            mint_backfill: true,
            snapshots: true,
            import_corpus: true,
            max_launch_time_fetches: 4_000,
            max_unknown_pools: 400,
            max_mints: 800,
            max_snapshots: 8_000,
            history_days: 90,
            corpus_dir: None,
            skip_block_time_rpc: false,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct RepairReport {
    pub pool_identities_loaded: usize,
    pub quote_mint_note: &'static str,
    pub launch_times_fetched: usize,
    pub launch_times_block_time: usize,
    pub launch_times_interpolated: usize,
    pub pumpswap_redecoded: usize,
    pub pumpswap_fetched: usize,
    pub mint_backfills: usize,
    pub mint_transactions: usize,
    pub snapshots_written: usize,
    pub corpus_events_imported: usize,
    pub coverage: Value,
}

pub async fn run(stores: &Stores, config: RepairConfig) -> anyhow::Result<RepairReport> {
    let mut report = RepairReport {
        quote_mint_note: "applied by 020_evidence_layer.sql when present",
        ..RepairReport::default()
    };
    let source = pumphound_chain_source::SourceConfig::from_env();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .context("repair http client")?;
    let evidence = EvidenceWriter::spawn(
        std::env::var("PUMPHOUND_DATA_DIR").unwrap_or_else(|_| "./data".into()),
    );

    if config.identities {
        report.pool_identities_loaded = load_identity_cache(stores).await?;
        info!(
            identities = report.pool_identities_loaded,
            "evidence repair: pool identity cache loaded"
        );
    }

    if config.launch_times {
        let (fetched, block_times, interpolated) = repair_launch_times(
            stores,
            &http,
            &source.rpc_url,
            config.max_launch_time_fetches,
            config.skip_block_time_rpc,
        )
        .await?;
        report.launch_times_fetched = fetched;
        report.launch_times_block_time = block_times;
        report.launch_times_interpolated = interpolated;
    }

    if config.pumpswap {
        let (redecoded, fetched) = repair_pumpswap_quarantines(
            stores,
            &http,
            &source.rpc_url,
            &evidence,
            config.max_unknown_pools,
        )
        .await?;
        report.pumpswap_redecoded = redecoded;
        report.pumpswap_fetched = fetched;
    }

    if config.import_corpus {
        if let Some(dir) = config
            .corpus_dir
            .clone()
            .or_else(|| default_corpus_dir())
        {
            report.corpus_events_imported = import_corpus(stores, &evidence, Path::new(&dir)).await?;
        }
    }

    if config.mint_backfill {
        let (mints, txs) = backfill_recent_mints(
            stores,
            &http,
            &source.rpc_url,
            &evidence,
            source.helius_api_key.is_some(),
            config.history_days,
            config.max_mints,
        )
        .await?;
        report.mint_backfills = mints;
        report.mint_transactions = txs;
    }

    if config.snapshots {
        report.snapshots_written =
            write_decision_age_snapshots(stores, config.max_snapshots).await?;
    }

    report.coverage = coverage(stores).await.unwrap_or_else(|error| {
        json!({"error": error.to_string()})
    });
    info!(?report, "evidence repair complete");
    Ok(report)
}

pub async fn load_identity_cache(stores: &Stores) -> anyhow::Result<usize> {
    let rows = stores.load_pool_identities().await?;
    let mapped = rows
        .into_iter()
        .filter_map(|row| {
            Some((
                row.pool,
                CachedPoolIdentity {
                    pool_base_mint: if row.reversed {
                        pumphound_domain::Pubkey::from_base58(&row.quote_mint).ok()?
                    } else {
                        pumphound_domain::Pubkey::from_base58(&row.token_mint).ok()?
                    },
                    pool_quote_mint: if row.reversed {
                        pumphound_domain::Pubkey::from_base58(&row.token_mint).ok()?
                    } else {
                        pumphound_domain::Pubkey::from_base58(&row.quote_mint).ok()?
                    },
                    pool_quote_decimals: if row.reversed {
                        row.token_decimals as u8
                    } else {
                        row.quote_decimals as u8
                    },
                    token_mint: pumphound_domain::Pubkey::from_base58(&row.token_mint).ok()?,
                    quote_mint: pumphound_domain::Pubkey::from_base58(&row.quote_mint).ok()?,
                    token_decimals: row.token_decimals as u8,
                    quote_decimals: row.quote_decimals as u8,
                    reversed: row.reversed,
                },
            ))
        })
        .collect::<Vec<_>>();
    let count = mapped.len();
    replace_pool_identities(mapped);
    Ok(count)
}

async fn repair_launch_times(
    stores: &Stores,
    http: &reqwest::Client,
    rpc: &str,
    max_fetches: usize,
    skip_block_time_rpc: bool,
) -> anyhow::Result<(usize, usize, usize)> {
    let mut block_times = 0usize;
    if skip_block_time_rpc {
        info!("evidence repair: skipping getBlockTime RPC; interpolating leftovers");
    } else {
    let _ = max_fetches;
    let slot_rows = sqlx::query(
        r#"SELECT launch_id, creation_slot
           FROM pumphound.launches
           WHERE creation_slot > 0
             AND creation_slot < 2000000000
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
             AND (
               creation_time IS NULL
               OR attributes->>'creation_time_source' = 'slot_interpolation'
             )"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut slots = slot_rows
        .iter()
        .filter_map(|row| {
            let slot = row.get::<i64, _>("creation_slot");
            (slot > 0).then_some(slot as u64)
        })
        .collect::<Vec<_>>();
    slots.sort_unstable();
    slots.dedup();
    slots.reverse();
    let block_rpc = block_time_rpc_url(rpc);
    info!(
        slots = slots.len(),
        rpc = %redact_rpc(&block_rpc),
        "evidence repair: getBlockTime for launch slots newest-first"
    );
    let mut by_slot: std::collections::HashMap<u64, Vec<Uuid>> = std::collections::HashMap::new();
    for row in &slot_rows {
        let launch_id: Uuid = row.get("launch_id");
        let slot = row.get::<i64, _>("creation_slot") as u64;
        by_slot.entry(slot).or_default().push(launch_id);
    }
    for (index, chunk) in slots.chunks(25).enumerate() {
        let times = match fetch_block_times(http, &block_rpc, chunk).await {
            Ok(times) if !times.is_empty() => times,
            other => {
                if let Err(error) = &other {
                    warn!(%error, chunk = index, "public getBlockTime missed; trying Helius");
                }
                if block_rpc != rpc {
                    fetch_block_times(http, rpc, chunk).await.unwrap_or_default()
                } else {
                    Default::default()
                }
            }
        };
        for (slot, time) in times {
            if let Some(ids) = by_slot.get(&slot) {
                for launch_id in ids {
                    stores
                        .update_launch_creation_time(*launch_id, time, "rpc_block_time")
                        .await?;
                    block_times += 1;
                }
            }
        }
        if (index + 1) % 10 == 0 {
            info!(
                done_chunks = index + 1,
                written = block_times,
                "evidence repair: launch-time persist progress"
            );
        }
    }
    } // skip_block_time_rpc else

    let anchors = sqlx::query(
        r#"SELECT creation_slot, creation_time
           FROM pumphound.launches
           WHERE creation_time IS NOT NULL
             AND creation_time >= timestamptz '2024-01-01'
             AND creation_time < now() + interval '2 hours'
           ORDER BY creation_slot"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let anchors: Vec<(i64, DateTime<Utc>)> = anchors
        .into_iter()
        .filter_map(|row| {
            Some((
                row.get::<i64, _>("creation_slot"),
                row.get::<DateTime<Utc>, _>("creation_time"),
            ))
        })
        .collect();
    let still_missing = sqlx::query(
        r#"SELECT launch_id, creation_slot
           FROM pumphound.launches
           WHERE creation_time IS NULL
             AND creation_slot > 0
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut interpolated = 0usize;
    for row in still_missing {
        let launch_id: Uuid = row.get("launch_id");
        let slot: i64 = row.get("creation_slot");
        if let Some(time) = interpolate_from_anchors(slot, &anchors) {
            stores
                .update_launch_creation_time(launch_id, time, "slot_interpolation")
                .await?;
            interpolated += 1;
        }
    }
    let mut fetched = 0usize;
    if skip_block_time_rpc {
        return Ok((fetched, block_times, interpolated));
    }
    let remaining_creates = sqlx::query(
        r#"SELECT launch_id, creation_signature
           FROM pumphound.launches
           WHERE creation_time IS NULL
             AND COALESCE(attributes->>'observed_from','') = 'create'
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
           ORDER BY known_at DESC
           LIMIT $1"#,
    )
    .bind(max_fetches.min(200) as i64)
    .fetch_all(&stores.postgres)
    .await?;
    for row in remaining_creates {
        let launch_id: Uuid = row.get("launch_id");
        let signature: String = row.get("creation_signature");
        match fetch_transaction(http, rpc, &signature).await {
            Ok(Some(tx)) => {
                if let Some(time) = tx.block_time.or_else(|| decode_event_time(&tx)) {
                    let source = if tx.block_time.is_some() {
                        "rpc_block_time"
                    } else {
                        "create_event_timestamp"
                    };
                    stores
                        .update_launch_creation_time(launch_id, time, source)
                        .await?;
                    fetched += 1;
                }
            }
            Ok(None) => {}
            Err(error) => warn!(%error, signature, "launch-time getTransaction failed"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    }
    Ok((fetched, block_times, interpolated))
}

pub fn interpolate_from_anchors(
    slot: i64,
    anchors: &[(i64, DateTime<Utc>)],
) -> Option<DateTime<Utc>> {
    if anchors.len() < 2 {
        return None;
    }
    let mut left = None;
    let mut right = None;
    for (anchor_slot, time) in anchors {
        if *anchor_slot <= slot {
            left = Some((*anchor_slot, *time));
        }
        if *anchor_slot >= slot && right.is_none() {
            right = Some((*anchor_slot, *time));
            break;
        }
    }
    match (left, right) {
        (Some((ls, lt)), Some((rs, rt))) if (rs - ls).abs() <= 15_000 => {
            interpolate_slot_time(slot, ls, lt, rs, rt)
        }
        (Some((ls, lt)), None) if (slot - ls).abs() <= 250 => {
            interpolate_slot_time(slot, ls, lt, ls + 1, lt + Duration::milliseconds(400))
        }
        (None, Some((rs, rt))) if (rs - slot).abs() <= 250 => {
            interpolate_slot_time(slot, rs.saturating_sub(1), rt - Duration::milliseconds(400), rs, rt)
        }
        _ => None,
    }
}

fn decode_event_time(tx: &RawTransaction) -> Option<DateTime<Utc>> {
    let pump = PumpBondingCurveDecoder::default();
    let swap = PumpSwapDecoder::default();
    pump.decode(tx)
        .ok()
        .into_iter()
        .flatten()
        .chain(swap.decode(tx).ok().into_iter().flatten())
        .find_map(|event| event.block_time)
}

async fn repair_pumpswap_quarantines(
    stores: &Stores,
    http: &reqwest::Client,
    rpc: &str,
    evidence: &EvidenceWriter,
    max_unknown_pools: usize,
) -> anyhow::Result<(usize, usize)> {
    #[derive(Deserialize, clickhouse::Row)]
    struct QuarantineRow {
        signature: String,
        payload_json: String,
    }
    let rows = stores
        .clickhouse
        .query(
            "SELECT signature, payload_json \
             FROM canonical_event_revisions_buffer \
             WHERE kind='decode.quarantined' \
               AND position('pump_swap_identity_unresolved' IN payload_json) > 0 \
             ORDER BY slot DESC LIMIT 5000",
        )
        .fetch_all::<QuarantineRow>()
        .await
        .unwrap_or_default();
    let mut unknown_pools = HashSet::new();
    let mut fetched = 0usize;
    for row in &rows {
        let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or(json!({}));
        let reason = payload
            .pointer("/reason")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(pool) = unresolved_pool_from_reason(reason) {
            if pumphound_protocol_pumpswap::lookup_pool_identity(&pool).is_some() {
                continue;
            }
            if !unknown_pools.insert(pool.clone()) || unknown_pools.len() > max_unknown_pools {
                continue;
            }
            match fetch_transaction(http, rpc, &row.signature).await {
                Ok(Some(tx)) => {
                    let stats = persist_transaction(stores, evidence, &tx).await?;
                    fetched += stats.decoded;
                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                }
                _ => {}
            }
        }
    }
    Ok((0, fetched))
}

async fn backfill_recent_mints(
    stores: &Stores,
    http: &reqwest::Client,
    rpc: &str,
    evidence: &EvidenceWriter,
    helius: bool,
    history_days: i64,
    max_mints: usize,
) -> anyhow::Result<(usize, usize)> {
    let rows = sqlx::query(
        r#"SELECT mint, creation_slot, creation_time
           FROM pumphound.launches
           WHERE protocol='pump_curve'
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
             AND (
               creation_time >= now() - ($1::text || ' days')::interval
               OR (creation_time IS NULL AND known_at >= now() - ($1::text || ' days')::interval)
             )
           ORDER BY COALESCE(creation_time, known_at) DESC
           LIMIT $2"#,
    )
    .bind(history_days.to_string())
    .bind(max_mints as i64)
    .fetch_all(&stores.postgres)
    .await?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<RawChainEnvelope>(256);
    let persist_stores = stores.clone();
    let persist_evidence = evidence.clone();
    let persist_task = tokio::spawn(async move {
        let mut transactions = 0usize;
        while let Some(envelope) = rx.recv().await {
            if let Ok(tx) = serde_json::from_value::<RawTransaction>(envelope.payload.clone()) {
                if let Ok(stats) = persist_transaction(&persist_stores, &persist_evidence, &tx).await
                {
                    transactions += stats.decoded + stats.quarantined;
                }
            }
        }
        transactions
    });
    let mut mints = 0usize;
    for (index, row) in rows.iter().enumerate() {
        let mint: String = row.get("mint");
        let slot = row.try_get::<i64, _>("creation_slot").ok().map(|s| s as u64);
        let created = row
            .try_get::<Option<DateTime<Utc>>, _>("creation_time")
            .ok()
            .flatten();
        let recovered = backfill_mint_early_life(
            http,
            rpc,
            &mint,
            slot,
            created,
            helius,
            MintBackfillCaps::default(),
            &tx,
            1,
        )
        .await
        .unwrap_or(0);
        if recovered > 0 {
            mints += 1;
        }
        if (index + 1) % 25 == 0 {
            info!(
                done = index + 1,
                recovered_mints = mints,
                "evidence repair: mint backfill progress"
            );
        }
    }
    drop(tx);
    let transactions = persist_task.await.unwrap_or(0);
    Ok((mints, transactions))
}

async fn import_corpus(
    stores: &Stores,
    evidence: &EvidenceWriter,
    root: &Path,
) -> anyhow::Result<usize> {
    let canonical = root.join("canonical").join("by-event-id");
    if !canonical.is_dir() {
        return Ok(0);
    }
    let mut imported = 0usize;
    let pump = PumpBondingCurveDecoder::default();
    let swap = PumpSwapDecoder::default();
    let raw_root = root.join("raw");
    if raw_root.is_dir() {
        for date_dir in std::fs::read_dir(&raw_root).into_iter().flatten().flatten() {
            if !date_dir.path().is_dir() {
                continue;
            }
            for file in std::fs::read_dir(date_dir.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                if imported >= 120_000 {
                    return Ok(imported);
                }
                let Ok(bytes) = std::fs::read(file.path()) else {
                    continue;
                };
                let Ok(archive) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                let Some(tx) = archive
                    .get("transaction")
                    .cloned()
                    .and_then(|value| serde_json::from_value::<RawTransaction>(value).ok())
                else {
                    continue;
                };
                let _ = pump;
                let _ = swap;
                if let Ok(stats) = persist_transaction(stores, evidence, &tx).await {
                    imported += stats.decoded;
                    if imported > 0 && imported % 2_000 == 0 {
                        info!(imported, "evidence repair: corpus import progress");
                    }
                }
            }
        }
    }
    Ok(imported)
}

async fn write_decision_age_snapshots(
    stores: &Stores,
    max_launches: usize,
) -> anyhow::Result<usize> {
    let launches = sqlx::query(
        r#"SELECT launch_id, mint, creation_time, creation_slot
           FROM pumphound.launches
           WHERE creation_time IS NOT NULL
             AND creation_time >= now() - interval '90 days'
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
           ORDER BY creation_time DESC
           LIMIT $1"#,
    )
    .bind(max_launches as i64)
    .fetch_all(&stores.postgres)
    .await?;
    let mut written = 0usize;
    for row in launches {
        let launch_id: Uuid = row.get("launch_id");
        let mint: String = row.get("mint");
        let created: DateTime<Utc> = row.get("creation_time");
        let creation_slot: i64 = row.get("creation_slot");
        let ticks = load_trade_ticks(stores, &mint).await.unwrap_or_default();
        if ticks.is_empty() {
            continue;
        }
        for age in DECISION_AGES_SECONDS {
            let cutoff = created + Duration::seconds(age as i64);
            let mut aggregator = WindowAggregator::default();
            let mut last_slot = creation_slot as u64;
            for tick in &ticks {
                if tick.at > cutoff.timestamp() {
                    break;
                }
                last_slot = last_slot.max(tick.slot);
                aggregator.push(tick.inner.clone());
            }
            let as_of = ObservationPoint {
                slot: Slot(last_slot),
                tx_index: None,
                event_index: None,
                event_time: Some(cutoff),
                observed_at: cutoff,
                knowledge_cutoff: cutoff,
                commitment: Commitment::Confirmed,
            };
            let mut vector = aggregator.compute(
                &launch_id.to_string(),
                age.max(1),
                as_of,
                Some(age as f64),
                None,
                0,
                None,
            );
            vector.feature_snapshot_id = sha256_hex(
                format!("{DECISION_AGE_SCHEMA}:{launch_id}:{age}").as_bytes(),
            );
            vector.schema_version = DECISION_AGE_SCHEMA.into();
            vector
                .values
                .insert("decision_age_seconds".into(), json!(age));
            vector.quality = DataQuality::unknown("reconstructed_from_canonical_trades");
            if persist_decision_age_snapshot(stores, launch_id, &mint, &vector)
                .await
                .is_ok()
            {
                written += 1;
            }
        }
    }
    Ok(written)
}

struct DatedTick {
    at: i64,
    slot: u64,
    inner: TradeTick,
}

async fn load_trade_ticks(stores: &Stores, mint: &str) -> anyhow::Result<Vec<DatedTick>> {
    #[derive(Deserialize, clickhouse::Row)]
    struct Row {
        slot: u64,
        #[serde(with = "clickhouse::serde::chrono::datetime64::micros::option")]
        block_time: Option<DateTime<Utc>>,
        #[serde(with = "clickhouse::serde::chrono::datetime64::micros")]
        observed_at: DateTime<Utc>,
        trader: String,
        base_amount_atomic: u128,
        quote_amount_atomic: u128,
        payload_json: String,
    }
    let rows = stores
        .clickhouse
        .query(
            "SELECT slot, block_time, observed_at, trader, base_amount_atomic, quote_amount_atomic, payload_json \
             FROM canonical_event_revisions_buffer \
             WHERE mint=? AND kind='trade.executed' \
             ORDER BY slot ASC, event_index ASC \
             LIMIT 5000",
        )
        .bind(mint)
        .fetch_all::<Row>()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut ticks = Vec::new();
    for row in rows {
        let payload: Value = serde_json::from_str(&row.payload_json).unwrap_or(json!({}));
        let side = payload
            .get("side")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let at = row
            .block_time
            .unwrap_or(row.observed_at)
            .timestamp();
        ticks.push(DatedTick {
            at,
            slot: row.slot,
            inner: TradeTick {
                at,
                side_buy: side == "buy",
                base_atomic: row.base_amount_atomic,
                quote_atomic: row.quote_amount_atomic,
                trader: row.trader,
                price_ratio: if row.base_amount_atomic > 0 {
                    Some(row.quote_amount_atomic as f64 / row.base_amount_atomic as f64)
                } else {
                    None
                },
            },
        });
    }
    Ok(ticks)
}

async fn persist_decision_age_snapshot(
    stores: &Stores,
    launch_id: Uuid,
    mint: &str,
    vector: &FeatureVector,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.feature_snapshots
           (feature_snapshot_id,launch_id,mint,observation_window_seconds,cutoff_slot,
            knowledge_cutoff,computed_at,schema_version,values,dependency_ids,quality)
           VALUES ($1,$2,$3,$4,$5,$6,now(),$7,$8,$9,$10)
           ON CONFLICT (feature_snapshot_id) DO NOTHING"#,
    )
    .bind(&vector.feature_snapshot_id)
    .bind(launch_id)
    .bind(mint)
    .bind(vector.observation_window_seconds as i32)
    .bind(vector.as_of.slot.0 as i64)
    .bind(vector.as_of.knowledge_cutoff)
    .bind(&vector.schema_version)
    .bind(Value::Object(vector.values.clone()))
    .bind(json!(vector.dependency_ids))
    .bind(serde_json::to_value(&vector.quality)?)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

#[derive(Default)]
struct PersistStats {
    decoded: usize,
    quarantined: usize,
}

async fn persist_transaction(
    stores: &Stores,
    evidence: &EvidenceWriter,
    transaction: &RawTransaction,
) -> anyhow::Result<PersistStats> {
    let pump = PumpBondingCurveDecoder::default();
    let pumpswap = PumpSwapDecoder::default();
    let mut events = Vec::new();
    if let Ok(decoded) = pump.decode(transaction) {
        events.extend(decoded);
    }
    if let Ok(decoded) = pumpswap.decode(transaction) {
        events.extend(decoded);
    }
    for event in &mut events {
        event.revision = event.revision.max(2);
        remember_from_event(event);
    }
    let envelope = envelope_from_tx(transaction, 1, None);
    let batch_id = Uuid::now_v7();
    let durable = DurableBatch {
        batch_id,
        content_sha256: payload_sha256(envelope.payload_sha256.as_bytes()),
        rows: vec![envelope.clone()],
        cursor: pumphound_domain::ChainCursor {
            source: "evidence-repair".into(),
            subscription_id: "repair".into(),
            slot: Some(envelope.slot),
            signature: Some(transaction.signature.clone()),
            provider_cursor: None,
        },
        journal_uri: format!(
            "{}#repair={batch_id}",
            pumphound_storage::journal_stream(envelope.observed_at)
        ),
    };
    if let Ok(line) = serde_json::to_vec(&durable) {
        evidence
            .append(pumphound_storage::journal_stream(envelope.observed_at), line)
            .await;
    }
    let _ = stores.persist_raw_batch(&durable).await;
    let mut stats = PersistStats::default();
    let mut fresh = Vec::new();
    for ev in events {
        match stores.claim_event_key(&ev).await {
            Ok(true) => {
                if matches!(ev.payload, ChainEventPayload::DecodeQuarantined(_)) {
                    stats.quarantined += 1;
                } else {
                    stats.decoded += 1;
                }
                fresh.push(ev);
            }
            Ok(false) => {
                if ev.revision > 1 {
                    fresh.push(ev);
                }
            }
            Err(_) => {}
        }
    }
    for ev in &fresh {
        let _ = stores.upsert_launch_from_event(ev).await;
        let _ = stores.upsert_graph_from_event(ev).await;
    }
    let _ = stores.persist_canonical(&fresh).await;
    Ok(stats)
}

async fn coverage(stores: &Stores) -> anyhow::Result<Value> {
    let row = sqlx::query(
        r#"SELECT
             count(*)::bigint AS launches_total,
             count(*) FILTER (WHERE creation_time IS NOT NULL)::bigint AS with_chain_time,
             count(*) FILTER (WHERE canonicality='canonical')::bigint AS canonical,
             count(*) FILTER (WHERE quote_mint='11111111111111111111111111111111')::bigint AS bad_quote
           FROM pumphound.launches"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let identities = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pumphound.pool_identities")
        .fetch_optional(&stores.postgres)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);
    let snapshots = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM pumphound.feature_snapshots WHERE schema_version=$1",
    )
    .bind(DECISION_AGE_SCHEMA)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0);
    let ages = sqlx::query(
        r#"SELECT COALESCE(values->>'decision_age_seconds','unknown') AS age, count(*)::bigint AS n
           FROM pumphound.feature_snapshots
           WHERE schema_version=$1
           GROUP BY 1
           ORDER BY 1"#,
    )
    .bind(DECISION_AGE_SCHEMA)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    let age_map = ages
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("age"),
                json!(row.get::<i64, _>("n")),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let sources = sqlx::query(
        r#"SELECT COALESCE(attributes->>'creation_time_source','unknown') AS source, count(*)::bigint AS n
           FROM pumphound.launches
           WHERE creation_time IS NOT NULL
           GROUP BY 1"#,
    )
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    let source_map = sources
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("source"),
                json!(row.get::<i64, _>("n")),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let ingest = ingest_calibration(stores).await.unwrap_or(json!({}));
    Ok(json!({
        "launches_total": row.get::<i64,_>("launches_total"),
        "launches_with_chain_time": row.get::<i64,_>("with_chain_time"),
        "launches_canonical": row.get::<i64,_>("canonical"),
        "invalid_quote_mint": row.get::<i64,_>("bad_quote"),
        "pool_identities": identities,
        "decision_age_snapshots": snapshots,
        "decision_ages": age_map,
        "creation_time_sources": source_map,
        "ingest": ingest,
        "disk": disk_runway(),
    }))
}

async fn ingest_calibration(stores: &Stores) -> anyhow::Result<Value> {
    #[derive(Deserialize, clickhouse::Row)]
    struct Row {
        events_24h: u64,
        quarantined_24h: u64,
        pumpswap_unresolved_24h: u64,
        median_lag_ms: f64,
        p95_lag_ms: f64,
        future_timestamps: u64,
        impossible_slots: u64,
    }
    let row = stores
        .clickhouse
        .query(
            "SELECT \
               count() AS events_24h, \
               countIf(kind='decode.quarantined') AS quarantined_24h, \
               countIf(kind='decode.quarantined' AND position('pump_swap_identity_unresolved' IN payload_json) > 0) AS pumpswap_unresolved_24h, \
               quantileExact(0.5)(if(isNotNull(block_time) AND observed_at >= block_time, dateDiff('ms', block_time, observed_at), NULL)) AS median_lag_ms, \
               quantileExact(0.95)(if(isNotNull(block_time) AND observed_at >= block_time, dateDiff('ms', block_time, observed_at), NULL)) AS p95_lag_ms, \
               countIf(isNotNull(block_time) AND block_time > now() + INTERVAL 2 HOUR) AS future_timestamps, \
               countIf(slot > 2000000000) AS impossible_slots \
             FROM canonical_event_revisions_buffer \
             WHERE observed_at >= now() - INTERVAL 1 DAY",
        )
        .fetch_one::<Row>()
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(json!({
        "events_24h": row.events_24h,
        "quarantined_24h": row.quarantined_24h,
        "pumpswap_unresolved_24h": row.pumpswap_unresolved_24h,
        "quarantine_rate_24h": if row.events_24h == 0 {
            0.0
        } else {
            row.quarantined_24h as f64 / row.events_24h as f64
        },
        "median_observe_lag_ms": row.median_lag_ms,
        "p95_observe_lag_ms": row.p95_lag_ms,
        "future_timestamps_24h": row.future_timestamps,
        "impossible_slots_24h": row.impossible_slots,
    }))
}

fn block_time_rpc_url(rpc: &str) -> String {
    if let Ok(explicit) = std::env::var("PUMPHOUND_BLOCK_TIME_RPC_URL") {
        if !explicit.is_empty() {
            return explicit;
        }
    }
    if rpc.contains("helius") {
        "https://solana-rpc.publicnode.com".into()
    } else {
        rpc.to_string()
    }
}

fn redact_rpc(rpc: &str) -> String {
    match rpc.split_once('?') {
        Some((base, _)) => format!("{base}?[REDACTED]"),
        None => rpc.to_string(),
    }
}

fn disk_runway() -> Value {
    let paths = [
        std::env::var("PUMPHOUND_DATA_DIR").unwrap_or_else(|_| "./data".into()),
        "/Users/shubhamkumar/PumpHoundFastData".into(),
        "/Volumes/PumpHoundData".into(),
    ];
    let mut out = serde_json::Map::new();
    for path in paths {
        if let Ok(output) = std::process::Command::new("df")
            .args(["-g", &path])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = text.lines().nth(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() >= 4 {
                    out.insert(
                        path,
                        json!({
                            "size_gib": cols[1],
                            "used_gib": cols[2],
                            "avail_gib": cols[3],
                        }),
                    );
                }
            }
        }
    }
    Value::Object(out)
}

fn default_corpus_dir() -> Option<String> {
    for candidate in [
        "data/history-helius-full-25k",
        "data/history-recent-90d",
        "data/history-smoke-v2",
    ] {
        if Path::new(candidate).join("canonical").exists()
            || Path::new(candidate).join("raw").exists()
        {
            return Some(candidate.into());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_only_when_anchors_are_nearby() {
        let left = DateTime::from_timestamp(1_720_000_000, 0).unwrap();
        let right = DateTime::from_timestamp(1_720_000_400, 0).unwrap();
        let mid = interpolate_from_anchors(150, &[(100, left), (200, right)]).unwrap();
        assert_eq!(mid.timestamp(), 1_720_000_200);
        assert!(interpolate_from_anchors(50_000, &[(100, left), (200, right)]).is_none());
    }
}
