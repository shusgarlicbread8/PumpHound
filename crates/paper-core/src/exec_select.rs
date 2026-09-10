//! EXEC-SELECT-V1: picker / survivor experiment family.
//!
//! Frozen ALPHA-LEARN, ALPHA-LOOP and EXECUTION-EDGE-V1 books are untouched. Every book in this
//! family uses the Execution Twin, the same capital, the same 0.5 SOL entry, the same hold, the
//! same liquidity/impact envelope, and no confidence sizing.
//!
//! Timing is a first-class, labelled difference:
//! - CONFIRMED_EARLY (20–90s) is the matched ablation group.
//! - FAST_SCOUT (5–20s) and SURVIVOR (120–600s) share economics but not the observation window,
//!   so Strategy Diff marks those pairs CONFOUNDED on timing.

use crate::{
    evaluate_information_value_explorer, evaluate_microstructure_baseline, evaluate_random_control,
    InformationValueScore, LearningPolicyV1, OpportunityAction, OpportunityDecision, StrategyBook,
    EXECUTION_EDGE_CAPITAL_ATOMIC, EXECUTION_EDGE_ENTRY_ATOMIC, SIMULATOR_EXECUTION_TWIN,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EXEC_SELECT_EXPERIMENT_ID: &str = "EXEC-SELECT-V1";
pub const STRATEGY_RANDOM_SELECT_V1: &str = "RANDOM_SELECT_V1";
pub const STRATEGY_MARKET_CONFIRMED_V1: &str = "MARKET_CONFIRMED_V1";
pub const STRATEGY_LOSS_FILTER_V1: &str = "LOSS_FILTER_V1";
pub const STRATEGY_IV_SELECT_V1: &str = "IV_SELECT_V1";
pub const STRATEGY_FAST_SCOUT_V1: &str = "FAST_SCOUT_V1";
pub const STRATEGY_SURVIVOR_PICKER_V1: &str = "SURVIVOR_PICKER_V1";

pub const HOLDER_TOP1_LOSS_FILTER: f64 = 0.55;
pub const CREATOR_RISK_LOSS_FILTER: f64 = 0.75;
pub const SAME_SLOT_FIRST10_LOSS_FILTER: f64 = 0.70;
pub const SAME_FUNDER_FIRST10_LOSS_FILTER: f64 = 6.0;

pub const HOLDER_SPIKE_ABS: f64 = 0.20;
pub const HOLDER_SPIKE_LEVEL: f64 = 0.45;
pub const COHORT_SELL_ACCEL: f64 = 8.0;
pub const COHORT_RETURN_DROP: f64 = -0.12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimingRegime {
    FastScout,
    ConfirmedEarly,
    Survivor,
}

impl TimingRegime {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FastScout => "FAST_SCOUT",
            Self::ConfirmedEarly => "CONFIRMED_EARLY",
            Self::Survivor => "SURVIVOR",
        }
    }

    pub const fn min_age_seconds(self) -> f64 {
        match self {
            Self::FastScout => 5.0,
            Self::ConfirmedEarly => 20.0,
            Self::Survivor => 120.0,
        }
    }

    pub const fn max_age_seconds(self) -> f64 {
        match self {
            Self::FastScout => 20.0,
            Self::ConfirmedEarly => 90.0,
            Self::Survivor => 600.0,
        }
    }
}

const fn book(
    key: &'static str,
    desk_name: &'static str,
    display_name: &'static str,
    authority: &'static str,
) -> StrategyBook {
    StrategyBook {
        key,
        desk_name,
        display_name,
        uses_research_filter: false,
        uses_shadow_ml_gate: false,
        uses_control_flow: false,
        uses_exploration_policy: false,
        uses_local_graph_gate: false,
        allows_enrichment: false,
        requires_shadow_mode: false,
        ml_status: "NONE",
        execution_authority: authority,
        ai_status: "NONE",
        capital_sol_atomic: EXECUTION_EDGE_CAPITAL_ATOMIC,
    }
}

pub const RANDOM_SELECT_BOOK: StrategyBook = book(
    STRATEGY_RANDOM_SELECT_V1,
    "EXEC-SELECT RANDOM",
    "RANDOM SELECT",
    "DETERMINISTIC_SAMPLE",
);
pub const MARKET_CONFIRMED_BOOK: StrategyBook = book(
    STRATEGY_MARKET_CONFIRMED_V1,
    "EXEC-SELECT MARKET CONFIRMED",
    "MARKET CONFIRMED",
    "MICROSTRUCTURE_RULE_V1",
);
pub const LOSS_FILTER_BOOK: StrategyBook = book(
    STRATEGY_LOSS_FILTER_V1,
    "EXEC-SELECT LOSS FILTER",
    "LOSS FILTER",
    "LOSS_FILTER_V1",
);
pub const IV_SELECT_BOOK: StrategyBook = book(
    STRATEGY_IV_SELECT_V1,
    "EXEC-SELECT INFORMATION VALUE",
    "IV SELECT",
    "INFORMATION_VALUE",
);
pub const FAST_SCOUT_BOOK: StrategyBook = book(
    STRATEGY_FAST_SCOUT_V1,
    "EXEC-SELECT FAST SCOUT",
    "FAST SCOUT",
    "MICROSTRUCTURE_RULE_V1",
);
pub const SURVIVOR_PICKER_BOOK: StrategyBook = book(
    STRATEGY_SURVIVOR_PICKER_V1,
    "EXEC-SELECT SURVIVOR",
    "SURVIVOR PICKER",
    "SURVIVOR_RULE_V1",
);

pub fn exec_select_desks() -> [StrategyBook; 6] {
    [
        RANDOM_SELECT_BOOK,
        MARKET_CONFIRMED_BOOK,
        LOSS_FILTER_BOOK,
        IV_SELECT_BOOK,
        FAST_SCOUT_BOOK,
        SURVIVOR_PICKER_BOOK,
    ]
}

pub fn is_exec_select(strategy_key: &str) -> bool {
    exec_select_desks()
        .iter()
        .any(|book| book.key == strategy_key)
}

pub fn exec_select_timing(strategy_key: &str) -> Option<TimingRegime> {
    match strategy_key {
        STRATEGY_FAST_SCOUT_V1 => Some(TimingRegime::FastScout),
        STRATEGY_SURVIVOR_PICKER_V1 => Some(TimingRegime::Survivor),
        STRATEGY_RANDOM_SELECT_V1
        | STRATEGY_MARKET_CONFIRMED_V1
        | STRATEGY_LOSS_FILTER_V1
        | STRATEGY_IV_SELECT_V1 => Some(TimingRegime::ConfirmedEarly),
        _ => None,
    }
}

pub fn apply_timing_regime(
    technical: &OpportunityDecision,
    age_seconds: Option<f64>,
    regime: TimingRegime,
) -> OpportunityDecision {
    if technical.action != OpportunityAction::Buy {
        return technical.clone();
    }
    let Some(age) = age_seconds.filter(|value| value.is_finite() && *value >= 0.0) else {
        return OpportunityDecision {
            action: OpportunityAction::Abstain,
            reasons: vec!["age_unavailable".into()],
        };
    };
    if age < regime.min_age_seconds() {
        return OpportunityDecision {
            action: OpportunityAction::Watch,
            reasons: vec!["minimum_age_not_reached".into()],
        };
    }
    if age > regime.max_age_seconds() {
        return OpportunityDecision {
            action: OpportunityAction::Watch,
            reasons: vec!["regime_age_exceeded".into()],
        };
    }
    technical.clone()
}

/// Mechanical toxicity filter. Missing evidence never rejects.
pub fn evaluate_loss_filter(
    technical: &OpportunityDecision,
    values: &serde_json::Map<String, Value>,
) -> OpportunityDecision {
    let market = evaluate_microstructure_baseline(technical, values);
    if market.action != OpportunityAction::Buy {
        return market;
    }
    let mut reasons = Vec::new();
    if let Some(share) = feature(values, "holder_top_1_share") {
        if share > HOLDER_TOP1_LOSS_FILTER {
            reasons.push("holder_top_1_concentrated".into());
        }
    }
    if let Some(risk) = feature(values, "creator_risk") {
        if risk > CREATOR_RISK_LOSS_FILTER {
            reasons.push("creator_risk_elevated".into());
        }
    }
    if let Some(share) = feature(values, "same_slot_first10_share") {
        if share > SAME_SLOT_FIRST10_LOSS_FILTER {
            reasons.push("same_slot_first10_concentrated".into());
        }
    }
    if let Some(count) = feature(values, "same_funder_first10_count") {
        if count >= SAME_FUNDER_FIRST10_LOSS_FILTER {
            reasons.push("same_funder_first10_concentrated".into());
        }
    }
    if reasons.is_empty() {
        OpportunityDecision {
            action: OpportunityAction::Buy,
            reasons: vec!["loss_filter_v1_passed".into()],
        }
    } else {
        OpportunityDecision {
            action: OpportunityAction::Avoid,
            reasons,
        }
    }
}

pub fn evaluate_exec_select(
    book: StrategyBook,
    policy: &LearningPolicyV1,
    launch_id: &str,
    technical: &OpportunityDecision,
    age_seconds: Option<f64>,
    values: &serde_json::Map<String, Value>,
    information_score: Option<&InformationValueScore>,
) -> OpportunityDecision {
    let Some(regime) = exec_select_timing(book.key) else {
        return OpportunityDecision {
            action: OpportunityAction::Abstain,
            reasons: vec!["unknown_exec_select_book".into()],
        };
    };
    let gated = apply_timing_regime(technical, age_seconds, regime);
    if gated.action != OpportunityAction::Buy {
        return gated;
    }
    match book.key {
        STRATEGY_RANDOM_SELECT_V1 => evaluate_random_control(policy, launch_id, &gated),
        STRATEGY_MARKET_CONFIRMED_V1 | STRATEGY_FAST_SCOUT_V1 | STRATEGY_SURVIVOR_PICKER_V1 => {
            evaluate_microstructure_baseline(&gated, values)
        }
        STRATEGY_LOSS_FILTER_V1 => evaluate_loss_filter(&gated, values),
        STRATEGY_IV_SELECT_V1 => match information_score {
            Some(score) => evaluate_information_value_explorer(policy, &gated, score),
            None => OpportunityDecision {
                action: OpportunityAction::Abstain,
                reasons: vec!["information_value_unavailable".into()],
            },
        },
        _ => OpportunityDecision {
            action: OpportunityAction::Abstain,
            reasons: vec!["unknown_exec_select_book".into()],
        },
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RiskSnapshot {
    pub holder_top_1_share: Option<f64>,
    pub sell_count_30: Option<f64>,
    pub return_30: Option<f64>,
    pub creator_risk: Option<f64>,
}

impl RiskSnapshot {
    pub fn from_values(values: &serde_json::Map<String, Value>) -> Self {
        Self {
            holder_top_1_share: feature(values, "holder_top_1_share"),
            sell_count_30: feature(values, "sell_count_30"),
            return_30: feature(values, "return_30").or_else(|| feature(values, "return_30s")),
            creator_risk: feature(values, "creator_risk"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PositionRisk {
    Hold,
    Exit {
        class: &'static str,
        reasons: Vec<String>,
    },
}

/// Continuous risk for EXEC-SELECT open positions only. Frozen books never call this.
/// None/UNKNOWN never fires a class.
pub fn evaluate_open_position_risk(entry: &RiskSnapshot, now: &RiskSnapshot) -> PositionRisk {
    if let (Some(was), Some(is)) = (entry.holder_top_1_share, now.holder_top_1_share) {
        if is.is_finite() && was.is_finite() && is >= HOLDER_SPIKE_LEVEL && is - was >= HOLDER_SPIKE_ABS
        {
            return PositionRisk::Exit {
                class: "HOLDER_CONCENTRATION_SPIKE",
                reasons: vec!["holder_concentration_spike".into()],
            };
        }
    }
    if let (Some(sells0), Some(sells1), Some(ret)) =
        (entry.sell_count_30, now.sell_count_30, now.return_30)
    {
        if sells1 - sells0 >= COHORT_SELL_ACCEL && ret <= COHORT_RETURN_DROP {
            return PositionRisk::Exit {
                class: "COHORT_EXIT",
                reasons: vec!["cohort_sell_acceleration".into()],
            };
        }
    }
    if let (Some(was), Some(is)) = (entry.creator_risk, now.creator_risk) {
        if is >= 0.80 && was < 0.50 {
            return PositionRisk::Exit {
                class: "CREATOR_TRANSFER",
                reasons: vec!["creator_risk_deteriorated".into()],
            };
        }
    }
    PositionRisk::Hold
}

pub fn exec_select_entry_atomic() -> u128 {
    EXECUTION_EDGE_ENTRY_ATOMIC
}

pub fn exec_select_simulator() -> &'static str {
    SIMULATOR_EXECUTION_TWIN
}

fn feature(values: &serde_json::Map<String, Value>, name: &str) -> Option<f64> {
    values
        .get(name)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InformationValueWeights, LearningPolicyV1, STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
        STRATEGY_RANDOM_CONTROL_V1,
    };
    use serde_json::json;

    fn buy() -> OpportunityDecision {
        OpportunityDecision {
            action: OpportunityAction::Buy,
            reasons: vec!["technically_executable".into()],
        }
    }

    fn policy() -> LearningPolicyV1 {
        let execution = serde_json::from_value(json!({
            "schema_version": 1,
            "strategy_version": "HEURISTIC_V1",
            "threshold_rationale": "exec-select test envelope",
            "scout_selection_rate": 1.0,
            "scout_cooldown_seconds": 30,
            "minimum_scout_cooldown_seconds": 5,
            "maximum_scout_cooldown_seconds": 300,
            "minimum_age_seconds": 5.0,
            "maximum_age_seconds": 900.0,
            "minimum_unique_buyers": 0,
            "minimum_buy_sell_imbalance": -1.0,
            "minimum_return_30s": -1.0,
            "maximum_return_30s": 10.0,
            "minimum_curve_progress": null,
            "maximum_curve_progress": null,
            "minimum_curve_velocity_per_second": null,
            "minimum_data_quality_score": 0.0,
            "minimum_liquidity_quote_atomic": 1000,
            "maximum_entry_impact_bps": 500,
            "maximum_round_trip_cost_fraction": 0.25,
            "maximum_positions": 5,
            "fixed_sol_atomic": 1000000,
            "arrival_delay_ms": 1000,
            "maximum_arrival_wait_ms": 5000,
            "maximum_hold_seconds": 300,
            "take_profit_fraction": 0.5,
            "stop_loss_fraction": 0.3,
            "trailing_drawdown_fraction": 0.2,
            "buy_flow_deterioration_threshold": 0.8,
            "sell_acceleration_threshold": 0.9,
            "imbalance_reversal_threshold": -0.8,
            "emergency_minimum_liquidity_quote_atomic": 100,
            "emergency_maximum_impact_bps": 1000,
            "partial_exit_fraction": 0.5
        }))
        .unwrap();
        LearningPolicyV1 {
            schema_version: 1,
            experiment_id: crate::ADAPTIVE_EXPERIMENT_ID.into(),
            strategy_version: STRATEGY_RANDOM_CONTROL_V1.into(),
            rationale: "exec-select unit test".into(),
            deterministic_seed: "exec-select-test".into(),
            selection_rate: 1.0,
            minimum_information_value: 0.5,
            information_value_weights: InformationValueWeights {
                prediction_uncertainty: 0.4,
                missingness_novelty: 0.2,
                regime_rarity: 0.2,
                ood_novelty: 0.2,
            },
            execution,
        }
    }

    fn market_ok() -> serde_json::Map<String, Value> {
        let mut values = serde_json::Map::new();
        values.insert("unique_buyers_30".into(), json!(10.0));
        values.insert("buy_sell_imbalance_30".into(), json!(0.5));
        values.insert("return_30".into(), json!(0.05));
        values.insert("sell_count_30".into(), json!(2.0));
        values
    }

    #[test]
    fn confirmed_early_waits_then_accepts_then_expires() {
        let early = apply_timing_regime(&buy(), Some(12.0), TimingRegime::ConfirmedEarly);
        assert_eq!(early.action, OpportunityAction::Watch);
        let ok = apply_timing_regime(&buy(), Some(45.0), TimingRegime::ConfirmedEarly);
        assert_eq!(ok.action, OpportunityAction::Buy);
        let late = apply_timing_regime(&buy(), Some(200.0), TimingRegime::ConfirmedEarly);
        assert_eq!(late.action, OpportunityAction::Watch);
        assert!(late.reasons.iter().any(|r| r == "regime_age_exceeded"));
    }

    #[test]
    fn loss_filter_does_not_reject_unknown() {
        let decision = evaluate_loss_filter(&buy(), &market_ok());
        assert_eq!(decision.action, OpportunityAction::Buy);
    }

    #[test]
    fn loss_filter_rejects_known_holder_concentration() {
        let mut values = market_ok();
        values.insert("holder_top_1_share".into(), json!(0.80));
        let decision = evaluate_loss_filter(&buy(), &values);
        assert_eq!(decision.action, OpportunityAction::Avoid);
        assert!(decision
            .reasons
            .iter()
            .any(|r| r == "holder_top_1_concentrated"));
    }

    #[test]
    fn same_funder_count_is_not_encoded_as_rug() {
        let mut values = market_ok();
        values.insert("same_funder_first10_count".into(), json!(7.0));
        let decision = evaluate_loss_filter(&buy(), &values);
        assert_eq!(decision.action, OpportunityAction::Avoid);
        assert!(decision
            .reasons
            .iter()
            .all(|r| !r.to_ascii_lowercase().contains("rug")));
    }

    #[test]
    fn risk_unknown_never_exits() {
        assert_eq!(
            evaluate_open_position_risk(&RiskSnapshot::default(), &RiskSnapshot::default()),
            PositionRisk::Hold
        );
    }

    #[test]
    fn holder_spike_exits() {
        let entry = RiskSnapshot {
            holder_top_1_share: Some(0.18),
            ..RiskSnapshot::default()
        };
        let now = RiskSnapshot {
            holder_top_1_share: Some(0.52),
            ..RiskSnapshot::default()
        };
        match evaluate_open_position_risk(&entry, &now) {
            PositionRisk::Exit { class, .. } => assert_eq!(class, "HOLDER_CONCENTRATION_SPIKE"),
            PositionRisk::Hold => panic!("expected spike"),
        }
    }

    #[test]
    fn exec_select_iv_requires_score() {
        let decision = evaluate_exec_select(
            IV_SELECT_BOOK,
            &policy(),
            "launch",
            &buy(),
            Some(40.0),
            &market_ok(),
            None,
        );
        assert_eq!(decision.action, OpportunityAction::Abstain);
        let _ = STRATEGY_INFORMATION_VALUE_EXPLORER_V1;
    }

    #[test]
    fn all_exec_select_books_are_twin_sized() {
        for desk in exec_select_desks() {
            assert_eq!(desk.capital_sol_atomic, EXECUTION_EDGE_CAPITAL_ATOMIC);
            assert!(is_exec_select(desk.key));
        }
        assert_eq!(exec_select_entry_atomic(), EXECUTION_EDGE_ENTRY_ATOMIC);
        assert_eq!(exec_select_simulator(), SIMULATOR_EXECUTION_TWIN);
    }
}
