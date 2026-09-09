//! ExecutionTwin: immutable `ExecutionIntent` + decision/arrival market state → `ExecutionResult`.
//!
//! Pipeline (all pure, deterministic for a given input set):
//!   decision quote (exact) → tx build → network cost decomposition → landing sample
//!   → arrival-state exact quote → slippage / output check → success or classified failure
//!   → fee semantics (landed-but-failed still pays base+priority(+tip); never-landed pays nothing)
//!
//! The ledger fill ALWAYS comes from the arrival-state quote. The decision quote is recorded
//! only to measure adverse selection.

use crate::builder::{self, BuiltTransaction, MarketIdentity, SwapArgs};
use crate::contract::*;
use crate::landing::{seed_from, LandingModel};
use crate::network::{self, ComputeBudgetModel, FeeEnvironment, PriorityFeePolicy};
use crate::quote::{exact_quote, ExactQuote, QuoteRejection};
use pumphound_domain::{MarketSnapshot, TradeSide};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

/// Frozen per-book execution configuration. Part of the book manifest; books in one experiment
/// family must share it so their economics are comparable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwinConfig {
    pub priority_fee_policy: PriorityFeePolicy,
    pub jito_scenario: JitoScenario,
    pub compute: ComputeBudgetModel,
    pub landing: LandingModel,
}

impl Default for TwinConfig {
    fn default() -> Self {
        TwinConfig {
            priority_fee_policy: PriorityFeePolicy::P50,
            jito_scenario: JitoScenario::NoJito,
            compute: ComputeBudgetModel::default(),
            landing: LandingModel::default(),
        }
    }
}

/// Everything the twin needs besides the intent. `arrival_snapshot` is the market state at the
/// sampled landing time; the caller (paper loop) obtains it from the live reducer, replay from
/// the archive.
pub struct ExecutionInputs<'a> {
    pub intent: &'a ExecutionIntent,
    pub identity: &'a MarketIdentity,
    pub decision_snapshot: &'a MarketSnapshot,
    pub arrival_snapshot: Option<&'a MarketSnapshot>,
    pub fee_env: &'a FeeEnvironment,
    pub config: &'a TwinConfig,
    /// Whether the synthetic identity already holds an ATA for this mint (first buy creates it).
    pub user_ata_exists: bool,
}

/// Landing decision + arrival timing, produced BEFORE the arrival snapshot is known so the
/// caller can schedule the arrival lookup. Deterministic for a trace.
pub fn plan_landing(
    intent: &ExecutionIntent,
    fee_env: &FeeEnvironment,
    config: &TwinConfig,
) -> (LandingSample, Option<FailureClass>) {
    let seed = seed_from(&[
        intent.trace_id.as_bytes(),
        intent.intent_id.as_bytes(),
        SIMULATOR_VERSION.as_bytes(),
    ]);
    config.landing.sample(
        seed,
        intent.decision_timestamp,
        intent.decision_slot,
        fee_env.congestion,
        intent.cost_scenario,
    )
}

fn side_amount(intent: &ExecutionIntent) -> u64 {
    match intent.side {
        TradeSide::Buy => intent.requested_quote_amount,
        TradeSide::Sell => intent.token_amount_if_sell,
    }
}

fn swap_args(
    q: &ExactQuote,
    intent: &ExecutionIntent,
    net: &NetworkCosts,
    create_ata: bool,
) -> SwapArgs {
    let (amount, limit) = match q.transaction_kind {
        // curve `buy(amount=tokens, max_sol_cost=budget)`
        TransactionKind::PumpBuyExactSolIn => (q.filled_base, intent.requested_quote_amount),
        TransactionKind::PumpSell => (q.filled_base, q.minimum_output),
        // pumpswap `buy_exact_quote_in(spendable_quote_in=budget, min_base_amount_out)`
        TransactionKind::PumpSwapBuyExactQuoteIn => {
            (intent.requested_quote_amount, q.minimum_output)
        }
        TransactionKind::PumpSwapSell => (q.filled_base, q.minimum_output),
    };
    SwapArgs {
        kind: q.transaction_kind,
        amount,
        limit,
        compute_unit_limit: net.compute_unit_limit,
        compute_unit_price_micro_lamports: net.compute_unit_price_micro_lamports,
        create_user_ata: create_ata,
    }
}

fn adverse_bps(side: TradeSide, decision_mid: u128, arrival_mid: u128) -> i32 {
    // Positive = market moved against the order between decision and arrival.
    let m = crate::pump_math::relative_bps(decision_mid, arrival_mid);
    match side {
        TradeSide::Buy => m,
        TradeSide::Sell => -m,
    }
}

fn failed(
    intent: &ExecutionIntent,
    kind: TransactionKind,
    landing: LandingSample,
    class: FailureClass,
    detail: String,
    decision: Option<&ExactQuote>,
    built: Option<&BuiltTransaction>,
    net: NetworkCosts,
    arrival_snapshot_id: &str,
    arrival_mid: Option<u128>,
    extra: serde_json::Value,
) -> ExecutionResult {
    let charged = if class.charges_network_fee() {
        net.landed_cost_lamports()
    } else {
        0
    };
    let decision_price = decision.map(|d| d.mid_price_before_e9).unwrap_or(0);
    let arrival_price = arrival_mid.unwrap_or(0);
    ExecutionResult {
        result_id: Uuid::now_v7(),
        trace_id: intent.trace_id,
        intent_id: intent.intent_id,
        simulator_version: SIMULATOR_VERSION.into(),
        transaction_kind: kind,
        constructed: built.is_some(),
        simulated: decision.is_some(),
        landing,
        success: false,
        failure_class: Some(class),
        failure_detail: Some(detail),
        protocol_fees: ProtocolFees::default(),
        network: net,
        network_cost_charged_lamports: charged,
        price_impact_bps: 0,
        adverse_move_bps: if decision_price > 0 && arrival_price > 0 {
            adverse_bps(intent.side, decision_price, arrival_price)
        } else {
            0
        },
        requested_quote: side_amount(intent),
        filled_quote: 0,
        filled_base: 0,
        minimum_output: decision.map(|d| d.minimum_output).unwrap_or(0),
        expected_output_at_decision: decision
            .map(|d| match intent.side {
                TradeSide::Buy => d.filled_base,
                TradeSide::Sell => d.filled_quote,
            })
            .unwrap_or(0),
        prices: PriceTriple {
            decision_price_e9: decision_price,
            arrival_price_e9: arrival_price,
            execution_price_e9: 0,
            post_cost_effective_price_e9: 0,
        },
        decision_snapshot_id: intent.market_snapshot_id.clone(),
        arrival_snapshot_id: arrival_snapshot_id.to_string(),
        transaction_sha256: built.map(|b| b.message_sha256.clone()).unwrap_or_default(),
        evidence: json!({
            "upstream": {
                "pump_public_docs_commit": UPSTREAM_PUMP_PUBLIC_DOCS_COMMIT,
                "pump_sdk": UPSTREAM_PUMP_SDK_VERSION,
                "pump_swap_sdk": UPSTREAM_PUMP_SWAP_SDK_VERSION,
            },
            "decision_quote": decision,
            "built_transaction": built.map(|b| json!({
                "builder_version": b.builder_version,
                "fee_payer": b.fee_payer,
                "synthetic_identity": b.synthetic_identity,
                "instructions": b.instructions.iter().map(|i| json!({"program": i.program_id, "name": i.name, "accounts": i.accounts.len()})).collect::<Vec<_>>(),
                "creates_ata": b.creates_ata,
            })),
            "extra": extra,
            "paper_only": true,
            "live_money": false,
        }),
    }
}

/// Execute one intent. Never panics on market data; every rejection is a classified failure.
pub fn execute(inputs: ExecutionInputs<'_>) -> ExecutionResult {
    let intent = inputs.intent;
    let cfg = inputs.config;
    let amount = side_amount(intent);
    let kind_guess = match (intent.venue, intent.side) {
        (pumphound_domain::Protocol::PumpCurve, TradeSide::Buy) => {
            TransactionKind::PumpBuyExactSolIn
        }
        (pumphound_domain::Protocol::PumpCurve, TradeSide::Sell) => TransactionKind::PumpSell,
        (pumphound_domain::Protocol::PumpSwap, TradeSide::Buy) => {
            TransactionKind::PumpSwapBuyExactQuoteIn
        }
        (pumphound_domain::Protocol::PumpSwap, TradeSide::Sell) => TransactionKind::PumpSwapSell,
    };
    let create_ata = intent.side == TradeSide::Buy && !inputs.user_ata_exists;
    let (landing, landing_failure) = plan_landing(intent, inputs.fee_env, cfg);
    let net = network::network_costs(
        kind_guess,
        create_ata,
        &cfg.compute,
        cfg.priority_fee_policy,
        intent.cost_scenario,
        inputs.fee_env,
    );

    // 1. decision quote (what the strategy believed it would get)
    let decision = match exact_quote(
        inputs.decision_snapshot,
        intent.side,
        amount,
        intent.slippage_limit_bps,
    ) {
        Ok(q) => q,
        Err(QuoteRejection { class, detail }) => {
            return failed(
                intent,
                kind_guess,
                landing,
                class,
                detail,
                None,
                None,
                net,
                "",
                None,
                json!({"stage": "decision_quote"}),
            );
        }
    };
    // 2. transaction construction
    let built = match builder::build(
        inputs.identity,
        &swap_args(&decision, intent, &net, create_ata),
        intent.decision_slot,
    ) {
        Ok(b) => b,
        Err(e) => {
            return failed(
                intent,
                decision.transaction_kind,
                landing,
                FailureClass::SimulationFailure,
                format!("tx build: {e}"),
                Some(&decision),
                None,
                net,
                "",
                None,
                json!({"stage": "build"}),
            );
        }
    };
    // 3. landing
    if let Some(class) = landing_failure {
        return failed(
            intent,
            decision.transaction_kind,
            landing,
            class,
            "transaction never landed within the blockhash window".into(),
            Some(&decision),
            Some(&built),
            net,
            "",
            None,
            json!({"stage": "landing"}),
        );
    }
    let Some(arrival) = inputs.arrival_snapshot else {
        return failed(
            intent,
            decision.transaction_kind,
            landing,
            FailureClass::DataStale,
            "no market state available at arrival".into(),
            Some(&decision),
            Some(&built),
            net,
            "",
            None,
            json!({"stage": "arrival_state"}),
        );
    };
    // 4. arrival-state quote — the only quote the ledger may use
    let arrival_mid = crate::pump_math::mid_price_e9(
        arrival.effective_quote_atomic.0,
        arrival.effective_base_atomic.0,
    );
    let arrival_q = match exact_quote(arrival, intent.side, amount, intent.slippage_limit_bps) {
        Ok(q) => q,
        Err(QuoteRejection { class, detail }) => {
            // The tx landed but the program would have rejected it (state changed under us).
            let class = match class {
                FailureClass::UnsupportedMarketState => FailureClass::AccountStateChanged,
                other => other,
            };
            return failed(
                intent,
                decision.transaction_kind,
                landing,
                class,
                detail,
                Some(&decision),
                Some(&built),
                net,
                &arrival.snapshot_id,
                arrival_mid,
                json!({"stage": "arrival_quote"}),
            );
        }
    };
    if arrival_q.transaction_kind != decision.transaction_kind {
        return failed(
            intent,
            decision.transaction_kind,
            landing,
            FailureClass::AccountStateChanged,
            "venue changed between decision and arrival (migration)".into(),
            Some(&decision),
            Some(&built),
            net,
            &arrival.snapshot_id,
            arrival_mid,
            json!({"stage": "arrival_quote"}),
        );
    }
    // 5. on-chain limit check against the DECISION-time minimum encoded in the instruction
    let arrival_output = match intent.side {
        TradeSide::Buy => arrival_q.filled_base,
        TradeSide::Sell => arrival_q.filled_quote,
    };
    if arrival_output < decision.minimum_output {
        let detail = format!(
            "arrival output {} < instruction minimum {}",
            arrival_output, decision.minimum_output
        );
        return failed(
            intent,
            decision.transaction_kind,
            landing,
            FailureClass::SlippageExceeded,
            detail,
            Some(&decision),
            Some(&built),
            net,
            &arrival.snapshot_id,
            arrival_mid,
            json!({"stage": "slippage_check", "arrival_quote": arrival_q}),
        );
    }
    if net.compute_units_consumed > net.compute_unit_limit {
        return failed(
            intent,
            decision.transaction_kind,
            landing,
            FailureClass::ComputeExceeded,
            "estimated CU exceeds requested limit".into(),
            Some(&decision),
            Some(&built),
            net,
            &arrival.snapshot_id,
            arrival_mid,
            json!({"stage": "compute"}),
        );
    }
    // 6. success: fill from ARRIVAL state
    let charged = net.total_success_lamports();
    let (filled_quote, filled_base) = (arrival_q.filled_quote, arrival_q.filled_base);
    // post-cost effective price: for buys total quote incl. network; for sells net quote minus network.
    let post_cost_quote: u128 = match intent.side {
        TradeSide::Buy => filled_quote as u128 + charged as u128,
        TradeSide::Sell => (filled_quote as u128).saturating_sub(charged as u128),
    };
    let post_cost_price =
        crate::pump_math::execution_price_e9(post_cost_quote, filled_base as u128).unwrap_or(0);
    ExecutionResult {
        result_id: Uuid::now_v7(),
        trace_id: intent.trace_id,
        intent_id: intent.intent_id,
        simulator_version: SIMULATOR_VERSION.into(),
        transaction_kind: arrival_q.transaction_kind,
        constructed: true,
        simulated: true,
        landing,
        success: true,
        failure_class: None,
        failure_detail: None,
        protocol_fees: arrival_q.protocol_fees.clone(),
        network: net,
        network_cost_charged_lamports: charged,
        price_impact_bps: arrival_q.price_impact_bps,
        adverse_move_bps: adverse_bps(
            intent.side,
            decision.mid_price_before_e9,
            arrival_q.mid_price_before_e9,
        ),
        requested_quote: amount,
        filled_quote,
        filled_base,
        minimum_output: decision.minimum_output,
        expected_output_at_decision: match intent.side {
            TradeSide::Buy => decision.filled_base,
            TradeSide::Sell => decision.filled_quote,
        },
        prices: PriceTriple {
            decision_price_e9: decision.mid_price_before_e9,
            arrival_price_e9: arrival_q.mid_price_before_e9,
            execution_price_e9: arrival_q.execution_price_e9,
            post_cost_effective_price_e9: post_cost_price,
        },
        decision_snapshot_id: intent.market_snapshot_id.clone(),
        arrival_snapshot_id: arrival.snapshot_id.clone(),
        transaction_sha256: built.message_sha256.clone(),
        evidence: json!({
            "upstream": {
                "pump_public_docs_commit": UPSTREAM_PUMP_PUBLIC_DOCS_COMMIT,
                "pump_sdk": UPSTREAM_PUMP_SDK_VERSION,
                "pump_swap_sdk": UPSTREAM_PUMP_SWAP_SDK_VERSION,
                "fee_snapshot": crate::fee_config::FEE_SNAPSHOT_VERSION,
            },
            "decision_quote": decision,
            "arrival_quote": arrival_q,
            "fee_environment": inputs.fee_env,
            "built_transaction": {
                "builder_version": built.builder_version,
                "fee_payer": built.fee_payer,
                "synthetic_identity": true,
                "recent_blockhash_placeholder": built.recent_blockhash_placeholder,
                "instructions": built.instructions.iter().map(|i| json!({"program": i.program_id, "name": i.name, "accounts": i.accounts, "data_hex": i.data_hex})).collect::<Vec<_>>(),
                "message_hex": built.message_hex,
                "creates_ata": built.creates_ata,
            },
            "paper_only": true,
            "live_money": false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pda::{self, pk};
    use chrono::Utc;
    use pumphound_domain::*;

    fn curve_snapshot(id: &str, slot: u64, vq: u128, vt: u128) -> MarketSnapshot {
        let mint = pk("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        MarketSnapshot {
            snapshot_id: id.into(),
            market: pda::pump::bonding_curve(&mint),
            mint,
            quote_mint: pk(WSOL_MINT),
            protocol: Protocol::PumpCurve,
            as_of: ObservationPoint {
                slot: Slot(slot),
                tx_index: None,
                event_index: None,
                event_time: Some(now_utc()),
                observed_at: now_utc(),
                knowledge_cutoff: now_utc(),
                commitment: Commitment::Confirmed,
            },
            canonicality: Canonicality::Provisional,
            virtual_base_atomic: Atomic(vt),
            virtual_quote_atomic: Atomic(vq),
            real_base_atomic: Atomic(vt - 280_000_000_000_000),
            real_quote_atomic: Atomic(vq - 30_000_000_000),
            effective_base_atomic: Atomic(vt),
            effective_quote_atomic: Atomic(vq),
            complete: false,
            migrated_market: None,
            config_snapshot_id: "test".into(),
            quality: DataQuality::live(0, 0, true),
            is_mayhem_mode: Some(false),
            creator_fee_applies: Some(true),
            base_supply_atomic: None,
            cashback_fee_bps: Some(0),
        }
    }

    fn intent(side: TradeSide, snap: &MarketSnapshot, seed_variant: u128) -> ExecutionIntent {
        ExecutionIntent {
            trace_id: Uuid::from_u128(0xA000 + seed_variant),
            intent_id: Uuid::from_u128(0xB000 + seed_variant),
            experiment_id: "EXECUTION-EDGE-V1".into(),
            strategy_version: "test".into(),
            decision_id: Uuid::from_u128(1),
            decision_timestamp: Utc::now(),
            decision_slot: snap.as_of.slot,
            mint: snap.mint,
            market: snap.market,
            venue: snap.protocol,
            side,
            quote_asset: snap.quote_mint,
            requested_quote_amount: if side == TradeSide::Buy {
                50_000_000
            } else {
                0
            },
            token_amount_if_sell: if side == TradeSide::Sell {
                1_000_000_000_000
            } else {
                0
            },
            slippage_limit_bps: 300,
            strategy_model_versions: vec![],
            market_snapshot_id: snap.snapshot_id.clone(),
            cost_scenario: JitoScenario::NoJito,
        }
    }

    fn identity(snap: &MarketSnapshot) -> MarketIdentity {
        MarketIdentity {
            mint: snap.mint,
            market: snap.market,
            quote_mint: snap.quote_mint,
            creator: Some(pk("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")),
            base_token_program: pk(pda::TOKEN_PROGRAM),
        }
    }

    fn run(
        decision: &MarketSnapshot,
        arrival: Option<&MarketSnapshot>,
        side: TradeSide,
        variant: u128,
    ) -> ExecutionResult {
        let it = intent(side, decision, variant);
        let id = identity(decision);
        let env = FeeEnvironment::fallback(Utc::now());
        let cfg = TwinConfig::default();
        execute(ExecutionInputs {
            intent: &it,
            identity: &id,
            decision_snapshot: decision,
            arrival_snapshot: arrival,
            fee_env: &env,
            config: &cfg,
            user_ata_exists: false,
        })
    }

    fn first_landing_seed(side: TradeSide, decision: &MarketSnapshot) -> u128 {
        // find a variant whose landing sample lands (so tests exercise the fill path)
        for v in 0..64u128 {
            let it = intent(side, decision, v);
            let env = FeeEnvironment::fallback(Utc::now());
            if plan_landing(&it, &env, &TwinConfig::default()).1.is_none() {
                return v;
            }
        }
        panic!("no landing variant")
    }

    #[test]
    fn fill_uses_arrival_state_not_decision_state() {
        let decision = curve_snapshot("d", 100, 40_000_000_000, 1_000_000_000_000_000);
        // price moved up ~1% before arrival
        let arrival = curve_snapshot("a", 102, 40_400_000_000, 990_000_000_000_000);
        let v = first_landing_seed(TradeSide::Buy, &decision);
        let r = run(&decision, Some(&arrival), TradeSide::Buy, v);
        assert!(r.success, "{:?} {:?}", r.failure_class, r.failure_detail);
        assert!(
            r.filled_base < r.expected_output_at_decision,
            "arrival fill must be worse than decision expectation"
        );
        assert!(r.adverse_move_bps > 0);
        assert_eq!(r.arrival_snapshot_id, "a");
        assert!(r.network_cost_charged_lamports >= 5_000);
        assert!(r.protocol_fees.protocol_fee_quote > 0);
        assert!(r.prices.post_cost_effective_price_e9 > r.prices.execution_price_e9);
    }

    #[test]
    fn slippage_beyond_cap_is_rejected_and_still_pays_network_fee() {
        let decision = curve_snapshot("d", 100, 40_000_000_000, 1_000_000_000_000_000);
        // 10% adverse move > 3% cap
        let arrival = curve_snapshot("a", 102, 44_000_000_000, 900_000_000_000_000);
        let v = first_landing_seed(TradeSide::Buy, &decision);
        let r = run(&decision, Some(&arrival), TradeSide::Buy, v);
        assert!(!r.success);
        assert_eq!(r.failure_class, Some(FailureClass::SlippageExceeded));
        assert_eq!(r.filled_base, 0);
        assert_eq!(
            r.protocol_fees.total_quote(),
            0,
            "no swap => no protocol fees"
        );
        assert_eq!(
            r.network_cost_charged_lamports,
            r.network.landed_cost_lamports()
        );
        assert!(r.network_cost_charged_lamports > 0);
    }

    #[test]
    fn never_landed_transactions_pay_nothing() {
        let decision = curve_snapshot("d", 100, 40_000_000_000, 1_000_000_000_000_000);
        let mut found = false;
        for v in 0..200u128 {
            let r = run(&decision, Some(&decision), TradeSide::Buy, v);
            if r.failure_class == Some(FailureClass::LandingTimeout) {
                assert_eq!(r.network_cost_charged_lamports, 0);
                assert_eq!(r.filled_base, 0);
                assert!(r.constructed);
                found = true;
                break;
            }
        }
        assert!(found, "prior must produce some landing timeouts");
    }

    /// Replay contract: the paper loop stores intent/identity/snapshots/config/fee_env as JSON;
    /// replay deserializes them and must reproduce the original fingerprint byte-for-byte.
    #[test]
    fn replay_from_serialized_inputs_reproduces_fingerprint() {
        let decision = curve_snapshot("d", 100, 30_000_000_000, 1_073_000_000_000_000);
        let arrival = curve_snapshot("a", 101, 30_400_000_000, 1_060_000_000_000_000);
        let v = first_landing_seed(TradeSide::Buy, &decision);
        let it = intent(TradeSide::Buy, &decision, v);
        let id = identity(&decision);
        let env = FeeEnvironment::fallback(Utc::now());
        let cfg = TwinConfig::default();
        let live = execute(ExecutionInputs {
            intent: &it,
            identity: &id,
            decision_snapshot: &decision,
            arrival_snapshot: Some(&arrival),
            fee_env: &env,
            config: &cfg,
            user_ata_exists: false,
        });
        let stored = json!({
            "intent": it, "identity": id, "decision": decision, "arrival": arrival,
            "fee_env": env, "config": cfg
        });
        let text = stored.to_string();
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();
        let it2: ExecutionIntent = serde_json::from_value(back["intent"].clone()).unwrap();
        let id2: MarketIdentity = serde_json::from_value(back["identity"].clone()).unwrap();
        let d2: MarketSnapshot = serde_json::from_value(back["decision"].clone()).unwrap();
        let a2: MarketSnapshot = serde_json::from_value(back["arrival"].clone()).unwrap();
        let env2: FeeEnvironment = serde_json::from_value(back["fee_env"].clone()).unwrap();
        let cfg2: TwinConfig = serde_json::from_value(back["config"].clone()).unwrap();
        let replayed = execute(ExecutionInputs {
            intent: &it2,
            identity: &id2,
            decision_snapshot: &d2,
            arrival_snapshot: Some(&a2),
            fee_env: &env2,
            config: &cfg2,
            user_ata_exists: false,
        });
        assert!(live.success, "{:?}", live.failure_detail);
        assert_eq!(result_fingerprint(&live), result_fingerprint(&replayed));
        assert_eq!(live.filled_base, replayed.filled_base);
        assert_eq!(live.transaction_sha256, replayed.transaction_sha256);
    }

    #[test]
    fn results_are_deterministic_for_same_inputs() {
        let decision = curve_snapshot("d", 100, 40_000_000_000, 1_000_000_000_000_000);
        let arrival = curve_snapshot("a", 101, 40_100_000_000, 998_000_000_000_000);
        let a = run(&decision, Some(&arrival), TradeSide::Buy, 3);
        let b = run(&decision, Some(&arrival), TradeSide::Buy, 3);
        assert_eq!(result_fingerprint(&a), result_fingerprint(&b));
    }

    #[test]
    fn sell_fill_is_net_of_all_protocol_fees() {
        let decision = curve_snapshot("d", 100, 40_000_000_000, 1_000_000_000_000_000);
        let v = first_landing_seed(TradeSide::Sell, &decision);
        let r = run(&decision, Some(&decision), TradeSide::Sell, v);
        assert!(r.success, "{:?}", r.failure_detail);
        let q = &r.evidence["arrival_quote"];
        let gross = q["swap_quote"].as_u64().unwrap();
        assert_eq!(gross - r.protocol_fees.total_quote(), r.filled_quote);
        assert_eq!(r.adverse_move_bps, 0);
    }
}
