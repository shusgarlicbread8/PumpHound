//! EXECUTION-EDGE-V1 experiment family: books that trade through the high-fidelity Execution
//! Twin instead of the legacy paper simulator. Frozen ALPHA-LEARN books are untouched; the
//! RANDOM_EXECUTION_CONTROL book deliberately reuses the RANDOM_CONTROL selection (same seed,
//! same rate, same fixed size, same hold) so the two books form a matched pair whose ONLY
//! difference is the execution simulator. That pair answers: "does the legacy headline survive
//! realistic execution?"

use crate::{OpportunityAction, OpportunityDecision, StrategyBook};
use serde_json::Value;

pub const EXECUTION_EDGE_EXPERIMENT_ID: &str = "EXECUTION-EDGE-V1";
pub const STRATEGY_RANDOM_EXECUTION_CONTROL_V1: &str = "RANDOM_EXECUTION_CONTROL_V1";
pub const STRATEGY_MICROSTRUCTURE_BASELINE_V1: &str = "MICROSTRUCTURE_BASELINE_V1";

pub const SIMULATOR_LEGACY: &str = "legacy-paper-v1";
/// Must equal `pumphound_execution_twin::SIMULATOR_VERSION`; asserted in core-api tests.
pub const SIMULATOR_EXECUTION_TWIN: &str = "execution-twin-v1.0.0";

const EXECUTION_EDGE_CAPITAL_ATOMIC: i128 = 2_000_000_000;

pub const RANDOM_EXECUTION_CONTROL_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_RANDOM_EXECUTION_CONTROL_V1,
    desk_name: "EXECUTION-EDGE RANDOM CONTROL",
    display_name: "RANDOM (TWIN)",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "NONE",
    execution_authority: "DETERMINISTIC_SAMPLE",
    ai_status: "NONE",
    capital_sol_atomic: EXECUTION_EDGE_CAPITAL_ATOMIC,
};

pub const MICROSTRUCTURE_BASELINE_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_MICROSTRUCTURE_BASELINE_V1,
    desk_name: "EXECUTION-EDGE MICROSTRUCTURE",
    display_name: "MICROSTRUCTURE (TWIN)",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "NONE",
    execution_authority: "MICROSTRUCTURE_RULE_V1",
    ai_status: "NONE",
    capital_sol_atomic: EXECUTION_EDGE_CAPITAL_ATOMIC,
};

pub fn execution_edge_desks() -> [StrategyBook; 2] {
    [RANDOM_EXECUTION_CONTROL_BOOK, MICROSTRUCTURE_BASELINE_BOOK]
}

/// Which simulator a book's ledger is produced by. Legacy books stay legacy forever.
pub fn book_simulator(strategy_key: &str) -> &'static str {
    match strategy_key {
        STRATEGY_RANDOM_EXECUTION_CONTROL_V1 | STRATEGY_MICROSTRUCTURE_BASELINE_V1 => {
            SIMULATOR_EXECUTION_TWIN
        }
        _ => SIMULATOR_LEGACY,
    }
}

pub fn uses_execution_twin(strategy_key: &str) -> bool {
    book_simulator(strategy_key) == SIMULATOR_EXECUTION_TWIN
}

/// Transparent market/curve-only entry rule (no ML, no wallet, no graph, no AI). Thresholds are
/// deliberately simple and versioned in the strategy key; changing them means a new key.
pub const MICROSTRUCTURE_MIN_UNIQUE_BUYERS_30: f64 = 6.0;
pub const MICROSTRUCTURE_MIN_IMBALANCE_30: f64 = 0.2;
pub const MICROSTRUCTURE_MIN_RETURN_30: f64 = 0.0;
pub const MICROSTRUCTURE_MAX_SELL_COUNT_30: f64 = 12.0;

fn feature(values: &serde_json::Map<String, Value>, name: &str) -> Option<f64> {
    values.get(name).and_then(Value::as_f64).filter(|v| v.is_finite())
}

/// Requires the technical gate (age, liquidity, impact, cost, exposure) to have passed first.
pub fn evaluate_microstructure_baseline(
    technical: &OpportunityDecision,
    values: &serde_json::Map<String, Value>,
) -> OpportunityDecision {
    if technical.action != OpportunityAction::Buy {
        return technical.clone();
    }
    let (Some(buyers), Some(imbalance), Some(ret), Some(sells)) = (
        feature(values, "unique_buyers_30"),
        feature(values, "buy_sell_imbalance_30"),
        feature(values, "return_30"),
        feature(values, "sell_count_30"),
    ) else {
        return OpportunityDecision {
            action: OpportunityAction::Abstain,
            reasons: vec!["microstructure_features_unavailable".into()],
        };
    };
    let mut reasons = Vec::new();
    if buyers < MICROSTRUCTURE_MIN_UNIQUE_BUYERS_30 {
        reasons.push("insufficient_unique_buyers_30".to_string());
    }
    if imbalance < MICROSTRUCTURE_MIN_IMBALANCE_30 {
        reasons.push("buy_sell_imbalance_30_weak".to_string());
    }
    if ret <= MICROSTRUCTURE_MIN_RETURN_30 {
        reasons.push("return_30_non_positive".to_string());
    }
    if sells > MICROSTRUCTURE_MAX_SELL_COUNT_30 {
        reasons.push("sell_pressure_30_high".to_string());
    }
    if reasons.is_empty() {
        OpportunityDecision {
            action: OpportunityAction::Buy,
            reasons: vec!["microstructure_rule_v1_passed".into()],
        }
    } else {
        OpportunityDecision {
            action: OpportunityAction::Watch,
            reasons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn buy() -> OpportunityDecision {
        OpportunityDecision {
            action: OpportunityAction::Buy,
            reasons: vec![],
        }
    }

    #[test]
    fn legacy_books_never_use_the_twin() {
        assert_eq!(book_simulator(crate::STRATEGY_RANDOM_CONTROL_V1), SIMULATOR_LEGACY);
        assert!(uses_execution_twin(STRATEGY_RANDOM_EXECUTION_CONTROL_V1));
    }

    #[test]
    fn microstructure_rule_requires_all_features() {
        let mut m = serde_json::Map::new();
        m.insert("unique_buyers_30".into(), json!(10.0));
        let d = evaluate_microstructure_baseline(&buy(), &m);
        assert_eq!(d.action, OpportunityAction::Abstain);
        m.insert("buy_sell_imbalance_30".into(), json!(0.5));
        m.insert("return_30".into(), json!(0.05));
        m.insert("sell_count_30".into(), json!(2.0));
        assert_eq!(evaluate_microstructure_baseline(&buy(), &m).action, OpportunityAction::Buy);
        m.insert("sell_count_30".into(), json!(40.0));
        assert_eq!(evaluate_microstructure_baseline(&buy(), &m).action, OpportunityAction::Watch);
    }
}
