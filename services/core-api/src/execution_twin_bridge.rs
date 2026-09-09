//! Bridge between the paper loop/ledger and `pumphound_execution_twin`.
//!
//! Paper-only. Nothing here signs or submits; the "transaction" is a canonical message built
//! for simulation and shadow calibration. Responsibilities:
//!   * fee oracle poller (Helius priority-fee estimate + Jito public tip floor, with a labelled
//!     static fallback) → `AppState::fee_env` + `fee_environment_observations`
//!   * turn a `PaperOrderRequest` into an immutable `ExecutionIntent`, plan its landing, and
//!     persist the intent + trace stage before the order is queued
//!   * on arrival, run the twin against the ARRIVAL snapshot, persist the `ExecutionResult`,
//!     cost components, landing observation and trace stages, then hand the ledger either a
//!     fill (built from the arrival quote) or a classified failure with the correct fee charge.

use crate::paper_ledger::{self, TwinFill, TwinQueue};
use crate::state::AppState;
use chrono::Utc;
use pumphound_domain::{
    AssetAmount, Atomic, FeeComponent, FeeKind, MarketSnapshot, PaperOrderRequest, PaperQuote,
    Protocol, Pubkey, TradeSide,
};
use pumphound_execution_twin::builder::MarketIdentity;
use pumphound_execution_twin::contract::{
    result_fingerprint, ExecutionIntent, ExecutionResult, FailureClass, JitoScenario,
    SIMULATOR_VERSION,
};
use pumphound_execution_twin::landing::LANDING_MODEL_VERSION;
use pumphound_execution_twin::network::{CongestionClass, FeeEnvironment, Percentiles};
use pumphound_execution_twin::twin::{self, ExecutionInputs, TwinConfig};
use pumphound_storage::Stores;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const FEE_ORACLE_INTERVAL: Duration = Duration::from_secs(20);
const FEE_ENV_MAX_AGE: chrono::Duration = chrono::Duration::seconds(120);
/// Volume the paper loop is willing to tolerate before treating the hot-market snapshot as
/// unusable for arrival-state execution.
pub const ARRIVAL_STATE_MAX_STALENESS_MS: i64 = 4_000;

/// Frozen per-book twin configuration stored in the order payload so settlement and replay use
/// exactly what the decision used, regardless of later config changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TwinOrderPayload {
    pub intent: ExecutionIntent,
    pub decision_snapshot: MarketSnapshot,
    pub identity: MarketIdentity,
    pub config: TwinConfig,
    pub fee_env: FeeEnvironment,
    pub user_ata_exists: bool,
}

pub fn default_config() -> TwinConfig {
    TwinConfig::default()
}

// ---------------------------------------------------------------------------------------------
// Fee oracle
// ---------------------------------------------------------------------------------------------

pub fn spawn_fee_oracle(state: Arc<AppState>) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .build()
            .expect("reqwest client");
        loop {
            let env = observe_fee_environment(&client).await;
            if let Err(error) = persist_fee_environment(&state.stores, &env).await {
                tracing::warn!(%error, "fee environment persist failed");
            }
            *state.fee_env.write().await = Some(env);
            tokio::time::sleep(FEE_ORACLE_INTERVAL).await;
        }
    });
}

/// Current fee environment; always returns something usable. Stale or missing observations
/// degrade to the static fallback and say so (`is_fallback`, provider label).
pub async fn current_fee_environment(state: &AppState) -> FeeEnvironment {
    let now = Utc::now();
    match state.fee_env.read().await.clone() {
        Some(env) if now - env.observed_at <= FEE_ENV_MAX_AGE => env,
        Some(stale) => FeeEnvironment {
            priority_fee_provider: format!("stale:{}", stale.priority_fee_provider),
            jito_tip_provider: format!("stale:{}", stale.jito_tip_provider),
            is_fallback: true,
            uncertainty_penalty_bps: stale.uncertainty_penalty_bps.max(25),
            ..stale
        },
        None => FeeEnvironment::fallback(now),
    }
}

async fn observe_fee_environment(client: &reqwest::Client) -> FeeEnvironment {
    let now = Utc::now();
    let mut env = FeeEnvironment::fallback(now);
    let mut components_observed = 0u8;
    if let Some(p) = helius_priority_fee(client).await {
        env.cu_price_micro_lamports = p;
        env.priority_fee_provider = "helius:getPriorityFeeEstimate".into();
        env.congestion = CongestionClass::from_cu_price_p50(p.p50);
        components_observed += 1;
    }
    if let Some(p) = jito_tip_floor(client).await {
        env.jito_tip_lamports = p;
        env.jito_tip_provider = "jito:bundles.jito.wtf/tip_floor".into();
        components_observed += 1;
    }
    env.is_fallback = components_observed < 2;
    env.uncertainty_penalty_bps = match components_observed {
        2 => 0,
        1 => 15,
        _ => 30,
    };
    env
}

async fn helius_priority_fee(client: &reqwest::Client) -> Option<Percentiles> {
    let key = std::env::var("PUMPHOUND_HELIUS_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())?;
    let url = format!("https://mainnet.helius-rpc.com/?api-key={key}");
    let body = json!({
        "jsonrpc": "2.0", "id": "pumphound-fee-oracle",
        "method": "getPriorityFeeEstimate",
        "params": [{
            "accountKeys": [pumphound_execution_twin::pda::PUMP_PROGRAM, pumphound_execution_twin::pda::PUMP_AMM_PROGRAM],
            "options": {"includeAllPriorityFeeLevels": true}
        }]
    });
    let value: Value = client.post(url).json(&body).send().await.ok()?.json().await.ok()?;
    let levels = value.pointer("/result/priorityFeeLevels")?;
    let pick = |k: &str| levels.get(k).and_then(Value::as_f64).map(|v| v.round() as u64);
    Some(Percentiles {
        p25: pick("low")?,
        p50: pick("medium")?,
        p75: pick("high")?,
        p95: pick("veryHigh")?,
    })
}

async fn jito_tip_floor(client: &reqwest::Client) -> Option<Percentiles> {
    let value: Value = client
        .get("https://bundles.jito.wtf/api/v1/bundles/tip_floor")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let row = value.as_array()?.first()?;
    let sol = |k: &str| {
        row.get(k)
            .and_then(Value::as_f64)
            .map(|v| (v * 1e9).round() as u64)
    };
    Some(Percentiles {
        p25: sol("landed_tips_25th_percentile")?,
        p50: sol("landed_tips_50th_percentile")?,
        p75: sol("landed_tips_75th_percentile")?,
        p95: sol("landed_tips_95th_percentile")?,
    })
}

async fn persist_fee_environment(stores: &Stores, env: &FeeEnvironment) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.fee_environment_observations
           (observation_id,observed_at,priority_fee_provider,cu_price_p25,cu_price_p50,cu_price_p75,
            cu_price_p95,jito_tip_provider,jito_tip_p25,jito_tip_p50,jito_tip_p75,jito_tip_p95,
            congestion,is_fallback,raw)
           VALUES ($1,$2,$3,$4::numeric,$5::numeric,$6::numeric,$7::numeric,$8,$9::numeric,
                   $10::numeric,$11::numeric,$12::numeric,$13,$14,$15)"#,
    )
    .bind(Uuid::now_v7())
    .bind(env.observed_at)
    .bind(&env.priority_fee_provider)
    .bind(env.cu_price_micro_lamports.p25.to_string())
    .bind(env.cu_price_micro_lamports.p50.to_string())
    .bind(env.cu_price_micro_lamports.p75.to_string())
    .bind(env.cu_price_micro_lamports.p95.to_string())
    .bind(&env.jito_tip_provider)
    .bind(env.jito_tip_lamports.p25.to_string())
    .bind(env.jito_tip_lamports.p50.to_string())
    .bind(env.jito_tip_lamports.p75.to_string())
    .bind(env.jito_tip_lamports.p95.to_string())
    .bind(env.congestion.as_str())
    .bind(env.is_fallback)
    .bind(serde_json::to_value(env)?)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Intent construction + queueing
// ---------------------------------------------------------------------------------------------

pub fn market_identity(
    snapshot: &MarketSnapshot,
    quote_mint: Pubkey,
    creator: Option<Pubkey>,
) -> anyhow::Result<MarketIdentity> {
    Ok(MarketIdentity {
        mint: snapshot.mint,
        market: snapshot.market,
        quote_mint,
        creator,
        base_token_program: Pubkey::from_base58(TOKEN_PROGRAM)?,
    })
}

pub fn build_intent(
    req: &PaperOrderRequest,
    snapshot: &MarketSnapshot,
    experiment_id: &str,
    strategy_version: &str,
    model_versions: Vec<String>,
    scenario: JitoScenario,
    trace_id: Uuid,
) -> anyhow::Result<ExecutionIntent> {
    let (quote_asset, requested_quote_amount, token_amount_if_sell) = match req.side {
        TradeSide::Buy => (req.input.mint, u64::try_from(req.input.atomic.0)?, 0),
        TradeSide::Sell => (req.output_mint, 0, u64::try_from(req.input.atomic.0)?),
    };
    Ok(ExecutionIntent {
        trace_id,
        intent_id: Uuid::now_v7(),
        experiment_id: experiment_id.to_string(),
        strategy_version: strategy_version.to_string(),
        decision_id: req.decision_id,
        decision_timestamp: Utc::now(),
        decision_slot: snapshot.as_of.slot,
        mint: snapshot.mint,
        market: snapshot.market,
        venue: snapshot.protocol,
        side: req.side,
        quote_asset,
        requested_quote_amount,
        token_amount_if_sell,
        slippage_limit_bps: req.max_slippage_bps,
        strategy_model_versions: model_versions,
        market_snapshot_id: snapshot.snapshot_id.clone(),
        cost_scenario: scenario,
    })
}

/// Legacy-shaped decision quote derived from the twin's exact decision quote so the existing
/// `paper_quotes` / order payload columns keep their meaning for twin orders.
fn paper_quote_from_exact(
    req: &PaperOrderRequest,
    q: &pumphound_execution_twin::quote::ExactQuote,
    snapshot: &MarketSnapshot,
) -> PaperQuote {
    let quote_mint = match req.side {
        TradeSide::Buy => req.input.mint,
        TradeSide::Sell => req.output_mint,
    };
    let fee = |kind: FeeKind, atomic: u64| FeeComponent {
        kind,
        asset: AssetAmount {
            mint: quote_mint,
            atomic: Atomic(atomic as u128),
            decimals: 9,
        },
        schedule_version: format!("{}:{}", SIMULATOR_VERSION, q.protocol_fees.fee_tier),
        charged_on_failure: false,
    };
    let mut fee_components = vec![fee(FeeKind::Protocol, q.protocol_fees.protocol_fee_quote)];
    if q.protocol_fees.creator_fee_quote > 0 {
        fee_components.push(fee(FeeKind::Creator, q.protocol_fees.creator_fee_quote));
    }
    if q.protocol_fees.lp_fee_quote > 0 {
        fee_components.push(fee(FeeKind::Lp, q.protocol_fees.lp_fee_quote));
    }
    if q.protocol_fees.cashback_fee_quote > 0 {
        fee_components.push(fee(FeeKind::Cashback, q.protocol_fees.cashback_fee_quote));
    }
    let (expected, minimum) = match req.side {
        TradeSide::Buy => (
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(q.filled_base as u128),
                decimals: 6,
            },
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(q.minimum_output as u128),
                decimals: 6,
            },
        ),
        TradeSide::Sell => (
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(q.filled_quote as u128),
                decimals: 9,
            },
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(q.minimum_output as u128),
                decimals: 9,
            },
        ),
    };
    PaperQuote {
        quote_id: req.quote_id,
        market_snapshot_id: snapshot.snapshot_id.clone(),
        input: req.input.clone(),
        expected_output: expected,
        minimum_output: minimum,
        reserve_input: req.input.clone(),
        fee_components,
        price_impact_bps: q.price_impact_bps,
        expires_at: Utc::now() + chrono::Duration::seconds(30),
        quality: snapshot.quality.clone(),
    }
}

#[allow(dead_code)]
pub struct QueuedTwinOrder {
    pub order_id: Uuid,
    pub trace_id: Uuid,
    pub intent_id: Uuid,
    pub arrival_delay_ms: u64,
}

/// Decision → exact decision quote → intent → landing plan → queued order carrying the frozen
/// twin payload. Returns `Ok(None)` when the twin refuses to quote the decision state (the
/// refusal is persisted as an intent + trace stage so the funnel stays honest).
#[allow(clippy::too_many_arguments)]
pub async fn queue_twin_order(
    state: &AppState,
    req: &PaperOrderRequest,
    snapshot: &MarketSnapshot,
    experiment_id: &str,
    strategy_version: &str,
    model_versions: Vec<String>,
    config: &TwinConfig,
    maximum_arrival_wait_ms: u64,
    trace_id: Uuid,
    portfolio_run_id: Uuid,
    creator: Option<Pubkey>,
) -> anyhow::Result<Option<QueuedTwinOrder>> {
    let fee_env = current_fee_environment(state).await;
    let intent = build_intent(
        req,
        snapshot,
        experiment_id,
        strategy_version,
        model_versions,
        config.jito_scenario,
        trace_id,
    )?;
    let quote_mint = intent.quote_asset;
    let identity = market_identity(snapshot, quote_mint, creator)?;
    let amount = match req.side {
        TradeSide::Buy => intent.requested_quote_amount,
        TradeSide::Sell => intent.token_amount_if_sell,
    };
    let exact = pumphound_execution_twin::quote::exact_quote(
        snapshot,
        req.side,
        amount,
        req.max_slippage_bps,
    );
    persist_intent(&state.stores, &intent, None, req.portfolio_id, portfolio_run_id).await?;
    let exact = match exact {
        Ok(q) => q,
        Err(rejection) => {
            trace_event(
                &state.stores,
                trace_id,
                "EXECUTION_INTENT_REJECTED",
                Some(intent.decision_slot.0 as i64),
                "intent",
                &intent.intent_id.to_string(),
                &intent,
                json!({"failure_class": rejection.class.as_str(), "detail": rejection.detail}),
            )
            .await?;
            return Ok(None);
        }
    };
    let (landing, _) = twin::plan_landing(&intent, &fee_env, config);
    let user_ata_exists = req.side == TradeSide::Sell
        || paper_ledger::portfolio_holds_mint(
            &state.stores,
            req.portfolio_id,
            &snapshot.mint.to_string(),
        )
        .await
        .unwrap_or(false);
    let payload = TwinOrderPayload {
        intent: intent.clone(),
        decision_snapshot: snapshot.clone(),
        identity,
        config: config.clone(),
        fee_env: fee_env.clone(),
        user_ata_exists,
    };
    let decision_quote = paper_quote_from_exact(req, &exact, snapshot);
    let order_id = paper_ledger::queue_order_with(
        &state.stores,
        req,
        &decision_quote,
        landing.arrival_delay_ms,
        maximum_arrival_wait_ms,
        Some(TwinQueue {
            trace_id,
            intent_id: intent.intent_id,
            simulator_version: SIMULATOR_VERSION,
            payload: serde_json::to_value(&payload)?,
        }),
    )
    .await?;
    sqlx::query("UPDATE pumphound.execution_intents SET order_id=$2 WHERE intent_id=$1")
        .bind(intent.intent_id)
        .bind(order_id)
        .execute(&state.stores.postgres)
        .await?;
    trace_event(
        &state.stores,
        trace_id,
        "EXECUTION_INTENT",
        Some(intent.decision_slot.0 as i64),
        "order",
        &order_id.to_string(),
        &intent,
        json!({
            "intent": intent,
            "decision_quote": exact,
            "landing_plan": landing,
            "fee_environment": fee_env,
            "config": config
        }),
    )
    .await?;
    Ok(Some(QueuedTwinOrder {
        order_id,
        trace_id,
        intent_id: intent.intent_id,
        arrival_delay_ms: landing.arrival_delay_ms,
    }))
}

async fn persist_intent(
    stores: &Stores,
    intent: &ExecutionIntent,
    order_id: Option<Uuid>,
    portfolio_id: Uuid,
    run_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.execution_intents
           (intent_id,trace_id,order_id,decision_id,portfolio_id,simulation_run_id,experiment_id,
            strategy_version,simulator_version,decision_at,decision_slot,mint,market,venue,side,
            quote_asset,requested_quote_amount,token_amount_if_sell,slippage_limit_bps,cost_scenario,
            market_snapshot_id,intent)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17::numeric,$18::numeric,
                   $19,$20,$21,$22)
           ON CONFLICT (intent_id) DO NOTHING"#,
    )
    .bind(intent.intent_id)
    .bind(intent.trace_id)
    .bind(order_id)
    .bind(intent.decision_id)
    .bind(portfolio_id)
    .bind(run_id)
    .bind(&intent.experiment_id)
    .bind(&intent.strategy_version)
    .bind(SIMULATOR_VERSION)
    .bind(intent.decision_timestamp)
    .bind(intent.decision_slot.0 as i64)
    .bind(intent.mint.to_string())
    .bind(intent.market.to_string())
    .bind(protocol_name(intent.venue))
    .bind(match intent.side {
        TradeSide::Buy => "buy",
        TradeSide::Sell => "sell",
    })
    .bind(intent.quote_asset.to_string())
    .bind(intent.requested_quote_amount.to_string())
    .bind(intent.token_amount_if_sell.to_string())
    .bind(intent.slippage_limit_bps as i32)
    .bind(intent.cost_scenario.as_str())
    .bind(&intent.market_snapshot_id)
    .bind(serde_json::to_value(intent)?)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

fn protocol_name(p: Protocol) -> &'static str {
    match p {
        Protocol::PumpCurve => "pump_curve",
        Protocol::PumpSwap => "pumpswap",
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn trace_event(
    stores: &Stores,
    trace_id: Uuid,
    stage: &str,
    slot: Option<i64>,
    entity_type: &str,
    entity_id: &str,
    intent: &ExecutionIntent,
    payload: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.trade_trace_events
           (trace_event_id,trace_id,stage,occurred_at,slot,entity_type,entity_id,experiment_id,
            strategy_version,simulator_version,payload,idempotency_key)
           VALUES ($1,$2,$3,now(),$4,$5,$6,$7,$8,$9,$10,$11)
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(trace_id)
    .bind(stage)
    .bind(slot)
    .bind(entity_type)
    .bind(entity_id)
    .bind(&intent.experiment_id)
    .bind(&intent.strategy_version)
    .bind(SIMULATOR_VERSION)
    .bind(payload)
    .bind(format!("trace:{trace_id}:{stage}:{entity_type}:{entity_id}"))
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Settlement
// ---------------------------------------------------------------------------------------------

/// Curve creator / canonical-pool coin_creator for a launch (needed for the creator-vault PDA).
pub async fn launch_creator(stores: &Stores, launch_id: Uuid) -> Option<Pubkey> {
    sqlx::query_scalar::<_, String>("SELECT creator FROM pumphound.launches WHERE launch_id=$1")
        .bind(launch_id)
        .fetch_optional(&stores.postgres)
        .await
        .ok()
        .flatten()
        .and_then(|c| Pubkey::from_base58(&c).ok())
}

pub fn twin_payload(order_payload: &Value) -> Option<TwinOrderPayload> {
    order_payload
        .get("execution_twin")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
}

/// Pure: run the twin for a stored order payload against an arrival snapshot. Used by live
/// settlement AND replay so both paths share one code path (replay determinism depends on it).
pub fn execute_payload(
    payload: &TwinOrderPayload,
    arrival_snapshot: Option<&MarketSnapshot>,
) -> ExecutionResult {
    twin::execute(ExecutionInputs {
        intent: &payload.intent,
        identity: &payload.identity,
        decision_snapshot: &payload.decision_snapshot,
        arrival_snapshot,
        fee_env: &payload.fee_env,
        config: &payload.config,
        user_ata_exists: payload.user_ata_exists,
    })
}

/// Settle one arrived twin order. `arrival_snapshot` is the live reducer state at arrival time
/// (None → DATA_STALE failure, which charges nothing because nothing landed).
pub async fn settle_twin_order(
    state: &AppState,
    order_id: Uuid,
    req: &PaperOrderRequest,
    payload: &TwinOrderPayload,
    arrival_snapshot: Option<&MarketSnapshot>,
) -> anyhow::Result<ExecutionResult> {
    let arrival_snapshot = arrival_snapshot.filter(|s| {
        let age_ms = (Utc::now() - s.as_of.knowledge_cutoff).num_milliseconds();
        (0..=ARRIVAL_STATE_MAX_STALENESS_MS).contains(&age_ms)
    });
    let result = execute_payload(payload, arrival_snapshot);
    persist_result(&state.stores, &result, Some(order_id)).await?;
    persist_landing_observation(&state.stores, payload, &result, arrival_snapshot).await?;
    let intent = &payload.intent;
    trace_event(
        &state.stores,
        intent.trace_id,
        "SIMULATION",
        Some(result.landing.arrival_slot.0 as i64),
        "result",
        &result.result_id.to_string(),
        intent,
        json!({
            "result": result,
            "fingerprint": result_fingerprint(&result),
            "arrival_snapshot": arrival_snapshot
        }),
    )
    .await?;
    let quote_mint = intent.quote_asset.to_string();
    if result.success {
        let Some(arrival) = arrival_snapshot else {
            anyhow::bail!("twin reported success without arrival state");
        };
        let quote = fill_quote_from_result(req, &result, arrival);
        paper_ledger::record_arrival(&state.stores, order_id, arrival, &quote).await?;
        let input_override = match req.side {
            TradeSide::Buy => Some(result.filled_quote as u128),
            TradeSide::Sell => None,
        };
        paper_ledger::settle_arrived_with(
            &state.stores,
            order_id,
            result.landing.landed_slot.unwrap_or(result.landing.arrival_slot).0 as i64,
            Some(TwinFill {
                result_id: result.result_id,
                simulator_version: SIMULATOR_VERSION,
                network_cost_lamports: result.network_cost_charged_lamports,
                quote_mint: quote_mint.clone(),
                input_override,
            }),
        )
        .await?;
        trace_event(
            &state.stores,
            intent.trace_id,
            "FILL",
            result.landing.landed_slot.map(|s| s.0 as i64),
            "order",
            &order_id.to_string(),
            intent,
            json!({
                "filled_quote": result.filled_quote,
                "filled_base": result.filled_base,
                "network_cost_charged_lamports": result.network_cost_charged_lamports,
                "protocol_fees": result.protocol_fees,
                "prices": result.prices
            }),
        )
        .await?;
    } else {
        let class = result.failure_class.unwrap_or(FailureClass::Unknown);
        let reason = format!("REJECTED_TWIN:{}", class.as_str());
        paper_ledger::reject_order_with_fee(
            &state.stores,
            order_id,
            &reason,
            result.network_cost_charged_lamports,
            &quote_mint,
        )
        .await?;
        trace_event(
            &state.stores,
            intent.trace_id,
            "FAILURE",
            Some(result.landing.arrival_slot.0 as i64),
            "order",
            &order_id.to_string(),
            intent,
            json!({
                "failure_class": class.as_str(),
                "detail": result.failure_detail,
                "network_cost_charged_lamports": result.network_cost_charged_lamports
            }),
        )
        .await?;
    }
    Ok(result)
}

fn fill_quote_from_result(
    req: &PaperOrderRequest,
    result: &ExecutionResult,
    arrival: &MarketSnapshot,
) -> PaperQuote {
    let quote_mint = match req.side {
        TradeSide::Buy => req.input.mint,
        TradeSide::Sell => req.output_mint,
    };
    let quote_fee = |kind: FeeKind, atomic: u64, charged_on_failure: bool, decimals: u8| {
        FeeComponent {
            kind,
            asset: AssetAmount {
                mint: quote_mint,
                atomic: Atomic(atomic as u128),
                decimals,
            },
            schedule_version: format!("{}:{}", SIMULATOR_VERSION, result.protocol_fees.fee_tier),
            charged_on_failure,
        }
    };
    let pf = &result.protocol_fees;
    let mut fee_components = vec![quote_fee(FeeKind::Protocol, pf.protocol_fee_quote, false, 9)];
    if pf.creator_fee_quote > 0 {
        fee_components.push(quote_fee(FeeKind::Creator, pf.creator_fee_quote, false, 9));
    }
    if pf.lp_fee_quote > 0 {
        fee_components.push(quote_fee(FeeKind::Lp, pf.lp_fee_quote, false, 9));
    }
    if pf.cashback_fee_quote > 0 {
        fee_components.push(quote_fee(FeeKind::Cashback, pf.cashback_fee_quote, false, 9));
    }
    let net = &result.network;
    fee_components.push(quote_fee(FeeKind::Network, net.base_fee_lamports, true, 9));
    if net.priority_fee_lamports > 0 {
        fee_components.push(quote_fee(FeeKind::Priority, net.priority_fee_lamports, true, 9));
    }
    if net.estimated_jito_tip_lamports > 0 {
        fee_components.push(quote_fee(FeeKind::Tip, net.estimated_jito_tip_lamports, true, 9));
    }
    if net.rent_lamports > 0 {
        fee_components.push(quote_fee(FeeKind::Rent, net.rent_lamports, false, 9));
    }
    let (expected, minimum, input) = match req.side {
        TradeSide::Buy => (
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(result.filled_base as u128),
                decimals: 6,
            },
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(result.minimum_output as u128),
                decimals: 6,
            },
            AssetAmount {
                mint: req.input.mint,
                atomic: Atomic(result.filled_quote as u128),
                decimals: 9,
            },
        ),
        TradeSide::Sell => (
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(result.filled_quote as u128),
                decimals: 9,
            },
            AssetAmount {
                mint: req.output_mint,
                atomic: Atomic(result.minimum_output as u128),
                decimals: 9,
            },
            req.input.clone(),
        ),
    };
    PaperQuote {
        quote_id: req.quote_id,
        market_snapshot_id: arrival.snapshot_id.clone(),
        input,
        expected_output: expected,
        minimum_output: minimum,
        reserve_input: req.input.clone(),
        fee_components,
        price_impact_bps: result.price_impact_bps,
        expires_at: Utc::now(),
        quality: arrival.quality.clone(),
    }
}

pub async fn persist_result(
    stores: &Stores,
    r: &ExecutionResult,
    order_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut tx = stores.postgres.begin().await?;
    let inserted = sqlx::query(
        r#"INSERT INTO pumphound.execution_results
           (result_id,intent_id,trace_id,order_id,simulator_version,transaction_kind,constructed,
            simulated,success,failure_class,failure_detail,attempted_slot,arrival_slot,landed_slot,
            landing_slots,arrival_delay_ms,landing_model_version,compute_unit_limit,
            compute_units_consumed,cu_price_micro_lamports,base_fee_lamports,priority_fee_lamports,
            jito_tip_lamports,rent_lamports,network_cost_charged_lamports,protocol_fee_quote,
            creator_fee_quote,lp_fee_quote,cashback_fee_quote,fee_tier,price_impact_bps,
            adverse_move_bps,requested_quote,filled_quote,filled_base,minimum_output,
            expected_output_at_decision,decision_price_e9,arrival_price_e9,execution_price_e9,
            post_cost_price_e9,decision_snapshot_id,arrival_snapshot_id,transaction_sha256,
            fingerprint,result)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,
                   $20::numeric,$21::numeric,$22::numeric,$23::numeric,$24::numeric,$25::numeric,
                   $26::numeric,$27::numeric,$28::numeric,$29::numeric,$30,$31,$32,$33::numeric,
                   $34::numeric,$35::numeric,$36::numeric,$37::numeric,$38::numeric,$39::numeric,
                   $40::numeric,$41::numeric,$42,$43,$44,$45,$46)
           ON CONFLICT (intent_id) DO NOTHING"#,
    )
    .bind(r.result_id)
    .bind(r.intent_id)
    .bind(r.trace_id)
    .bind(order_id)
    .bind(&r.simulator_version)
    .bind(r.transaction_kind.as_str())
    .bind(r.constructed)
    .bind(r.simulated)
    .bind(r.success)
    .bind(r.failure_class.map(|c| c.as_str()))
    .bind(&r.failure_detail)
    .bind(r.landing.attempted_slot.0 as i64)
    .bind(r.landing.arrival_slot.0 as i64)
    .bind(r.landing.landed_slot.map(|s| s.0 as i64))
    .bind(r.landing.landing_slots.map(|s| s as i32))
    .bind(r.landing.arrival_delay_ms as i64)
    .bind(&r.landing.landing_model_version)
    .bind(r.network.compute_unit_limit as i32)
    .bind(r.network.compute_units_consumed as i32)
    .bind(r.network.compute_unit_price_micro_lamports.to_string())
    .bind(r.network.base_fee_lamports.to_string())
    .bind(r.network.priority_fee_lamports.to_string())
    .bind(r.network.estimated_jito_tip_lamports.to_string())
    .bind(r.network.rent_lamports.to_string())
    .bind(r.network_cost_charged_lamports.to_string())
    .bind(r.protocol_fees.protocol_fee_quote.to_string())
    .bind(r.protocol_fees.creator_fee_quote.to_string())
    .bind(r.protocol_fees.lp_fee_quote.to_string())
    .bind(r.protocol_fees.cashback_fee_quote.to_string())
    .bind(&r.protocol_fees.fee_tier)
    .bind(r.price_impact_bps as i32)
    .bind(r.adverse_move_bps)
    .bind(r.requested_quote.to_string())
    .bind(r.filled_quote.to_string())
    .bind(r.filled_base.to_string())
    .bind(r.minimum_output.to_string())
    .bind(r.expected_output_at_decision.to_string())
    .bind(r.prices.decision_price_e9.to_string())
    .bind(r.prices.arrival_price_e9.to_string())
    .bind(r.prices.execution_price_e9.to_string())
    .bind(r.prices.post_cost_effective_price_e9.to_string())
    .bind(&r.decision_snapshot_id)
    .bind(&r.arrival_snapshot_id)
    .bind(&r.transaction_sha256)
    .bind(result_fingerprint(r))
    .bind(serde_json::to_value(r)?)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        tx.commit().await?;
        return Ok(());
    }
    let landed = r.landing.landed_slot.is_some();
    let components: Vec<(&str, u64, Option<i32>, bool, &str)> = vec![
        ("base_fee", r.network.base_fee_lamports, None, landed, "solana:base-fee"),
        ("priority_fee", r.network.priority_fee_lamports, None, landed, &r.network.priority_fee_provider),
        ("jito_tip", r.network.estimated_jito_tip_lamports, None, landed, &r.network.jito_model_version),
        ("rent", r.network.rent_lamports, None, r.success, "solana:rent-exempt-minimum"),
        ("protocol_fee", r.protocol_fees.protocol_fee_quote, Some(r.protocol_fees.protocol_fee_bps as i32), r.success, &r.protocol_fees.fee_tier),
        ("creator_fee", r.protocol_fees.creator_fee_quote, Some(r.protocol_fees.creator_fee_bps as i32), r.success, &r.protocol_fees.fee_tier),
        ("lp_fee", r.protocol_fees.lp_fee_quote, Some(r.protocol_fees.lp_fee_bps as i32), r.success, &r.protocol_fees.fee_tier),
        ("cashback_fee", r.protocol_fees.cashback_fee_quote, Some(r.protocol_fees.cashback_fee_bps as i32), r.success, &r.protocol_fees.fee_tier),
    ];
    for (component, atomic, bps, charged, source) in components {
        if atomic == 0 {
            continue;
        }
        sqlx::query(
            r#"INSERT INTO pumphound.execution_cost_components
               (component_id,result_id,trace_id,component,quote_atomic,bps,charged,source)
               VALUES ($1,$2,$3,$4,$5::numeric,$6,$7,$8)"#,
        )
        .bind(Uuid::now_v7())
        .bind(r.result_id)
        .bind(r.trace_id)
        .bind(component)
        .bind(atomic.to_string())
        .bind(bps)
        .bind(charged)
        .bind(source)
        .execute(&mut *tx)
        .await?;
    }
    // Impact and adverse selection are bps observations (no fixed quote amount is charged); keep
    // them as rows so attribution can sum them per book.
    for (component, bps) in [
        ("price_impact", r.price_impact_bps as i32),
        ("adverse_selection", r.adverse_move_bps),
    ] {
        sqlx::query(
            r#"INSERT INTO pumphound.execution_cost_components
               (component_id,result_id,trace_id,component,quote_atomic,bps,charged,source)
               VALUES ($1,$2,$3,$4,0,$5,$6,$7)"#,
        )
        .bind(Uuid::now_v7())
        .bind(r.result_id)
        .bind(r.trace_id)
        .bind(component)
        .bind(bps)
        .bind(r.success)
        .bind(SIMULATOR_VERSION)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn persist_landing_observation(
    stores: &Stores,
    payload: &TwinOrderPayload,
    result: &ExecutionResult,
    arrival: Option<&MarketSnapshot>,
) -> anyhow::Result<()> {
    let observed_price = arrival.and_then(|s| {
        pumphound_execution_twin::pump_math::mid_price_e9(
            s.effective_quote_atomic.0,
            s.effective_base_atomic.0,
        )
    });
    let observed_delay_ms = arrival.map(|s| {
        (s.as_of.knowledge_cutoff - payload.intent.decision_timestamp)
            .num_milliseconds()
            .max(0)
    });
    sqlx::query(
        r#"INSERT INTO pumphound.landing_observations
           (observation_id,trace_id,intent_id,result_id,decision_slot,decision_at,
            predicted_landing_slots,predicted_arrival_delay_ms,predicted_arrival_price_e9,
            observed_arrival_slot,observed_arrival_at,observed_arrival_price_e9,
            observed_state_delay_ms,congestion,cu_price_p50,landing_model_version)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9::numeric,$10,$11,$12::numeric,$13,$14,$15::numeric,$16)"#,
    )
    .bind(Uuid::now_v7())
    .bind(result.trace_id)
    .bind(result.intent_id)
    .bind(result.result_id)
    .bind(payload.intent.decision_slot.0 as i64)
    .bind(payload.intent.decision_timestamp)
    .bind(result.landing.landing_slots.map(|s| s as i32))
    .bind(result.landing.arrival_delay_ms as i64)
    .bind(Some(result.prices.decision_price_e9.to_string()))
    .bind(arrival.map(|s| s.as_of.slot.0 as i64))
    .bind(arrival.map(|s| s.as_of.knowledge_cutoff))
    .bind(observed_price.map(|p| p.to_string()))
    .bind(observed_delay_ms)
    .bind(payload.fee_env.congestion.as_str())
    .bind(payload.fee_env.cu_price_micro_lamports.p50.to_string())
    .bind(LANDING_MODEL_VERSION)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}
