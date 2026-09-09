mod config;

use anyhow::Context;
use pumphound_chain_source::{
    envelope_from_tx, fetch_transaction, is_hot_mint, open_source, SourceConfig,
};
use pumphound_domain::{
    ids::payload_sha256, ChainEvent, DurableBatch, LaunchpadDecoder, RawKind, RawTransaction,
};
use pumphound_protocol_pump::PumpBondingCurveDecoder;
use pumphound_protocol_pumpswap::PumpSwapDecoder;
use pumphound_storage::Stores;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,pumphound_collector=info".into()),
        )
        .init();

    let cfg = config::CollectorConfig::load();
    let source_cfg = SourceConfig::from_env();
    let rpc_url = source_cfg.rpc_url.clone();
    let upgrade_http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("hot-set transaction client")?;
    println!("{}", source_cfg.label());
    info!(
        mode = %cfg.mode,
        source = %source_cfg.label(),
        pump = %pumphound_domain::PUMP_PROGRAM_ID,
        pumpswap = %pumphound_domain::PUMPSWAP_PROGRAM_ID,
        "collector starting"
    );

    assert_eq!(
        pumphound_protocol_pump::PROGRAM_ID,
        std::env::var("PUMPHOUND_PUMP_PROGRAM")
            .unwrap_or_else(|_| pumphound_domain::PUMP_PROGRAM_ID.into())
            .as_str(),
        "vendored pump IDL address must match configured program id"
    );
    assert_eq!(
        pumphound_protocol_pumpswap::PROGRAM_ID,
        std::env::var("PUMPHOUND_PUMPSWAP_PROGRAM")
            .unwrap_or_else(|_| pumphound_domain::PUMPSWAP_PROGRAM_ID.into())
            .as_str(),
        "vendored pumpswap IDL address must match configured program id"
    );

    let stores = Stores::connect().await.context("stores")?;
    let cursor = stores
        .load_checkpoint("solana-ws", "pump-programs")
        .await
        .ok()
        .flatten();

    let (tx, mut rx) = mpsc::channel(1024);
    {
        let stores = stores.clone();
        tokio::spawn(async move {
            loop {
                match stores.load_hot_mints().await {
                    Ok(mints) => pumphound_chain_source::replace_hot_mints(mints),
                    Err(error) => {
                        tracing::debug!(%error, "hot set table unavailable; broad scout only")
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });
    }
    let mut source = open_source(&source_cfg).await;
    tokio::spawn(async move {
        if let Err(e) = source.run(cursor, tx).await {
            error!(error = %e, "chain source stopped");
        }
    });

    let pump = PumpBondingCurveDecoder::default();
    let pumpswap = PumpSwapDecoder::default();
    let replay_canonical = load_replay_canonical(&cfg.mode);
    if !replay_canonical.is_empty() {
        info!(
            signatures = replay_canonical.len(),
            "REPLAY canonical sidecar loaded (synthetic; not live mainnet)"
        );
    }
    let data_dir = cfg.data_dir.clone();
    let evidence = pumphound_storage::EvidenceWriter::spawn(data_dir.clone());
    let mut seen_tx = 0u64;
    let mut decoded = 0u64;
    let mut quarantined = 0u64;
    let mut duplicate_skips = 0u64;
    let mut hot_upgrade_attempts = 0u64;
    let mut hot_upgrade_successes = 0u64;
    {
        let stores = stores.clone();
        let refresh_seconds = std::env::var("PUMPHOUND_INTELLIGENCE_REFRESH_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300)
            .max(60);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            loop {
                if let Err(e) = stores.refresh_intelligence().await {
                    warn!(error = %e, "intelligence refresh");
                }
                tokio::time::sleep(std::time::Duration::from_secs(refresh_seconds)).await;
            }
        });
    }

    while let Some(raw) = rx.recv().await {
        if matches!(raw.kind, RawKind::Slot) {
            let _ = stores
                .record_checkpoint(&pumphound_domain::ChainCursor {
                    source: raw.source.clone(),
                    subscription_id: "pump-programs".into(),
                    slot: Some(raw.slot),
                    signature: None,
                    provider_cursor: Some(raw.source_cursor.clone()),
                })
                .await;
            continue;
        }
        if !matches!(raw.kind, RawKind::Transaction) {
            continue;
        }

        let batch_id = Uuid::now_v7();
        let journal_uri = format!(
            "{}#batch={batch_id}",
            pumphound_storage::journal_stream(raw.observed_at)
        );
        let durable = DurableBatch {
            batch_id,
            content_sha256: payload_sha256(raw.payload_sha256.as_bytes()),
            rows: vec![raw.clone()],
            cursor: pumphound_domain::ChainCursor {
                source: raw.source.clone(),
                subscription_id: "pump-programs".into(),
                slot: Some(raw.slot),
                signature: raw
                    .payload
                    .get("signature")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                provider_cursor: Some(raw.source_cursor.clone()),
            },
            journal_uri: journal_uri.clone(),
        };
        // Write-behind: the journal/archive volume is slow on file creation; the ingest path
        // must not pay for it per sighting (see storage::EvidenceWriter).
        match serde_json::to_vec(&durable) {
            Ok(line) => {
                evidence
                    .append(pumphound_storage::journal_stream(raw.observed_at), line)
                    .await
            }
            Err(e) => warn!(error = %e, "journal encode failed"),
        }
        let parsed: Result<RawTransaction, _> = serde_json::from_value(raw.payload.clone());
        let Ok(mut transaction) = parsed else {
            // Still make the sighting durable so the batch/checkpoint ledger stays complete.
            if let Err(e) = stores.persist_raw_batch(&durable).await {
                warn!(error = %e, "raw persist failed");
            }
            let _ = stores.record_checkpoint(&durable.cursor).await;
            warn!("envelope payload is not a RawTransaction");
            continue;
        };
        seen_tx += 1;
        {
            let stream = pumphound_storage::Stores::raw_archive_stream(&raw);
            let body = pumphound_storage::Stores::raw_archive_body(&raw, &transaction);
            if let Ok(line) = serde_json::to_vec(&body) {
                evidence.append(stream.clone(), line).await;
            }
            let uri = format!("{stream}#sig={}", transaction.signature);
            // The batch row, the source checkpoint and the archive index are independent
            // writes (Postgres ×2, ClickHouse ×1). Issued sequentially they cost ~3 round
            // trips per sighting; the ingest loop was store-latency bound at ~30 tx/s.
            let (persisted, checkpointed, indexed) = tokio::join!(
                stores.persist_raw_batch(&durable),
                stores.record_checkpoint(&durable.cursor),
                stores.index_raw_archive(&raw, &transaction, &uri),
            );
            if let Err(e) = persisted {
                warn!(error = %e, "raw persist failed; continuing decode");
            }
            let _ = checkpointed;
            if let Err(e) = indexed {
                warn!(error = %e, "raw archive index");
            }
        }
        let mut events = decode_events(&transaction, &pump, &pumpswap, &replay_canonical);
        let logs_only =
            transaction.instructions.is_empty() && transaction.inner_instructions.is_empty();
        if logs_only && events.iter().any(event_mentions_hot_mint) {
            hot_upgrade_attempts += 1;
            let mut upgraded = None;
            for attempt in 0..3 {
                match fetch_transaction(&upgrade_http, &rpc_url, &transaction.signature).await {
                    Ok(Some(full)) if !full.instructions.is_empty() => {
                        upgraded = Some(full);
                        break;
                    }
                    Ok(_) if attempt < 2 => {
                        tokio::time::sleep(std::time::Duration::from_millis(150 * (attempt + 1)))
                            .await;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!(
                            signature = transaction.signature,
                            %error,
                            "hot-set transaction upgrade failed"
                        );
                        break;
                    }
                }
            }
            if let Some(full) = upgraded {
                let upgraded_raw =
                    envelope_from_tx(&full, raw.transport.reconnect_generation, None);
                let upgraded_batch_id = Uuid::now_v7();
                let upgraded_journal = format!(
                    "{}#batch={upgraded_batch_id}",
                    pumphound_storage::journal_stream(upgraded_raw.observed_at)
                );
                let upgraded_batch = DurableBatch {
                    batch_id: upgraded_batch_id,
                    content_sha256: payload_sha256(upgraded_raw.payload_sha256.as_bytes()),
                    rows: vec![upgraded_raw.clone()],
                    cursor: pumphound_domain::ChainCursor {
                        source: upgraded_raw.source.clone(),
                        subscription_id: "pump-programs-hot-upgrade".into(),
                        slot: Some(upgraded_raw.slot),
                        signature: Some(full.signature.clone()),
                        provider_cursor: Some(upgraded_raw.source_cursor.clone()),
                    },
                    journal_uri: upgraded_journal.clone(),
                };
                if let Ok(line) = serde_json::to_vec(&upgraded_batch) {
                    evidence
                        .append(
                            pumphound_storage::journal_stream(upgraded_raw.observed_at),
                            line,
                        )
                        .await;
                }
                let durable_upgrade = stores
                    .persist_raw_batch(&upgraded_batch)
                    .await
                    .map_err(anyhow::Error::from);
                match durable_upgrade {
                    Ok(()) => {
                        let stream = pumphound_storage::Stores::raw_archive_stream(&upgraded_raw);
                        if let Ok(line) = serde_json::to_vec(
                            &pumphound_storage::Stores::raw_archive_body(&upgraded_raw, &full),
                        ) {
                            evidence.append(stream.clone(), line).await;
                        }
                        let uri = format!("{stream}#sig={}", full.signature);
                        if let Err(error) =
                            stores.index_raw_archive(&upgraded_raw, &full, &uri).await
                        {
                            warn!(%error, "hot-set upgraded archive index");
                        }
                        transaction = full;
                        events = decode_events(&transaction, &pump, &pumpswap, &replay_canonical);
                        // The logs-only decode is revision 1 evidence. A
                        // transaction-account-backed decode is the correcting
                        // revision even when only the corrected rows are
                        // published by this collector process.
                        for event in &mut events {
                            event.revision = event.revision.max(2);
                        }
                        hot_upgrade_successes += 1;
                    }
                    Err(error) => {
                        warn!(
                            signature = transaction.signature,
                            %error,
                            "hot-set upgrade was not used because durable persistence failed"
                        );
                    }
                }
            }
        }
        // Idempotency claims are issued concurrently across the (typically 1–3) events of one
        // transaction; each claim is an independent unique-key insert.
        let claims = futures::future::join_all(
            events.iter().map(|ev| stores.claim_event_key(ev)),
        )
        .await;
        let mut fresh = Vec::new();
        for (ev, claim) in events.into_iter().zip(claims) {
            match claim {
                Ok(false) => {
                    duplicate_skips += 1;
                    continue;
                }
                Err(e) => warn!(error = %e, "event key claim"),
                Ok(true) => {}
            }
            if matches!(
                ev.payload,
                pumphound_domain::ChainEventPayload::DecodeQuarantined(_)
            ) {
                quarantined += 1;
            } else {
                decoded += 1;
            }
            fresh.push(ev);
        }
        let events = fresh;
        // Launch and graph projections are independent of each other and of the ClickHouse
        // canonical revision store; only the claim above must precede them. NATS fan-out stays
        // after the launch upsert so consumers (paper loop decisions reference `launches`)
        // never see an event before its launch row exists.
        // Launch upserts stay in event order within a transaction (a `create` must claim the
        // launch_id before the same transaction's first trade); graph upserts are independent.
        let (launches, graphs, canonical) = tokio::join!(
            async {
                let mut out = Vec::with_capacity(events.len());
                for ev in &events {
                    out.push(stores.upsert_launch_from_event(ev).await);
                }
                out
            },
            futures::future::join_all(events.iter().map(|ev| stores.upsert_graph_from_event(ev))),
            stores.persist_canonical(&events),
        );
        for e in launches.into_iter().filter_map(Result::err) {
            warn!(error = %e, "launch upsert");
        }
        for e in graphs.into_iter().filter_map(Result::err) {
            warn!(error = %e, "graph upsert");
        }
        if let Err(e) = canonical {
            warn!(error = %e, "canonical persist failed; still publishing");
        }
        if !events.is_empty() {
            if let Err(e) = stores.publish("chain.normalized.v1", &events).await {
                warn!(error = %e, "nats publish");
            }
            // Canonical event journal (read by ml/pumphound_ml/extraction/sources.py). Routed
            // through the single-task EvidenceWriter: one write per line, no concurrent
            // appenders, so lines can never interleave, and the ingest loop never blocks on
            // the volume.
            for ev in &events {
                if let Ok(line) = serde_json::to_vec(ev) {
                    evidence.append("journal/events.jsonl".to_string(), line).await;
                }
            }
        }

        if let Some(sig) = durable.cursor.signature.as_deref() {
            let lag = raw.transport.source_lag_ms.unwrap_or(-1);
            println!(
                "slot={} sig={} lag_ms={} raw={} decoded={} quarantined={} dups={} hot_upgrades={}/{}",
                raw.slot,
                sig,
                lag,
                seen_tx,
                decoded,
                quarantined,
                duplicate_skips,
                hot_upgrade_successes,
                hot_upgrade_attempts
            );
        }
    }
    Ok(())
}

fn decode_events(
    transaction: &RawTransaction,
    pump: &PumpBondingCurveDecoder,
    pumpswap: &PumpSwapDecoder,
    replay_canonical: &HashMap<String, Vec<ChainEvent>>,
) -> Vec<ChainEvent> {
    let mut events = Vec::new();
    match pump.decode(transaction) {
        Ok(mut decoded) => events.append(&mut decoded),
        Err(error) => warn!(%error, "pump decode"),
    }
    match pumpswap.decode(transaction) {
        Ok(mut decoded) => events.append(&mut decoded),
        Err(error) => warn!(%error, "pumpswap decode"),
    }
    if events.is_empty() {
        if let Some(extra) = replay_canonical.get(&transaction.signature) {
            events.extend(extra.clone());
        }
    }
    events
}

fn event_mentions_hot_mint(event: &ChainEvent) -> bool {
    let mint = match &event.payload {
        pumphound_domain::ChainEventPayload::LaunchCreated(value) => Some(value.mint),
        pumphound_domain::ChainEventPayload::TradeExecuted(value) => Some(value.mint),
        pumphound_domain::ChainEventPayload::LaunchMigrated(value) => Some(value.mint),
        pumphound_domain::ChainEventPayload::LaunchCompleted(value) => Some(value.mint),
        pumphound_domain::ChainEventPayload::LiquidityChanged(value) => Some(value.mint),
        pumphound_domain::ChainEventPayload::DecodeQuarantined(value) => value
            .reason
            .split("pool=")
            .nth(1)
            .and_then(|suffix| suffix.split_whitespace().next())
            .and_then(|pool| pumphound_domain::Pubkey::from_base58(pool).ok()),
        _ => None,
    };
    mint.is_some_and(|mint| is_hot_mint(&mint.to_string()))
}

/// Labelled REPLAY only. Fixture logs have no Anchor `Program data`, so the
/// decoder correctly yields nothing; the sidecar supplies the matching
/// synthetic canonical events from `decoded_events.json`.
fn load_replay_canonical(mode: &str) -> HashMap<String, Vec<ChainEvent>> {
    if !mode.eq_ignore_ascii_case("REPLAY") {
        return HashMap::new();
    }
    let path = std::env::var("PUMPHOUND_REPLAY_CANONICAL")
        .unwrap_or_else(|_| "fixtures/chain/decoded_events.json".into());
    let Ok(text) = std::fs::read_to_string(&path) else {
        warn!(path, "REPLAY canonical sidecar missing");
        return HashMap::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        warn!(path, "REPLAY canonical sidecar is not JSON");
        return HashMap::new();
    };
    let Some(arr) = v.get("events").and_then(|e| e.as_array()) else {
        return HashMap::new();
    };
    let mut out: HashMap<String, Vec<ChainEvent>> = HashMap::new();
    for ev in arr {
        match serde_json::from_value::<ChainEvent>(ev.clone()) {
            Ok(parsed) => out
                .entry(parsed.signature.clone())
                .or_default()
                .push(parsed),
            Err(e) => warn!(error = %e, "skip malformed replay canonical event"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_sidecar_deserializes() {
        let text = include_str!("../../../fixtures/chain/decoded_events.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let arr = v["events"].as_array().unwrap();
        assert!(!arr.is_empty());
        for ev in arr {
            serde_json::from_value::<ChainEvent>(ev.clone()).expect("canonical fixture");
        }
    }
}
