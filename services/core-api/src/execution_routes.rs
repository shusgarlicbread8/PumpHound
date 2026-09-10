//! Execution Twin HTTP surface: trace inspection, deterministic replay with labelled
//! counterfactuals, execution-fidelity metrics per book, and twin readiness.
//! Paper-only: replay never writes to the ledger; results land in `execution_replays`.

use crate::execution_twin_bridge::{self, TwinOrderPayload};
use crate::state::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use pumphound_domain::MarketSnapshot;
use pumphound_execution_twin::contract::{result_fingerprint, JitoScenario, SIMULATOR_VERSION};
use pumphound_execution_twin::landing::{LandingModel, LANDING_MODEL_VERSION, SLOT_MS};
use pumphound_execution_twin::network::PriorityFeePolicy;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/execution/status", get(status))
        .route("/api/v1/execution/fidelity", get(fidelity))
        .route("/api/v1/execution/traces/{trace_id}", get(trace))
        .route("/api/v1/execution/replay/{trace_id}", post(replay))
}

async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(status_value(&s).await)
}

/// READY / DEGRADED / BLOCKED for the twin itself. Surfpool is offline verification only and can
/// never block paper trading.
pub async fn status_value(s: &AppState) -> Value {
    let env = execution_twin_bridge::current_fee_environment(s).await;
    let fee_state = if env.is_fallback { "DEGRADED" } else { "READY" };
    let surfpool = std::env::var("PUMPHOUND_SURFPOOL_RPC_URL")
        .ok()
        .filter(|v| !v.is_empty());
    let hot_state_ms = {
        let markets = s.markets.read().await;
        let now = chrono::Utc::now();
        markets
            .all()
            .map(|snap| (now - snap.as_of.knowledge_cutoff).num_milliseconds())
            .min()
    };
    // Twin readiness follows the same market-freshness bar as paper entries; per-order arrival
    // staleness is enforced separately (ARRIVAL_STATE_MAX_STALENESS_MS).
    let hot_fresh =
        hot_state_ms.is_some_and(|ms| (0..=crate::readiness::STALE_MARKET_MS).contains(&ms));
    let overall = if !hot_fresh {
        "BLOCKED"
    } else if env.is_fallback {
        "DEGRADED"
    } else {
        "READY"
    };
    json!({
        "state": overall,
        "simulator_version": SIMULATOR_VERSION,
        "landing_model_version": LANDING_MODEL_VERSION,
        "components": {
            "canonical_quote_path": {"state": "READY", "detail": "exact Pump/PumpSwap math, golden parity 96/96"},
            "fast_svm": {"state": "READY", "detail": "deterministic in-process twin (production paper dependency)"},
            "fee_oracle": {
                "state": fee_state,
                "priority_fee_provider": env.priority_fee_provider,
                "jito_tip_provider": env.jito_tip_provider,
                "observed_at": env.observed_at,
                "congestion": env.congestion.as_str(),
                "is_fallback": env.is_fallback,
                "uncertainty_penalty_bps": env.uncertainty_penalty_bps
            },
            "landing_calibration": {
                "state": "PRIOR_ONLY",
                "detail": "landing-v1-prior is an empirical prior; landing_observations accumulate shadow calibration data"
            },
            "hot_market_state": {
                "state": if hot_fresh { "READY" } else { "STALE" },
                "freshest_snapshot_age_ms": hot_state_ms
            },
            "surfpool_verification": {
                "state": if surfpool.is_some() { "CONFIGURED" } else { "OFFLINE_VERIFICATION_DEGRADED" },
                "blocks_paper_trading": false,
                "rpc_url_configured": surfpool.is_some()
            }
        },
        "paper_only": true,
        "live_money": false
    })
}

async fn fidelity(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match fidelity_value(&s).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"state": "ERROR", "reason": e.to_string()})),
        ),
    }
}

/// Per-book execution fidelity: attempts, fills, failure distribution, landing slots, drag.
pub async fn fidelity_value(s: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT i.strategy_version,
                  COUNT(*)::bigint AS attempts,
                  COUNT(*) FILTER (WHERE r.success)::bigint AS fills,
                  COUNT(*) FILTER (WHERE NOT r.success)::bigint AS failures,
                  COALESCE(SUM(r.network_cost_charged_lamports) FILTER (WHERE NOT r.success),0)::text AS failed_attempt_cost_lamports,
                  COALESCE(SUM(r.network_cost_charged_lamports),0)::text AS network_cost_lamports,
                  COALESCE(SUM(r.protocol_fee_quote+r.creator_fee_quote+r.lp_fee_quote+r.cashback_fee_quote),0)::text AS protocol_fees_quote,
                  percentile_cont(0.5) WITHIN GROUP (ORDER BY r.landing_slots) FILTER (WHERE r.success) AS median_landing_slots,
                  percentile_cont(0.5) WITHIN GROUP (ORDER BY r.price_impact_bps) FILTER (WHERE r.success) AS median_impact_bps,
                  percentile_cont(0.5) WITHIN GROUP (ORDER BY r.adverse_move_bps) FILTER (WHERE r.success) AS median_adverse_bps,
                  percentile_cont(0.5) WITHIN GROUP (ORDER BY r.arrival_delay_ms) AS median_arrival_delay_ms
           FROM pumphound.execution_results r
           JOIN pumphound.execution_intents i ON i.intent_id=r.intent_id
           GROUP BY i.strategy_version ORDER BY i.strategy_version"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let failure_rows = sqlx::query(
        r#"SELECT i.strategy_version, r.failure_class, COUNT(*)::bigint AS n
           FROM pumphound.execution_results r
           JOIN pumphound.execution_intents i ON i.intent_id=r.intent_id
           WHERE NOT r.success GROUP BY 1,2 ORDER BY 1,3 DESC"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let books = rows
        .iter()
        .map(|r| {
            let key: String = r.get("strategy_version");
            let attempts: i64 = r.get("attempts");
            let fills: i64 = r.get("fills");
            let failures = failure_rows
                .iter()
                .filter(|f| f.get::<String, _>("strategy_version") == key)
                .map(|f| {
                    json!({
                        "failure_class": f.get::<Option<String>, _>("failure_class"),
                        "n": f.get::<i64, _>("n")
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "strategy_version": key,
                "simulator_version": SIMULATOR_VERSION,
                "attempts": attempts,
                "fills": fills,
                "failures": r.get::<i64, _>("failures"),
                "fill_rate": (attempts > 0).then(|| fills as f64 / attempts as f64),
                "failure_distribution": failures,
                "failed_attempt_cost_lamports": r.get::<String, _>("failed_attempt_cost_lamports"),
                "network_cost_lamports": r.get::<String, _>("network_cost_lamports"),
                "protocol_fees_quote": r.get::<String, _>("protocol_fees_quote"),
                "median_landing_slots": r.get::<Option<f64>, _>("median_landing_slots"),
                "median_price_impact_bps": r.get::<Option<f64>, _>("median_impact_bps"),
                "median_adverse_move_bps": r.get::<Option<f64>, _>("median_adverse_bps"),
                "median_arrival_delay_ms": r.get::<Option<f64>, _>("median_arrival_delay_ms"),
                "sample_state": if attempts < 30 { "insufficient" } else if attempts < 100 { "early" } else { "sufficient" }
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "simulator_version": SIMULATOR_VERSION,
        "landing_model_version": LANDING_MODEL_VERSION,
        "books": books,
        "note": "Legacy books (legacy-paper-v1) have no execution_results and are not listed; their fills assume a fixed 1s delay and always land."
    }))
}

async fn trace(State(s): State<Arc<AppState>>, Path(trace_id): Path<Uuid>) -> impl IntoResponse {
    match trace_value(&s, trace_id).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"trace_id": trace_id, "state": "ERROR", "reason": e.to_string()})),
        ),
    }
}

pub async fn trace_value(s: &AppState, trace_id: Uuid) -> anyhow::Result<Value> {
    let intents = sqlx::query(
        "SELECT intent, order_id::text FROM pumphound.execution_intents WHERE trace_id=$1 ORDER BY decision_at",
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let results = sqlx::query(
        "SELECT result, fingerprint FROM pumphound.execution_results WHERE trace_id=$1 ORDER BY created_at",
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let costs = sqlx::query(
        r#"SELECT result_id::text,component,quote_atomic::text,bps,charged,source
           FROM pumphound.execution_cost_components WHERE trace_id=$1"#,
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let events = sqlx::query(
        r#"SELECT stage,occurred_at,slot,entity_type,entity_id,experiment_id,strategy_version,
                  simulator_version,payload
           FROM pumphound.trade_trace_events WHERE trace_id=$1 ORDER BY occurred_at"#,
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let landing = sqlx::query(
        r#"SELECT predicted_landing_slots,predicted_arrival_delay_ms,predicted_arrival_price_e9::text,
                  observed_arrival_slot,observed_arrival_at,observed_arrival_price_e9::text,
                  observed_state_delay_ms,congestion,landing_model_version
           FROM pumphound.landing_observations WHERE trace_id=$1 ORDER BY created_at"#,
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let replays = sqlx::query(
        r#"SELECT replay_id::text,counterfactual,fingerprint,matches_original,created_at
           FROM pumphound.execution_replays WHERE trace_id=$1 ORDER BY created_at DESC LIMIT 50"#,
    )
    .bind(trace_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    Ok(json!({
        "trace_id": trace_id,
        "simulator_version": SIMULATOR_VERSION,
        "intents": intents.iter().map(|r| json!({"intent": r.get::<Value,_>("intent"), "order_id": r.get::<Option<String>,_>("order_id")})).collect::<Vec<_>>(),
        "results": results.iter().map(|r| json!({"result": r.get::<Value,_>("result"), "fingerprint": r.get::<String,_>("fingerprint")})).collect::<Vec<_>>(),
        "cost_components": costs.iter().map(|r| json!({
            "result_id": r.get::<String,_>("result_id"),
            "component": r.get::<String,_>("component"),
            "quote_atomic": r.get::<String,_>("quote_atomic"),
            "bps": r.get::<Option<i32>,_>("bps"),
            "charged": r.get::<bool,_>("charged"),
            "source": r.get::<String,_>("source")
        })).collect::<Vec<_>>(),
        "events": events.iter().map(|r| json!({
            "stage": r.get::<String,_>("stage"),
            "occurred_at": r.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at"),
            "slot": r.get::<Option<i64>,_>("slot"),
            "entity_type": r.get::<Option<String>,_>("entity_type"),
            "entity_id": r.get::<Option<String>,_>("entity_id"),
            "experiment_id": r.get::<Option<String>,_>("experiment_id"),
            "strategy_version": r.get::<Option<String>,_>("strategy_version"),
            "simulator_version": r.get::<Option<String>,_>("simulator_version"),
            "payload": r.get::<Value,_>("payload")
        })).collect::<Vec<_>>(),
        "landing_observations": landing.iter().map(|r| json!({
            "predicted_landing_slots": r.get::<Option<i32>,_>("predicted_landing_slots"),
            "predicted_arrival_delay_ms": r.get::<i64,_>("predicted_arrival_delay_ms"),
            "predicted_arrival_price_e9": r.get::<Option<String>,_>("predicted_arrival_price_e9"),
            "observed_arrival_slot": r.get::<Option<i64>,_>("observed_arrival_slot"),
            "observed_arrival_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("observed_arrival_at"),
            "observed_arrival_price_e9": r.get::<Option<String>,_>("observed_arrival_price_e9"),
            "observed_state_delay_ms": r.get::<Option<i64>,_>("observed_state_delay_ms"),
            "congestion": r.get::<String,_>("congestion"),
            "landing_model_version": r.get::<String,_>("landing_model_version")
        })).collect::<Vec<_>>(),
        "replays": replays.iter().map(|r| json!({
            "replay_id": r.get::<String,_>("replay_id"),
            "counterfactual": r.get::<Value,_>("counterfactual"),
            "fingerprint": r.get::<String,_>("fingerprint"),
            "matches_original": r.get::<Option<bool>,_>("matches_original"),
            "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
        })).collect::<Vec<_>>(),
        "paper_only": true
    }))
}

/// Counterfactual knobs. All optional; an empty body is a pure determinism replay.
#[derive(Debug, Default, Deserialize)]
pub struct ReplayRequest {
    /// Which intent within the trace (default: first / entry).
    pub intent_id: Option<Uuid>,
    /// Shift the sampled arrival by N slots (e.g. +1). Arrival state is still the recorded one;
    /// the shift changes fee semantics/landing classification, not market state (no fabrication).
    pub landing_delay_slots: Option<i32>,
    pub priority_fee_policy: Option<String>,
    pub jito_scenario: Option<String>,
    pub slippage_limit_bps: Option<u32>,
    /// One of FAST/BASE/SLOW/STRESSED (see `canonical_scenario`). Unlike `landing_delay_slots`,
    /// this resolves *real* market state at the scenario's target slot from `market_snapshots`
    /// instead of reusing the one arrival snapshot recorded live — a trade landing later saw
    /// different price/liquidity, not the same snapshot with different fees stapled on. Mutually
    /// exclusive with the manual knobs above (returns an error if both are set): this field exists
    /// for systematic dataset construction, not ad-hoc tweaking of one already-precise knob.
    pub scenario: Option<String>,
}

/// FAST/BASE/SLOW/STRESSED, in terms this simulator's own landing model already understands:
/// `attempted_slot = decision_slot + 1` is the earliest a transaction can land at all, and the
/// model itself resolves an expired/never-landed transaction at `landing_slots=3` beyond that
/// (`landing.rs::sample`) — i.e. `decision_slot + 5` is where "timed out" territory begins. These
/// four offsets are chosen relative to those two anchors, not fitted percentiles of a continuous
/// distribution (the landing model is a 4-outcome discrete draw, not one) — do not read them as
/// calibrated p25/p50/p75/p95. `BASE` is a passthrough: replay the trace exactly as it actually
/// happened, no override, no historical-state resolution.
fn canonical_scenario(name: &str) -> Option<Option<(PriorityFeePolicy, JitoScenario, u64)>> {
    match name.to_ascii_uppercase().as_str() {
        "BASE" => Some(None),
        "FAST" => Some(Some((PriorityFeePolicy::P25, JitoScenario::NoJito, 1))),
        "SLOW" => Some(Some((PriorityFeePolicy::P50, JitoScenario::NoJito, 3))),
        "STRESSED" => Some(Some((PriorityFeePolicy::P95, JitoScenario::Aggressive, 6))),
        _ => None,
    }
}

async fn replay(
    State(s): State<Arc<AppState>>,
    Path(trace_id): Path<Uuid>,
    body: Option<Json<ReplayRequest>>,
) -> impl IntoResponse {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    match replay_value(&s, trace_id, &req).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"trace_id": trace_id, "state": "REPLAY_UNAVAILABLE", "reason": e.to_string()})),
        ),
    }
}

pub struct StoredExecution {
    pub payload: TwinOrderPayload,
    pub original_result_id: Option<Uuid>,
    pub original_fingerprint: Option<String>,
    pub arrival_snapshot: Option<MarketSnapshot>,
}

pub async fn load_execution(
    stores: &pumphound_storage::Stores,
    trace_id: Uuid,
    intent_id: Option<Uuid>,
) -> anyhow::Result<StoredExecution> {
    let row = match intent_id {
        Some(id) => {
            sqlx::query(
                r#"SELECT i.intent_id,o.payload FROM pumphound.execution_intents i
                   JOIN pumphound.paper_orders o ON o.order_id=i.order_id
                   WHERE i.trace_id=$1 AND i.intent_id=$2"#,
            )
            .bind(trace_id)
            .bind(id)
            .fetch_optional(&stores.postgres)
            .await?
        }
        None => {
            sqlx::query(
                r#"SELECT i.intent_id,o.payload FROM pumphound.execution_intents i
                   JOIN pumphound.paper_orders o ON o.order_id=i.order_id
                   WHERE i.trace_id=$1 ORDER BY i.decision_at LIMIT 1"#,
            )
            .bind(trace_id)
            .fetch_optional(&stores.postgres)
            .await?
        }
    };
    let row = row.ok_or_else(|| anyhow::anyhow!("no queued execution intent for this trace"))?;
    let intent_id: Uuid = row.get("intent_id");
    let payload = execution_twin_bridge::twin_payload(&row.get::<Value, _>("payload"))
        .ok_or_else(|| anyhow::anyhow!("order payload has no execution_twin section"))?;
    let original = sqlx::query(
        "SELECT result_id,fingerprint FROM pumphound.execution_results WHERE intent_id=$1",
    )
    .bind(intent_id)
    .fetch_optional(&stores.postgres)
    .await?;
    let arrival_snapshot: Option<MarketSnapshot> = sqlx::query_scalar::<_, Value>(
        r#"SELECT payload->'arrival_snapshot' FROM pumphound.trade_trace_events
           WHERE trace_id=$1 AND stage='SIMULATION' AND payload->'result'->>'intent_id'=$2
           ORDER BY occurred_at LIMIT 1"#,
    )
    .bind(trace_id)
    .bind(intent_id.to_string())
    .fetch_optional(&stores.postgres)
    .await?
    .and_then(|v| serde_json::from_value(v).ok());
    Ok(StoredExecution {
        payload,
        original_result_id: original.as_ref().map(|r| r.get("result_id")),
        original_fingerprint: original.as_ref().map(|r| r.get("fingerprint")),
        arrival_snapshot,
    })
}

pub async fn replay_value(
    s: &AppState,
    trace_id: Uuid,
    req: &ReplayRequest,
) -> anyhow::Result<Value> {
    let stored = load_execution(&s.stores, trace_id, req.intent_id).await?;
    let mut payload = stored.payload.clone();
    let mut counterfactual = serde_json::Map::new();
    if req.scenario.is_some()
        && (req.priority_fee_policy.is_some()
            || req.jito_scenario.is_some()
            || req.landing_delay_slots.is_some())
    {
        anyhow::bail!(
            "scenario is mutually exclusive with priority_fee_policy/jito_scenario/landing_delay_slots"
        );
    }
    let mut arrival_snapshot = stored.arrival_snapshot.clone();
    let mut historical_state_resolved: Option<bool> = None;
    if let Some(name) = req.scenario.as_deref() {
        let parsed = canonical_scenario(name).ok_or_else(|| {
            anyhow::anyhow!("unknown scenario {name} (expected FAST/BASE/SLOW/STRESSED)")
        })?;
        counterfactual.insert("scenario".into(), json!(name.to_ascii_uppercase()));
        if let Some((fee_policy, jito, offset_slots)) = parsed {
            payload.config.priority_fee_policy = fee_policy;
            payload.config.jito_scenario = jito;
            payload.intent.cost_scenario = jito;
            counterfactual.insert("priority_fee_policy".into(), json!(fee_policy.as_str()));
            counterfactual.insert("jito_scenario".into(), json!(jito.as_str()));
            counterfactual.insert("scenario_offset_slots".into(), json!(offset_slots));
            let target_slot = payload.intent.decision_slot.0 + offset_slots;
            let resolved = s
                .stores
                .market_snapshot_at_or_after(&payload.intent.mint.to_string(), target_slot)
                .await?;
            historical_state_resolved = Some(resolved.is_some());
            // None here is honest degradation (DATA_STALE via execute_payload below), not a
            // silent fallback to the original recorded snapshot — a scenario that asked for
            // market state 6 slots later must not be scored against state from slot+1.
            arrival_snapshot = resolved;
        }
    }
    if let Some(policy) = req.priority_fee_policy.as_deref() {
        let parsed = PriorityFeePolicy::parse(policy)
            .ok_or_else(|| anyhow::anyhow!("unknown priority_fee_policy {policy}"))?;
        payload.config.priority_fee_policy = parsed;
        counterfactual.insert("priority_fee_policy".into(), json!(parsed.as_str()));
    }
    if let Some(scenario) = req.jito_scenario.as_deref() {
        let parsed = JitoScenario::parse(scenario)
            .ok_or_else(|| anyhow::anyhow!("unknown jito_scenario {scenario}"))?;
        payload.config.jito_scenario = parsed;
        payload.intent.cost_scenario = parsed;
        counterfactual.insert("jito_scenario".into(), json!(parsed.as_str()));
    }
    if let Some(bps) = req.slippage_limit_bps {
        payload.intent.slippage_limit_bps = bps;
        counterfactual.insert("slippage_limit_bps".into(), json!(bps));
    }
    if let Some(shift) = req.landing_delay_slots {
        let base = payload.config.landing;
        payload.config.landing = LandingModel {
            submit_latency_ms: base
                .submit_latency_ms
                .saturating_add_signed(shift as i64 * SLOT_MS as i64),
        };
        counterfactual.insert("landing_delay_slots".into(), json!(shift));
    }
    // scenario="BASE" is a labelled passthrough, not a counterfactual: it must still report
    // matches_original like an empty body would. Any other key means a real override applied.
    let is_counterfactual = counterfactual
        .iter()
        .any(|(key, value)| key != "scenario" || value.as_str() != Some("BASE"));
    let result = execution_twin_bridge::execute_payload(&payload, arrival_snapshot.as_ref());
    let fingerprint = result_fingerprint(&result);
    let matches_original = if is_counterfactual {
        None
    } else {
        stored
            .original_fingerprint
            .as_ref()
            .map(|orig| orig == &fingerprint)
    };
    let replay_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO pumphound.execution_replays
           (replay_id,trace_id,intent_id,original_result_id,simulator_version,counterfactual,
            fingerprint,matches_original,result)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)"#,
    )
    .bind(replay_id)
    .bind(trace_id)
    .bind(payload.intent.intent_id)
    .bind(stored.original_result_id)
    .bind(SIMULATOR_VERSION)
    .bind(Value::Object(counterfactual.clone()))
    .bind(&fingerprint)
    .bind(matches_original)
    .bind(serde_json::to_value(&result)?)
    .execute(&s.stores.postgres)
    .await?;
    Ok(json!({
        "replay_id": replay_id,
        "trace_id": trace_id,
        "intent_id": payload.intent.intent_id,
        "label": if is_counterfactual { "COUNTERFACTUAL_SIMULATION" } else { "DETERMINISTIC_REPLAY" },
        "counterfactual": counterfactual,
        "simulator_version": SIMULATOR_VERSION,
        "fingerprint": fingerprint,
        "original_fingerprint": stored.original_fingerprint,
        "matches_original": matches_original,
        "arrival_state_available": arrival_snapshot.is_some(),
        "historical_state_resolved": historical_state_resolved,
        "result": result,
        "paper_only": true,
        "live_money": false
    }))
}
