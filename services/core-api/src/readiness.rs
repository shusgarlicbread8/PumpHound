use crate::state::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const STALE_MARKET_MS: i64 = 20_000;

/// The quarantine queries below scan the whole `canonical_event_revisions` table (hundreds of
/// thousands of rows). They are polled by the desktop, `/health`, the cockpit and every paper
/// scan; without a cache they were ~85 full scans/minute and kept ClickHouse at >250% CPU on
/// the shared Docker VM, starving Postgres. Evidence is 15-minute windows, so a 10 s cache
/// changes nothing material.
const QUARANTINE_CACHE_TTL: Duration = Duration::from_secs(10);

struct Cached<T> {
    at: Instant,
    value: T,
}

fn cached<T: Clone>(slot: &Mutex<Option<Cached<T>>>) -> Option<T> {
    let guard = slot.lock().ok()?;
    let entry = guard.as_ref()?;
    (entry.at.elapsed() < QUARANTINE_CACHE_TTL).then(|| entry.value.clone())
}

fn store<T>(slot: &Mutex<Option<Cached<T>>>, value: &T)
where
    T: Clone,
{
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(Cached {
            at: Instant::now(),
            value: value.clone(),
        });
    }
}

fn impact_cache() -> &'static Mutex<Option<Cached<QuarantineImpact>>> {
    static CACHE: OnceLock<Mutex<Option<Cached<QuarantineImpact>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn health_cache() -> &'static Mutex<Option<Cached<Value>>> {
    static CACHE: OnceLock<Mutex<Option<Cached<Value>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub fn idle_ms(
    last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<i64> {
    last_event_at.map(|at| (now - at).num_milliseconds().max(0))
}

pub fn display_chain(
    process_chain: &str,
    last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let idle = idle_ms(last_event_at, now).unwrap_or(i64::MAX);
    if process_chain == "healthy" && idle > STALE_MARKET_MS {
        "delayed".into()
    } else {
        process_chain.to_string()
    }
}

pub fn headline_from_ready(ready: &Readiness) -> String {
    let process_ok = ready.database == "READY";
    if process_ok && ready.autonomous_paper == "BLOCKED" {
        "SYSTEM RUNNING · TRADING BLOCKED".into()
    } else if process_ok && ready.market_data == "STALE" {
        "SYSTEM RUNNING · DATA STALE".into()
    } else if process_ok && ready.allow_new_entries {
        "SYSTEM RUNNING · TRADING READY".into()
    } else if ready.database == "DOWN" {
        "PROCESS DEGRADED".into()
    } else {
        format!(
            "SYSTEM {} · TRADING {}",
            ready.market_data, ready.autonomous_paper
        )
    }
}

#[derive(Clone, Debug)]
pub struct Readiness {
    pub market_data: &'static str,
    pub execution: &'static str,
    pub models: &'static str,
    pub database: &'static str,
    pub ai: &'static str,
    pub autonomous_paper: &'static str,
    pub allow_new_entries: bool,
    pub reasons: Vec<String>,
    pub last_market_snapshot_at: Option<chrono::DateTime<chrono::Utc>>,
    pub market_snapshot_count: usize,
    pub last_market_snapshot_persisted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub market_snapshot_persist_error: Option<String>,
}

pub fn market_freshness(
    last_slot: u64,
    last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    chain: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> (&'static str, bool) {
    let lag_ms = match last_event_at {
        Some(at) => (now - at).num_milliseconds().max(0),
        None => i64::MAX,
    };
    let fresh =
        last_slot > 0 && chain == "healthy" && last_event_at.is_some() && lag_ms <= STALE_MARKET_MS;
    if fresh {
        ("READY", true)
    } else if last_slot > 0 {
        ("STALE", false)
    } else {
        ("UNAVAILABLE", false)
    }
}

pub async fn compute(state: &AppState) -> Readiness {
    let now = pumphound_domain::now_utc();
    let (
        last_slot,
        last_event_at,
        chain,
        last_market_snapshot_persisted_at,
        market_snapshot_persist_error,
    ) = {
        let health = state.health.read().await;
        (
            health.last_slot,
            health.last_event_at,
            health.chain.clone(),
            health.last_market_snapshot_persisted_at,
            health.market_snapshot_persist_error.clone(),
        )
    };
    let (_, collector_fresh) = market_freshness(last_slot, last_event_at, &chain, now);
    let (last_market_snapshot_at, market_snapshot_count) = {
        let markets = state.markets.read().await;
        let snapshots = markets.all().collect::<Vec<_>>();
        (
            snapshots
                .iter()
                .map(|snapshot| snapshot.as_of.knowledge_cutoff)
                .max(),
            snapshots.len(),
        )
    };
    let snapshot_idle_ms = idle_ms(last_market_snapshot_at, now).unwrap_or(i64::MAX);
    let snapshot_fresh = market_snapshot_count > 0 && snapshot_idle_ms <= STALE_MARKET_MS;
    let snapshot_store_fresh = idle_ms(last_market_snapshot_persisted_at, now).unwrap_or(i64::MAX)
        <= STALE_MARKET_MS
        && market_snapshot_persist_error.is_none();
    let market_fresh = collector_fresh && snapshot_fresh && snapshot_store_fresh;
    let market_data = if market_fresh {
        "READY"
    } else if last_slot > 0 || market_snapshot_count > 0 {
        "STALE"
    } else {
        "UNAVAILABLE"
    };

    let mode = crate::paper_loop::OperatingMode::parse(&state.mode);
    let execution = if mode.allows_paper_orders() {
        "READY"
    } else {
        "WATCH_ONLY"
    };

    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.stores.postgres)
        .await
        .is_ok();
    let database = if db_ok { "READY" } else { "DOWN" };

    let champions = crate::paper_ledger::champions(&state.stores)
        .await
        .unwrap_or_default();
    let shadows = crate::paper_ledger::has_shadow_models(&state.stores)
        .await
        .unwrap_or(false);
    let short_horizon = champions.contains_key("short_momentum_2m_v1");
    let models = if !champions.is_empty() {
        "READY"
    } else if shadows {
        "SHADOW_ONLY"
    } else {
        "NOT_TRAINED"
    };

    let investigator = std::env::var("PUMPHOUND_INVESTIGATOR_URL")
        .ok()
        .is_some_and(|url| !url.trim().is_empty());
    let ai = if investigator { "ADVISORY" } else { "DEGRADED" };

    let mut reasons = Vec::new();
    if !collector_fresh {
        reasons.push("collector_event_stale_or_absent".into());
    }
    if !snapshot_fresh {
        reasons.push("market_snapshot_stale_or_absent".into());
    }
    if !snapshot_store_fresh {
        reasons.push("market_snapshot_evidence_not_persisted".into());
    }
    if !db_ok {
        reasons.push("postgres_unavailable".into());
    }
    if !mode.allows_paper_orders() {
        reasons.push("mode_does_not_allow_paper_orders".into());
    }
    let quarantine_impact = quarantine_impact(state).await;
    if quarantine_impact.blocks_entries {
        reasons.push("material_hot_market_decode_quarantine".into());
    }
    let allow_new_entries =
        market_fresh && db_ok && mode.allows_paper_orders() && !quarantine_impact.blocks_entries;
    if matches!(mode, crate::paper_loop::OperatingMode::LivePaperMl) && !short_horizon {
        reasons.push("live_paper_ml_missing_short_horizon_champion".into());
    }
    let autonomous_paper = if allow_new_entries {
        "READY"
    } else {
        "BLOCKED"
    };

    Readiness {
        market_data,
        execution,
        models,
        database,
        ai,
        autonomous_paper,
        allow_new_entries,
        reasons,
        last_market_snapshot_at,
        market_snapshot_count,
        last_market_snapshot_persisted_at,
        market_snapshot_persist_error,
    }
}

pub async fn as_json(state: &AppState) -> Value {
    let ready = compute(state).await;
    let health = state.health.read().await;
    let quarantine = quarantine_health(state).await;
    json!({
        "generated_at": pumphound_domain::now_utc(),
        "mode": state.mode,
        "paper_only": true,
        "subsystems": {
            "MARKET DATA": ready.market_data,
            "EXECUTION": ready.execution,
            "MODELS": ready.models,
            "DATABASE": ready.database,
            "AI": ready.ai
        },
        "autonomous_paper": ready.autonomous_paper,
        "allow_new_entries": ready.allow_new_entries,
        "stale_threshold_ms": STALE_MARKET_MS,
        "last_slot": health.last_slot.to_string(),
        "last_event_at": health.last_event_at,
        "last_market_snapshot_at": ready.last_market_snapshot_at,
        "last_market_snapshot_persisted_at": ready.last_market_snapshot_persisted_at,
        "market_snapshot_persist_error": ready.market_snapshot_persist_error,
        "market_snapshot_count": ready.market_snapshot_count,
        "market_snapshot_idle_ms": idle_ms(ready.last_market_snapshot_at, pumphound_domain::now_utc()),
        "market_snapshot_store_idle_ms": idle_ms(ready.last_market_snapshot_persisted_at, pumphound_domain::now_utc()),
        "lag_ms": health.lag_ms,
        "reasons": ready.reasons,
        "layers": {
            "process": if ready.database == "READY" { "RUNNING" } else { "DOWN" },
            "data": ready.market_data,
            "trading": ready.autonomous_paper
        },
        "headline": headline_from_ready(&ready),
        "idle_ms": idle_ms(health.last_event_at, pumphound_domain::now_utc()),
        "source_lag_ms": health.lag_ms,
        "quarantine": quarantine,
        "freshness": {
            "collector_max_idle": format!("{:?}", Duration::from_millis(STALE_MARKET_MS as u64))
        }
    })
}

#[derive(clickhouse::Row, Deserialize)]
struct QuarantineCounts {
    lifetime_total: u64,
    lifetime_quarantined: u64,
    recent_total: u64,
    recent_quarantined: u64,
    invalid_action_rows: u64,
}

#[derive(clickhouse::Row, Deserialize)]
struct QuarantineCause {
    program_id: String,
    reason: String,
    discriminator: String,
    decoder_version: String,
    n: u64,
    first_seen: String,
    last_seen: String,
}

#[derive(clickhouse::Row, Deserialize)]
struct QuarantineEvidence {
    reason: String,
    discriminator: String,
    n: u64,
    recent_total: u64,
}

#[derive(Default, Clone)]
struct QuarantineImpact {
    evidence_available: bool,
    hot_markets: HashSet<String>,
    unattributed_market_events: u64,
    other_unknown_events: u64,
    recent_total: u64,
    blocks_entries: bool,
}

async fn quarantine_impact(state: &AppState) -> QuarantineImpact {
    if let Some(hit) = cached(impact_cache()) {
        return hit;
    }
    let impact = quarantine_impact_uncached(state).await;
    if impact.evidence_available {
        store(impact_cache(), &impact);
    }
    impact
}

async fn quarantine_impact_uncached(state: &AppState) -> QuarantineImpact {
    let aliases = match state.stores.load_hot_mints().await {
        Ok(aliases) => aliases.into_iter().collect::<HashSet<_>>(),
        Err(_) => return QuarantineImpact::default(),
    };
    let rows = state
        .stores
        .clickhouse
        .query(
            // Reads go through the _buffer view (see db/clickhouse/002_insert_buffering.sql)
            // rather than the underlying table: this query wants the last 15 minutes, and
            // rows can sit in the buffer for up to 5s before landing in the base table.
            r#"WITH canonical AS (
              SELECT event_id,
                argMax(kind,revision) AS canonical_kind,
                argMax(action,revision) AS canonical_action,
                argMax(payload_json,revision) AS canonical_payload
              FROM canonical_event_revisions_buffer
              WHERE canonical_event_revisions_buffer.known_at>=now()-INTERVAL 15 MINUTE
              GROUP BY event_id
              HAVING canonical_action='UPSERT'
            )
            SELECT JSONExtractString(canonical_payload,'reason') AS reason,
              JSONExtractString(canonical_payload,'discriminator_hex') AS discriminator,
              count() AS n,
              (SELECT count() FROM canonical) AS recent_total
            FROM canonical
            WHERE canonical_kind='decode.quarantined'
            GROUP BY reason,discriminator"#,
        )
        .fetch_all::<QuarantineEvidence>()
        .await;
    let Ok(rows) = rows else {
        return QuarantineImpact::default();
    };
    let mut impact = QuarantineImpact {
        evidence_available: true,
        ..Default::default()
    };
    for row in rows {
        impact.recent_total = impact.recent_total.max(row.recent_total);
        if let Some(pool) = row
            .reason
            .split("pool=")
            .nth(1)
            .and_then(|suffix| suffix.split_whitespace().next())
        {
            if aliases.contains(pool) {
                impact.hot_markets.insert(pool.to_string());
            }
            continue;
        }
        if is_market_event_discriminator(&row.discriminator) {
            impact.unattributed_market_events += row.n;
        } else if !is_known_non_market_discriminator(&row.discriminator) {
            impact.other_unknown_events += row.n;
        }
    }
    let unattributed = impact.unattributed_market_events + impact.other_unknown_events;
    // A single unattributed event is evidence, not a global outage. One
    // percent of the recent canonical stream is material enough to fail
    // closed until the decoder or attribution is repaired.
    impact.blocks_entries = !impact.hot_markets.is_empty()
        || (unattributed >= 10
            && impact.recent_total > 0
            && unattributed as f64 / impact.recent_total as f64 >= 0.01);
    impact
}

fn is_market_event_discriminator(discriminator: &str) -> bool {
    matches!(
        discriminator,
        "b1310cd2a076a774" // create pool
            | "67f4521f2cf57777" // buy
            | "3e2f370aa503dc2a" // sell
            | "78f83d531f8e6b90" // deposit
            | "1609851aa02c47c0" // withdraw
    )
}

fn is_known_non_market_discriminator(discriminator: &str) -> bool {
    matches!(
        discriminator,
        // Pump program 1.36.0.
        "4045c0681d1e196b"
            | "f53b46224bb96d5c"
            | "93fa6c78f71d43de"
            | "e2d6f62107f293e5"
            | "4facf631cd5bcee8"
            | "929fbdac925838f4"
            | "7a027f010ebf0caf"
            | "a537817004b3ca28"
            | "6161d7905d92167c"
            | "86240d48e86582d8"
            | "9ba768dcd56cf303"
            | "a8d884efebb63134"
            | "2bbcfa12dd4bbb5f"
            | "ed347b25f5fb48d2"
            | "8ecb06207f69bfa2"
            | "dfc39ff63e308f83"
            | "c57aa77c74515bff"
            | "b6c3892a23cecff7"
            | "757be4b6a1a8dcd6"
            // PumpSwap program 1.19.0 additions.
            | "2ddc5d181961ac68"
            | "3f451c16305cc2b9"
            | "e8f5c2eeeada3a59"
            | "6b34598137e25116"
            | "6bfdc14ce4ca1b68"
            | "ae7c4af90451f611"
            | "aadd52c793a5f72e"
            | "f2e7eb664163bdd3"
            | "5980f08d5bca4769"
            | "966bc77b7ccf66e4"
            | "e198ab57f63f42ea"
            | "5a1741233ef4bcd0"
    )
}

pub async fn quarantine_health(state: &AppState) -> Value {
    if let Some(hit) = cached(health_cache()) {
        return hit;
    }
    let value = quarantine_health_uncached(state).await;
    if value.get("state").and_then(Value::as_str) != Some("UNAVAILABLE") {
        store(health_cache(), &value);
    }
    value
}

async fn quarantine_health_uncached(state: &AppState) -> Value {
    let counts = state
        .stores
        .clickhouse
        .query(
            r#"SELECT
          countIf(CAST(t.1 AS Int8)=1) AS lifetime_total,
          countIf(CAST(t.1 AS Int8)=1 AND t.2='decode.quarantined') AS lifetime_quarantined,
          countIf(CAST(t.1 AS Int8)=1 AND t.3>=now()-INTERVAL 15 MINUTE) AS recent_total,
          countIf(CAST(t.1 AS Int8)=1 AND t.2='decode.quarantined' AND t.3>=now()-INTERVAL 15 MINUTE) AS recent_quarantined,
          countIf(CAST(t.1 AS Int8) NOT IN (1,2)) AS invalid_action_rows
        FROM (
          SELECT event_id,argMax(tuple(action,kind,known_at),revision) AS t
          FROM canonical_event_revisions_buffer GROUP BY event_id
        )"#,
        )
        .fetch_one::<QuarantineCounts>()
        .await;
    let Ok(counts) = counts else {
        return json!({
            "state": "UNAVAILABLE",
            "reason": "Canonical decoder-health store is unavailable",
            "window_minutes": 15,
            "recent_rate": null,
            "hot_markets_affected": null,
            "hot_impact_state": "UNKNOWN"
        });
    };
    let causes = state.stores.clickhouse.query(
        r#"SELECT t.3 AS program_id,JSONExtractString(t.4,'reason') AS reason,
          JSONExtractString(t.4,'discriminator_hex') AS discriminator,
          t.5 AS decoder_version,count() AS n,
          formatDateTime(min(t.6),'%FT%T.%fZ') AS first_seen,
          formatDateTime(max(t.6),'%FT%T.%fZ') AS last_seen
        FROM (
          SELECT event_id,argMax(tuple(action,kind,program_id,payload_json,decoder_version,known_at),revision) AS t
          FROM canonical_event_revisions_buffer GROUP BY event_id
        ) WHERE CAST(t.1 AS Int8)=1 AND t.2='decode.quarantined'
          AND t.6>=now()-INTERVAL 15 MINUTE
        GROUP BY program_id,reason,discriminator,decoder_version
        ORDER BY n DESC LIMIT 12"#,
    ).fetch_all::<QuarantineCause>().await.unwrap_or_default();
    let rate = (counts.recent_total > 0)
        .then_some(counts.recent_quarantined as f64 / counts.recent_total as f64);
    let state_label = match rate {
        Some(rate) if rate >= 0.05 => "DEGRADED",
        Some(rate) if rate >= 0.01 => "ELEVATED",
        Some(_) => "READY",
        None => "INSUFFICIENT_EVIDENCE",
    };
    let impact = quarantine_impact(state).await;
    let hot_impact_state = if !impact.evidence_available {
        "UNKNOWN"
    } else if !impact.hot_markets.is_empty() {
        "AFFECTED"
    } else if impact.unattributed_market_events + impact.other_unknown_events > 0 {
        "NO_MATERIAL_HOT_IMPACT_DETECTED"
    } else {
        "CLEAR"
    };
    json!({
        "state": state_label,
        "window_minutes": 15,
        "recent_total": counts.recent_total,
        "recent_quarantined": counts.recent_quarantined,
        "recent_rate": rate,
        "lifetime_total": counts.lifetime_total,
        "lifetime_quarantined": counts.lifetime_quarantined,
        "invalid_canonical_action_rows": counts.invalid_action_rows,
        "hot_markets_affected": impact.hot_markets.len(),
        "hot_impact_state": hot_impact_state,
        "unattributed_market_events": impact.unattributed_market_events,
        "other_unknown_events": impact.other_unknown_events,
        "trading_impact": if impact.blocks_entries {
            "NEW_ENTRIES_BLOCKED"
        } else {
            "NO_MATERIAL_HOT_IMPACT_DETECTED"
        },
        "causes": causes.into_iter().map(|cause| json!({
            "program_id": cause.program_id,
            "reason": cause.reason,
            "discriminator": cause.discriminator,
            "decoder_version": cause.decoder_version,
            "count": cause.n,
            "first_seen": cause.first_seen,
            "last_seen": cause.last_seen
        })).collect::<Vec<_>>()
    })
}

pub async fn alerts_json(state: &AppState) -> Value {
    let ready = compute(state).await;
    let health = state.health.read().await;
    let now = pumphound_domain::now_utc();
    let idle = idle_ms(health.last_event_at, now);
    let mut items = Vec::new();
    if ready.market_data != "READY" {
        items.push(json!({
            "alert_id": "market-data-stale",
            "severity": "CRITICAL",
            "code": "MARKET_DATA_STALE_OR_ABSENT",
            "title": "Market freshness exceeded 20s threshold",
            "detail": "New paper entries suspended while the market clock is stale.",
            "since": health.last_event_at,
            "idle_ms": idle,
            "status": "OPEN"
        }));
    }
    if ready.autonomous_paper == "BLOCKED" {
        items.push(json!({
            "alert_id": "autonomous-paper-blocked",
            "severity": "CRITICAL",
            "code": "AUTONOMOUS_PAPER_BLOCKED",
            "title": "Autonomous paper blocked",
            "detail": ready.reasons.join(" · "),
            "since": health.last_event_at,
            "status": "OPEN"
        }));
    }
    if ready.database == "DOWN" {
        items.push(json!({
            "alert_id": "postgres-down",
            "severity": "CRITICAL",
            "code": "POSTGRES_UNAVAILABLE",
            "title": "Postgres unavailable",
            "status": "OPEN"
        }));
    }
    json!({
        "items": items,
        "headline": headline_from_ready(&ready),
        "generated_at": now
    })
}

pub async fn allow_new_entries(state: &AppState) -> bool {
    compute(state).await.allow_new_entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn stale_market_blocks_freshness() {
        let now = pumphound_domain::now_utc();
        let (_, fresh) = market_freshness(12, Some(now - Duration::seconds(30)), "healthy", now);
        assert!(!fresh);
        let (label, ready) = market_freshness(12, Some(now - Duration::seconds(2)), "healthy", now);
        assert_eq!(label, "READY");
        assert!(ready);
        let (label, ready) = market_freshness(0, None, "disconnected", now);
        assert_eq!(label, "UNAVAILABLE");
        assert!(!ready);
    }

    #[test]
    fn decoder_impact_separates_market_and_administrative_events() {
        assert!(is_market_event_discriminator("3e2f370aa503dc2a"));
        assert!(is_known_non_market_discriminator("3f451c16305cc2b9"));
        assert!(is_known_non_market_discriminator("6161d7905d92167c"));
        assert!(is_known_non_market_discriminator("a537817004b3ca28"));
        assert!(!is_known_non_market_discriminator("ffffffffffffffff"));
    }
}
