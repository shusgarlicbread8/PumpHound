use crate::{HeuristicPolicyV1, OpportunityAction, OpportunityDecision, StrategyBook};
use serde::{Deserialize, Serialize};

pub const ADAPTIVE_EXPERIMENT_ID: &str = "ALPHA-LEARN";
pub const STRATEGY_RANDOM_CONTROL_V1: &str = "RANDOM_CONTROL_V1";
pub const STRATEGY_INFORMATION_VALUE_EXPLORER_V1: &str = "INFORMATION_VALUE_EXPLORER_V1";
pub const STRATEGY_CHAMPION_SHORT_HORIZON_V1: &str = "CHAMPION_SHORT_HORIZON_V1";
pub const CHAMPION_MINIMUM_MOMENTUM: f64 = 0.55;
pub const CHAMPION_MAXIMUM_ADVERSE: f64 = 0.50;
pub const CHAMPION_MINIMUM_FAMILIARITY: f64 = 0.50;
pub const CHAMPION_MAX_MISSINGNESS: f64 = 0.50;
pub const CHAMPION_MAX_OOD: f64 = 0.80;

#[derive(Clone, Copy, Debug, Default)]
pub struct ChampionInputQuality {
    pub missingness: Option<f64>,
    pub ood: Option<f64>,
}
const LEARNING_BOOK_CAPITAL_ATOMIC: i128 = 2_000_000_000;

pub const RANDOM_CONTROL_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_RANDOM_CONTROL_V1,
    desk_name: "ALPHA-LEARN RANDOM CONTROL",
    display_name: "RANDOM CONTROL",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "SHADOW",
    execution_authority: "DETERMINISTIC_SAMPLE",
    ai_status: "NONE",
    capital_sol_atomic: LEARNING_BOOK_CAPITAL_ATOMIC,
};

pub const INFORMATION_VALUE_EXPLORER_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
    desk_name: "ALPHA-LEARN INFORMATION VALUE",
    display_name: "INFORMATION VALUE",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: true,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "SHADOW",
    execution_authority: "INFORMATION_VALUE",
    ai_status: "NONE",
    capital_sol_atomic: LEARNING_BOOK_CAPITAL_ATOMIC,
};

pub const CHAMPION_SHORT_HORIZON_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_CHAMPION_SHORT_HORIZON_V1,
    desk_name: "ALPHA-LEARN CHAMPION ML",
    display_name: "CHAMPION ML",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "CONTROL",
    execution_authority: "CHAMPION_SHORT_HORIZON",
    ai_status: "NONE",
    capital_sol_atomic: LEARNING_BOOK_CAPITAL_ATOMIC,
};

pub fn learning_books() -> [StrategyBook; 2] {
    [RANDOM_CONTROL_BOOK, INFORMATION_VALUE_EXPLORER_BOOK]
}

pub fn adaptive_desks() -> [StrategyBook; 3] {
    [
        RANDOM_CONTROL_BOOK,
        INFORMATION_VALUE_EXPLORER_BOOK,
        CHAMPION_SHORT_HORIZON_BOOK,
    ]
}

pub const SERVING_FEATURE_NAMES: &[&str] = &[
    "age_seconds",
    "buy_count_30",
    "buy_count_60",
    "buy_count_120",
    "sell_count_30",
    "sell_count_60",
    "sell_count_120",
    "buy_sell_imbalance_30",
    "buy_sell_imbalance_60",
    "unique_buyers_30",
    "unique_buyers_60",
    "return_30",
];

pub fn vector_serving_missingness(values: &serde_json::Map<String, serde_json::Value>) -> f64 {
    let missing = SERVING_FEATURE_NAMES
        .iter()
        .filter(|&&name| values.get(name).map_or(true, |v| v.is_null()))
        .count();
    missing as f64 / SERVING_FEATURE_NAMES.len() as f64
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LearningFunnelState {
    Observed,
    TechnicallyInvalid,
    WaitingInformation,
    StrategyEvaluated,
    StrategyRejected,
    Selected,
    PaperEntered,
    Exited,
    LabelMatured,
}

impl LearningFunnelState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "OBSERVED",
            Self::TechnicallyInvalid => "TECHNICALLY_INVALID",
            Self::WaitingInformation => "WAITING_INFORMATION",
            Self::StrategyEvaluated => "STRATEGY_EVALUATED",
            Self::StrategyRejected => "STRATEGY_REJECTED",
            Self::Selected => "SELECTED",
            Self::PaperEntered => "PAPER_ENTERED",
            Self::Exited => "EXITED",
            Self::LabelMatured => "LABEL_MATURED",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InformationValueWeights {
    pub prediction_uncertainty: f64,
    pub missingness_novelty: f64,
    pub regime_rarity: f64,
    pub ood_novelty: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LearningPolicyV1 {
    pub schema_version: u32,
    pub experiment_id: String,
    pub strategy_version: String,
    pub rationale: String,
    pub deterministic_seed: String,
    pub selection_rate: f64,
    pub minimum_information_value: f64,
    pub information_value_weights: InformationValueWeights,
    pub execution: HeuristicPolicyV1,
}

impl LearningPolicyV1 {
    pub fn from_json(bytes: &[u8], expected_strategy: &str) -> Result<Self, String> {
        let policy: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid_learning_json:{error}"))?;
        policy.validate(expected_strategy)?;
        Ok(policy)
    }

    pub fn validate(&self, expected_strategy: &str) -> Result<(), String> {
        if self.schema_version != 1
            || self.experiment_id != ADAPTIVE_EXPERIMENT_ID
            || self.strategy_version != expected_strategy
            || !matches!(
                expected_strategy,
                STRATEGY_RANDOM_CONTROL_V1 | STRATEGY_INFORMATION_VALUE_EXPLORER_V1
            )
        {
            return Err("unsupported_learning_policy".into());
        }
        if self.rationale.trim().is_empty() || self.deterministic_seed.trim().is_empty() {
            return Err("learning_policy_missing_rationale_or_seed".into());
        }
        for value in [
            self.selection_rate,
            self.minimum_information_value,
            self.information_value_weights.prediction_uncertainty,
            self.information_value_weights.missingness_novelty,
            self.information_value_weights.regime_rarity,
            self.information_value_weights.ood_novelty,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err("learning_policy_invalid_probability".into());
            }
        }
        let weight_sum = self.information_value_weights.prediction_uncertainty
            + self.information_value_weights.missingness_novelty
            + self.information_value_weights.regime_rarity
            + self.information_value_weights.ood_novelty;
        if (weight_sum - 1.0).abs() > 1e-9 {
            return Err("learning_policy_weights_must_sum_to_one".into());
        }
        self.execution.validate()
    }
}

#[derive(Clone, Debug)]
pub struct TechnicalEligibilityInput {
    pub age_seconds: Option<f64>,
    pub quality_sufficient: bool,
    pub liquidity_quote_atomic: Option<u128>,
    pub entry_impact_bps: Option<u32>,
    pub round_trip_cost_fraction: Option<f64>,
    pub open_positions: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct InformationValueInput {
    pub prediction_uncertainty: Option<f64>,
    pub missingness_novelty: f64,
    pub regime_rarity: Option<f64>,
    pub familiarity: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InformationValueScore {
    pub score: f64,
    pub prediction_uncertainty: f64,
    pub missingness_novelty: f64,
    pub regime_rarity: f64,
    pub ood_novelty: f64,
}

pub fn evaluate_technical_eligibility(
    policy: &LearningPolicyV1,
    input: &TechnicalEligibilityInput,
) -> OpportunityDecision {
    let execution = &policy.execution;
    let Some(age) = input
        .age_seconds
        .filter(|value| value.is_finite() && *value >= 0.0)
    else {
        return decision(OpportunityAction::Abstain, &["age_unavailable"]);
    };
    if age < execution.minimum_age_seconds {
        return decision(OpportunityAction::Watch, &["minimum_age_not_reached"]);
    }
    if input.entry_impact_bps.is_none() || input.round_trip_cost_fraction.is_none() {
        return decision(OpportunityAction::Abstain, &["executable_cost_unavailable"]);
    }
    let mut invalid = Vec::new();
    if age > execution.maximum_age_seconds {
        invalid.push("maximum_age_exceeded");
    }
    if !input.quality_sufficient {
        invalid.push("data_quality_insufficient");
    }
    if input.liquidity_quote_atomic.unwrap_or_default() < execution.minimum_liquidity_quote_atomic {
        invalid.push("liquidity_below_minimum");
    }
    if input.entry_impact_bps.unwrap_or(u32::MAX) > execution.maximum_entry_impact_bps {
        invalid.push("entry_impact_too_high");
    }
    if input.round_trip_cost_fraction.unwrap_or(f64::INFINITY)
        > execution.maximum_round_trip_cost_fraction
    {
        invalid.push("round_trip_cost_too_high");
    }
    if !invalid.is_empty() {
        return OpportunityDecision {
            action: OpportunityAction::Avoid,
            reasons: invalid.into_iter().map(str::to_string).collect(),
        };
    }
    if input.open_positions >= execution.maximum_positions {
        return decision(OpportunityAction::Watch, &["position_capacity_reached"]);
    }
    decision(OpportunityAction::Buy, &["technically_executable"])
}

pub fn evaluate_random_control(
    policy: &LearningPolicyV1,
    launch_id: &str,
    technical: &OpportunityDecision,
) -> OpportunityDecision {
    if technical.action != OpportunityAction::Buy {
        return technical.clone();
    }
    let selected =
        deterministic_sample(&policy.deterministic_seed, launch_id, policy.selection_rate);
    if selected {
        decision(OpportunityAction::Buy, &["deterministic_control_selected"])
    } else {
        decision(
            OpportunityAction::Watch,
            &["deterministic_control_not_selected"],
        )
    }
}

pub fn score_information_value(
    policy: &LearningPolicyV1,
    input: &InformationValueInput,
) -> InformationValueScore {
    let uncertainty = bounded(input.prediction_uncertainty.unwrap_or(1.0));
    let missingness = bounded(input.missingness_novelty);
    let rarity = bounded(input.regime_rarity.unwrap_or(0.5));
    let ood = bounded(1.0 - input.familiarity.unwrap_or(0.0));
    let weights = &policy.information_value_weights;
    InformationValueScore {
        score: uncertainty * weights.prediction_uncertainty
            + missingness * weights.missingness_novelty
            + rarity * weights.regime_rarity
            + ood * weights.ood_novelty,
        prediction_uncertainty: uncertainty,
        missingness_novelty: missingness,
        regime_rarity: rarity,
        ood_novelty: ood,
    }
}

pub fn evaluate_champion_short_horizon(
    technical: &OpportunityDecision,
    momentum: Option<f64>,
    adverse: Option<f64>,
    expected_mfe: Option<f64>,
    familiarity: Option<f64>,
    market_stale: bool,
    input_quality: ChampionInputQuality,
) -> OpportunityDecision {
    if market_stale {
        return decision(OpportunityAction::Abstain, &["market_data_stale"]);
    }
    if input_quality
        .missingness
        .is_some_and(|missingness| missingness > CHAMPION_MAX_MISSINGNESS)
        || input_quality.ood.is_some_and(|ood| ood > CHAMPION_MAX_OOD)
    {
        return decision(OpportunityAction::Abstain, &["data_quality_blocked"]);
    }
    if technical.action != OpportunityAction::Buy {
        return technical.clone();
    }
    let Some(momentum) = momentum else {
        return decision(
            OpportunityAction::Abstain,
            &["champion_momentum_unavailable"],
        );
    };
    let Some(adverse) = adverse else {
        return decision(
            OpportunityAction::Abstain,
            &["champion_adverse_prediction_unavailable"],
        );
    };
    if familiarity.unwrap_or(0.0) < CHAMPION_MINIMUM_FAMILIARITY {
        return decision(
            OpportunityAction::Watch,
            &["champion_familiarity_below_minimum"],
        );
    }
    if momentum >= CHAMPION_MINIMUM_MOMENTUM && adverse <= CHAMPION_MAXIMUM_ADVERSE {
        let mut reasons = vec!["champion_short_horizon_selected".to_string()];
        if expected_mfe.is_some() {
            reasons.push("expected_mfe_observed".into());
        }
        return OpportunityDecision {
            action: OpportunityAction::Buy,
            reasons,
            ..technical.clone()
        };
    }
    decision(OpportunityAction::Watch, &["champion_thresholds_not_met"])
}

pub fn evaluate_information_value_explorer(
    policy: &LearningPolicyV1,
    technical: &OpportunityDecision,
    score: &InformationValueScore,
) -> OpportunityDecision {
    if technical.action != OpportunityAction::Buy {
        return technical.clone();
    }
    if score.score >= policy.minimum_information_value {
        decision(OpportunityAction::Buy, &["information_value_selected"])
    } else {
        decision(
            OpportunityAction::Watch,
            &["information_value_below_minimum"],
        )
    }
}

pub fn learning_funnel_state(decision: &OpportunityDecision) -> LearningFunnelState {
    if decision.reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "age_unavailable" | "minimum_age_not_reached" | "executable_cost_unavailable"
        )
    }) {
        return LearningFunnelState::WaitingInformation;
    }
    if decision.reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "maximum_age_exceeded"
                | "data_quality_insufficient"
                | "data_quality_blocked"
                | "liquidity_below_minimum"
                | "entry_impact_too_high"
                | "round_trip_cost_too_high"
        )
    }) {
        return LearningFunnelState::TechnicallyInvalid;
    }
    match decision.action {
        OpportunityAction::Buy => LearningFunnelState::Selected,
        OpportunityAction::Watch | OpportunityAction::Avoid => {
            LearningFunnelState::StrategyRejected
        }
        OpportunityAction::Abstain => LearningFunnelState::WaitingInformation,
    }
}

pub fn deterministic_sample(seed: &str, launch_id: &str, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    if rate <= 0.0 {
        return false;
    }
    let hash = pumphound_domain::sha256_hex(format!("{seed}:{launch_id}").as_bytes());
    let bucket = u64::from_str_radix(&hash[..16], 16).unwrap_or(u64::MAX);
    (bucket as f64 / u64::MAX as f64) < rate
}

fn decision(action: OpportunityAction, reasons: &[&str]) -> OpportunityDecision {
    OpportunityDecision {
        action,
        reasons: reasons.iter().map(|reason| (*reason).to_string()).collect(),
    }
}

fn bounded(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(strategy: &str) -> LearningPolicyV1 {
        let execution = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "strategy_version": "HEURISTIC_V1",
            "threshold_rationale": "paper-only fixed execution and exit policy",
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
            experiment_id: ADAPTIVE_EXPERIMENT_ID.into(),
            strategy_version: strategy.into(),
            rationale: "collect unbiased paper outcomes".into(),
            deterministic_seed: "alpha-learn-v1".into(),
            selection_rate: 0.2,
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

    #[test]
    fn control_is_deterministic_and_does_not_accept_model_input() {
        let policy = policy(STRATEGY_RANDOM_CONTROL_V1);
        let eligible = decision(OpportunityAction::Buy, &["technically_executable"]);
        let first = evaluate_random_control(&policy, "launch-1", &eligible);
        for _ in 0..20 {
            assert_eq!(
                evaluate_random_control(&policy, "launch-1", &eligible).action,
                first.action
            );
        }
    }

    #[test]
    fn early_features_are_waiting_not_invalid() {
        let policy = policy(STRATEGY_RANDOM_CONTROL_V1);
        let decision = evaluate_technical_eligibility(
            &policy,
            &TechnicalEligibilityInput {
                age_seconds: Some(2.0),
                quality_sufficient: true,
                liquidity_quote_atomic: Some(10_000),
                entry_impact_bps: Some(1),
                round_trip_cost_fraction: Some(0.01),
                open_positions: 0,
            },
        );
        assert_eq!(
            learning_funnel_state(&decision),
            LearningFunnelState::WaitingInformation
        );
    }

    #[test]
    fn explorer_values_missing_and_ood_regions() {
        let policy = policy(STRATEGY_INFORMATION_VALUE_EXPLORER_V1);
        let score = score_information_value(
            &policy,
            &InformationValueInput {
                prediction_uncertainty: None,
                missingness_novelty: 1.0,
                regime_rarity: Some(1.0),
                familiarity: Some(0.0),
            },
        );
        assert!(score.score > 0.9);
    }

    #[test]
    fn learning_books_remain_the_two_policy_invariants() {
        assert_eq!(learning_books().len(), 2);
        assert_eq!(adaptive_desks().len(), 3);
        assert_eq!(adaptive_desks()[2].key, STRATEGY_CHAMPION_SHORT_HORIZON_V1);
        assert_eq!(adaptive_desks()[2].ml_status, "CONTROL");
    }

    #[test]
    fn champion_book_requires_valid_champion_and_fresh_market() {
        let eligible = decision(OpportunityAction::Buy, &["technically_executable"]);
        let stale = evaluate_champion_short_horizon(
            &eligible,
            Some(0.8),
            Some(0.1),
            Some(0.4),
            Some(0.9),
            true,
            ChampionInputQuality::default(),
        );
        assert_eq!(stale.action, OpportunityAction::Abstain);
        let missing = evaluate_champion_short_horizon(
            &eligible,
            None,
            Some(0.1),
            None,
            Some(0.9),
            false,
            ChampionInputQuality::default(),
        );
        assert_eq!(missing.action, OpportunityAction::Abstain);
        let missing_adverse = evaluate_champion_short_horizon(
            &eligible,
            Some(0.8),
            None,
            Some(0.4),
            Some(0.9),
            false,
            ChampionInputQuality::default(),
        );
        assert_eq!(missing_adverse.action, OpportunityAction::Abstain);
        assert!(missing_adverse
            .reasons
            .iter()
            .any(|reason| reason == "champion_adverse_prediction_unavailable"));
        let buy = evaluate_champion_short_horizon(
            &eligible,
            Some(0.7),
            Some(0.2),
            Some(0.3),
            Some(0.8),
            false,
            ChampionInputQuality::default(),
        );
        assert_eq!(buy.action, OpportunityAction::Buy);
        assert!(buy
            .reasons
            .iter()
            .any(|reason| reason == "champion_short_horizon_selected"));
        let blocked = evaluate_champion_short_horizon(
            &eligible,
            Some(0.8),
            Some(0.1),
            Some(0.4),
            Some(0.9),
            false,
            ChampionInputQuality {
                missingness: Some(0.82),
                ood: Some(0.95),
            },
        );
        assert_eq!(blocked.action, OpportunityAction::Abstain);
        assert!(blocked
            .reasons
            .iter()
            .any(|reason| reason == "data_quality_blocked"));
    }
}
