use crate::{ExitAction, ExitDecision, OpportunityAction, OpportunityDecision};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeuristicPolicyV1 {
    pub schema_version: u32,
    pub strategy_version: String,
    pub threshold_rationale: String,
    pub scout_selection_rate: f64,
    pub scout_cooldown_seconds: u64,
    pub minimum_scout_cooldown_seconds: u64,
    pub maximum_scout_cooldown_seconds: u64,
    pub minimum_age_seconds: f64,
    pub maximum_age_seconds: f64,
    pub minimum_unique_buyers: u64,
    pub minimum_buy_sell_imbalance: f64,
    pub minimum_return_30s: f64,
    pub maximum_return_30s: f64,
    pub minimum_curve_progress: Option<f64>,
    pub maximum_curve_progress: Option<f64>,
    pub minimum_curve_velocity_per_second: Option<f64>,
    pub minimum_data_quality_score: f64,
    pub minimum_liquidity_quote_atomic: u128,
    pub maximum_entry_impact_bps: u32,
    pub maximum_round_trip_cost_fraction: f64,
    pub maximum_positions: usize,
    pub fixed_sol_atomic: u128,
    pub arrival_delay_ms: u64,
    pub maximum_arrival_wait_ms: u64,
    pub maximum_hold_seconds: u64,
    pub take_profit_fraction: f64,
    pub stop_loss_fraction: f64,
    pub trailing_drawdown_fraction: f64,
    pub buy_flow_deterioration_threshold: f64,
    pub sell_acceleration_threshold: f64,
    pub imbalance_reversal_threshold: f64,
    pub emergency_minimum_liquidity_quote_atomic: u128,
    pub emergency_maximum_impact_bps: u32,
    pub partial_exit_fraction: f64,
}

impl HeuristicPolicyV1 {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let policy: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid_heuristic_json:{error}"))?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || !supported_strategy_version(&self.strategy_version) {
            return Err("unsupported_heuristic_policy".into());
        }
        if self.threshold_rationale.trim().is_empty() {
            return Err("missing_threshold_rationale".into());
        }
        for (name, value) in [
            ("scout_selection_rate", self.scout_selection_rate),
            (
                "minimum_data_quality_score",
                self.minimum_data_quality_score,
            ),
            (
                "maximum_round_trip_cost_fraction",
                self.maximum_round_trip_cost_fraction,
            ),
            ("take_profit_fraction", self.take_profit_fraction),
            ("stop_loss_fraction", self.stop_loss_fraction),
            (
                "trailing_drawdown_fraction",
                self.trailing_drawdown_fraction,
            ),
            (
                "buy_flow_deterioration_threshold",
                self.buy_flow_deterioration_threshold,
            ),
            (
                "sell_acceleration_threshold",
                self.sell_acceleration_threshold,
            ),
            ("partial_exit_fraction", self.partial_exit_fraction),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("invalid_heuristic_threshold:{name}"));
            }
        }
        if !self.minimum_buy_sell_imbalance.is_finite()
            || !self.minimum_return_30s.is_finite()
            || !self.maximum_return_30s.is_finite()
            || !self.imbalance_reversal_threshold.is_finite()
            || self.minimum_age_seconds < 0.0
            || self.maximum_age_seconds <= self.minimum_age_seconds
            || self.minimum_return_30s > self.maximum_return_30s
        {
            return Err("invalid_heuristic_range".into());
        }
        if self.scout_cooldown_seconds < self.minimum_scout_cooldown_seconds
            || self.scout_cooldown_seconds > self.maximum_scout_cooldown_seconds
            || self.minimum_scout_cooldown_seconds == 0
        {
            return Err("invalid_scout_cooldown".into());
        }
        if self.fixed_sol_atomic == 0 || self.maximum_positions == 0 {
            return Err("invalid_heuristic_exposure".into());
        }
        if self.arrival_delay_ms != 1_000 || self.maximum_arrival_wait_ms < self.arrival_delay_ms {
            return Err("heuristic_arrival_must_be_one_second".into());
        }
        if self.partial_exit_fraction <= 0.0 || self.partial_exit_fraction >= 1.0 {
            return Err("invalid_heuristic_partial_exit_fraction".into());
        }
        for value in [
            self.minimum_curve_progress,
            self.maximum_curve_progress,
            self.minimum_curve_velocity_per_second,
        ]
        .into_iter()
        .flatten()
        {
            if !value.is_finite() {
                return Err("invalid_curve_threshold".into());
            }
        }
        if self
            .minimum_curve_progress
            .zip(self.maximum_curve_progress)
            .is_some_and(|(minimum, maximum)| minimum > maximum)
        {
            return Err("invalid_curve_progress_range".into());
        }
        Ok(())
    }
}

pub fn supported_strategy_version(version: &str) -> bool {
    matches!(
        version,
        "HEURISTIC_V1" | "CONTROL_FLOW_V1" | "RESEARCH_EXPLORATION_V1"
    )
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeuristicOpportunityInput {
    pub age_seconds: Option<f64>,
    pub unique_buyers: Option<u64>,
    pub buy_sell_imbalance: Option<f64>,
    pub return_30s: Option<f64>,
    pub curve_progress: Option<f64>,
    pub curve_velocity_per_second: Option<f64>,
    pub data_quality_score: Option<f64>,
    pub quality_sufficient: bool,
    pub liquidity_quote_atomic: Option<u128>,
    pub entry_impact_bps: Option<u32>,
    pub round_trip_cost_fraction: Option<f64>,
    pub open_positions: usize,
    pub scout_selected: bool,
}

pub fn evaluate_heuristic_opportunity(
    policy: &HeuristicPolicyV1,
    input: &HeuristicOpportunityInput,
) -> OpportunityDecision {
    let mut missing = Vec::new();
    if input.age_seconds.is_none() {
        missing.push("age_unavailable");
    }
    if input.unique_buyers.is_none() {
        missing.push("buyers_unavailable");
    }
    if input.buy_sell_imbalance.is_none() {
        missing.push("imbalance_unavailable");
    }
    if input.return_30s.is_none() {
        missing.push("return_unavailable");
    }
    if input.data_quality_score.is_none() || !input.quality_sufficient {
        missing.push("data_quality_insufficient");
    }
    if input.liquidity_quote_atomic.is_none()
        || input.entry_impact_bps.is_none()
        || input.round_trip_cost_fraction.is_none()
    {
        missing.push("executable_cost_unavailable");
    }
    if !missing.is_empty() {
        return opportunity(OpportunityAction::Abstain, missing);
    }

    let mut avoid = Vec::new();
    if input.age_seconds.unwrap_or_default() > policy.maximum_age_seconds {
        avoid.push("maximum_age_exceeded");
    }
    if input.liquidity_quote_atomic.unwrap_or_default() < policy.minimum_liquidity_quote_atomic {
        avoid.push("liquidity_below_minimum");
    }
    if input.entry_impact_bps.unwrap_or(u32::MAX) > policy.maximum_entry_impact_bps {
        avoid.push("entry_impact_too_high");
    }
    if input.round_trip_cost_fraction.unwrap_or(f64::INFINITY)
        > policy.maximum_round_trip_cost_fraction
    {
        avoid.push("round_trip_cost_too_high");
    }
    if !avoid.is_empty() {
        return opportunity(OpportunityAction::Avoid, avoid);
    }

    let mut watch = Vec::new();
    if input.age_seconds.unwrap_or_default() < policy.minimum_age_seconds {
        watch.push("minimum_age_not_reached");
    }
    if input.unique_buyers.unwrap_or_default() < policy.minimum_unique_buyers {
        watch.push("buyers_below_minimum");
    }
    if input.buy_sell_imbalance.unwrap_or_default() < policy.minimum_buy_sell_imbalance {
        watch.push("buy_imbalance_below_minimum");
    }
    let return_30s = input.return_30s.unwrap_or_default();
    if return_30s < policy.minimum_return_30s || return_30s > policy.maximum_return_30s {
        watch.push("return_out_of_band");
    }
    if policy
        .minimum_curve_progress
        .zip(input.curve_progress)
        .is_some_and(|(minimum, actual)| actual < minimum)
        || policy
            .maximum_curve_progress
            .zip(input.curve_progress)
            .is_some_and(|(maximum, actual)| actual > maximum)
    {
        watch.push("curve_progress_out_of_band");
    }
    if policy
        .minimum_curve_velocity_per_second
        .zip(input.curve_velocity_per_second)
        .is_some_and(|(minimum, actual)| actual < minimum)
    {
        watch.push("curve_velocity_below_minimum");
    }
    if input.data_quality_score.unwrap_or_default() < policy.minimum_data_quality_score {
        watch.push("data_quality_below_minimum");
    }
    if input.open_positions >= policy.maximum_positions {
        watch.push("maximum_positions_reached");
    }
    if !input.scout_selected {
        watch.push("scout_rate_not_selected");
    }
    if watch.is_empty() {
        opportunity(OpportunityAction::Buy, vec!["heuristic_gates_passed"])
    } else {
        opportunity(OpportunityAction::Watch, watch)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeuristicExitInput {
    pub hold_seconds: u64,
    pub mark_return_fraction: Option<f64>,
    pub peak_return_fraction: Option<f64>,
    pub buy_flow_deterioration: Option<f64>,
    pub sell_acceleration: Option<f64>,
    pub buy_sell_imbalance: Option<f64>,
    pub liquidity_quote_atomic: Option<u128>,
    pub exit_impact_bps: Option<u32>,
}

pub fn evaluate_heuristic_exit(
    policy: &HeuristicPolicyV1,
    input: &HeuristicExitInput,
) -> ExitDecision {
    if input
        .liquidity_quote_atomic
        .is_some_and(|value| value < policy.emergency_minimum_liquidity_quote_atomic)
        || input
            .exit_impact_bps
            .is_some_and(|value| value > policy.emergency_maximum_impact_bps)
    {
        return exit(
            ExitAction::EmergencyExit,
            1.0,
            "liquidity_or_impact_emergency",
        );
    }
    if input.hold_seconds >= policy.maximum_hold_seconds {
        return exit(ExitAction::FullExit, 1.0, "maximum_hold_reached");
    }
    if input
        .mark_return_fraction
        .is_some_and(|value| value <= -policy.stop_loss_fraction)
    {
        return exit(ExitAction::FullExit, 1.0, "stop_loss_reached");
    }
    if input
        .sell_acceleration
        .is_some_and(|value| value >= policy.sell_acceleration_threshold)
        || input
            .buy_sell_imbalance
            .is_some_and(|value| value <= policy.imbalance_reversal_threshold)
    {
        return exit(ExitAction::FullExit, 1.0, "sell_flow_reversal");
    }
    if input
        .mark_return_fraction
        .is_some_and(|value| value >= policy.take_profit_fraction)
    {
        return exit(
            ExitAction::PartialExit,
            policy.partial_exit_fraction,
            "take_profit_reached",
        );
    }
    if input
        .mark_return_fraction
        .zip(input.peak_return_fraction)
        .is_some_and(|(current, peak)| {
            peak > 0.0 && peak - current >= policy.trailing_drawdown_fraction
        })
    {
        return exit(ExitAction::FullExit, 1.0, "trailing_drawdown_reached");
    }
    if input
        .buy_flow_deterioration
        .is_some_and(|value| value >= policy.buy_flow_deterioration_threshold)
    {
        return exit(
            ExitAction::PartialExit,
            policy.partial_exit_fraction,
            "buy_flow_deterioration",
        );
    }
    exit(ExitAction::Hold, 0.0, "heuristic_hold")
}

fn opportunity(action: OpportunityAction, reasons: Vec<&str>) -> OpportunityDecision {
    OpportunityDecision {
        action,
        reasons: reasons.into_iter().map(str::to_string).collect(),
    }
}

fn exit(action: ExitAction, fraction: f64, reason: &str) -> ExitDecision {
    ExitDecision {
        action,
        fraction,
        reasons: vec![reason.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HeuristicPolicyV1 {
        let value = serde_json::json!({
            "schema_version": 1,
            "strategy_version": "HEURISTIC_V1",
            "threshold_rationale": "conservative deterministic paper scout",
            "scout_selection_rate": 1.0,
            "scout_cooldown_seconds": 60,
            "minimum_scout_cooldown_seconds": 30,
            "maximum_scout_cooldown_seconds": 300,
            "minimum_age_seconds": 30.0,
            "maximum_age_seconds": 900.0,
            "minimum_unique_buyers": 25,
            "minimum_buy_sell_imbalance": 0.2,
            "minimum_return_30s": 0.05,
            "maximum_return_30s": 0.5,
            "minimum_curve_progress": 0.1,
            "maximum_curve_progress": 0.85,
            "minimum_curve_velocity_per_second": null,
            "minimum_data_quality_score": 0.7,
            "minimum_liquidity_quote_atomic": 1000,
            "maximum_entry_impact_bps": 150,
            "maximum_round_trip_cost_fraction": 0.08,
            "maximum_positions": 5,
            "fixed_sol_atomic": 50000000,
            "arrival_delay_ms": 1000,
            "maximum_arrival_wait_ms": 5000,
            "maximum_hold_seconds": 3600,
            "take_profit_fraction": 0.5,
            "stop_loss_fraction": 0.2,
            "trailing_drawdown_fraction": 0.15,
            "buy_flow_deterioration_threshold": 0.6,
            "sell_acceleration_threshold": 0.7,
            "imbalance_reversal_threshold": -0.25,
            "emergency_minimum_liquidity_quote_atomic": 500,
            "emergency_maximum_impact_bps": 500,
            "partial_exit_fraction": 0.5
        });
        HeuristicPolicyV1::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn eligible() -> HeuristicOpportunityInput {
        HeuristicOpportunityInput {
            age_seconds: Some(60.0),
            unique_buyers: Some(30),
            buy_sell_imbalance: Some(0.4),
            return_30s: Some(0.2),
            curve_progress: Some(0.4),
            data_quality_score: Some(0.9),
            quality_sufficient: true,
            liquidity_quote_atomic: Some(10_000),
            entry_impact_bps: Some(50),
            round_trip_cost_fraction: Some(0.03),
            scout_selected: true,
            ..Default::default()
        }
    }

    #[test]
    fn active_launch_reaches_buy_when_all_gates_pass() {
        assert_eq!(
            evaluate_heuristic_opportunity(&policy(), &eligible()).action,
            OpportunityAction::Buy
        );
    }

    #[test]
    fn unknown_persisted_launch_age_abstains() {
        let mut input = eligible();
        input.age_seconds = None;
        let decision = evaluate_heuristic_opportunity(&policy(), &input);
        assert_eq!(decision.action, OpportunityAction::Abstain);
        assert!(decision.reasons.contains(&"age_unavailable".to_string()));
    }

    #[test]
    fn unknown_mark_does_not_invent_pnl_exit() {
        let result = evaluate_heuristic_exit(
            &policy(),
            &HeuristicExitInput {
                hold_seconds: 10,
                liquidity_quote_atomic: Some(10_000),
                ..Default::default()
            },
        );
        assert_eq!(result.action, ExitAction::Hold);
    }

    #[test]
    fn heuristic_exit_precedence_is_deterministic() {
        let result = evaluate_heuristic_exit(
            &policy(),
            &HeuristicExitInput {
                hold_seconds: 10,
                mark_return_fraction: Some(-0.3),
                liquidity_quote_atomic: Some(10_000),
                exit_impact_bps: Some(50),
                ..Default::default()
            },
        );
        assert_eq!(result.action, ExitAction::FullExit);
    }

    #[test]
    fn heuristic_auto_exit_can_reconcile_fifo() {
        let exit = evaluate_heuristic_exit(
            &policy(),
            &HeuristicExitInput {
                hold_seconds: 3_601,
                liquidity_quote_atomic: Some(10_000),
                exit_impact_bps: Some(50),
                ..Default::default()
            },
        );
        assert_eq!(exit.action, ExitAction::FullExit);
        let allocations = crate::fifo_allocate(
            &[crate::OpenLot {
                lot_id: "first".into(),
                remaining_base_atomic: 100,
                remaining_cost_atomic: 200,
            }],
            100,
            300,
        )
        .unwrap();
        assert_eq!(allocations[0].realized_pnl_atomic, 100);
    }
}
