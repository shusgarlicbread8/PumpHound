use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OpportunityAction {
    Buy,
    Watch,
    Avoid,
    Abstain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExitAction {
    Hold,
    PartialExit,
    FullExit,
    EmergencyExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DurableOrderStatus {
    Decided,
    Queued,
    Arrived,
    Filled,
    Rejected,
    Position,
}

impl DurableOrderStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Decided, Self::Queued)
                | (Self::Queued, Self::Arrived)
                | (Self::Queued, Self::Rejected)
                | (Self::Arrived, Self::Filled)
                | (Self::Arrived, Self::Rejected)
                | (Self::Filled, Self::Position)
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutonomousPolicy {
    pub schema_version: u32,
    pub policy_version: String,
    #[serde(default = "default_minimum_familiarity")]
    pub minimum_familiarity: f64,
    pub minimum_data_completeness: f64,
    pub minimum_graduation_probability: f64,
    pub minimum_momentum_probability: f64,
    pub maximum_collapse_probability: f64,
    pub maximum_exit_probability_for_entry: f64,
    pub minimum_expected_net_edge: f64,
    pub maximum_round_trip_cost_fraction: f64,
    pub maximum_price_impact_bps: u32,
    pub minimum_liquidity_quote_atomic: u128,
    pub maximum_creator_risk: f64,
    pub maximum_wallet_risk: f64,
    pub maximum_cluster_risk: f64,
    pub partial_exit_probability: f64,
    pub full_exit_probability: f64,
    pub emergency_collapse_probability: f64,
    pub minimum_exit_liquidity_quote_atomic: u128,
    pub partial_exit_fraction: f64,
    pub arrival_delay_ms: u64,
    pub maximum_arrival_wait_ms: u64,
}

impl AutonomousPolicy {
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let policy: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("invalid_policy_json:{e}"))?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("unsupported_policy_schema".into());
        }
        if self.policy_version.trim().is_empty() {
            return Err("empty_policy_version".into());
        }
        for (name, value) in [
            ("minimum_data_completeness", self.minimum_data_completeness),
            ("minimum_familiarity", self.minimum_familiarity),
            (
                "minimum_graduation_probability",
                self.minimum_graduation_probability,
            ),
            (
                "minimum_momentum_probability",
                self.minimum_momentum_probability,
            ),
            (
                "maximum_collapse_probability",
                self.maximum_collapse_probability,
            ),
            (
                "maximum_exit_probability_for_entry",
                self.maximum_exit_probability_for_entry,
            ),
            ("maximum_creator_risk", self.maximum_creator_risk),
            ("maximum_wallet_risk", self.maximum_wallet_risk),
            ("maximum_cluster_risk", self.maximum_cluster_risk),
            ("partial_exit_probability", self.partial_exit_probability),
            ("full_exit_probability", self.full_exit_probability),
            (
                "emergency_collapse_probability",
                self.emergency_collapse_probability,
            ),
            ("partial_exit_fraction", self.partial_exit_fraction),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(format!("invalid_policy_threshold:{name}"));
            }
        }
        if !self.minimum_expected_net_edge.is_finite()
            || !self.maximum_round_trip_cost_fraction.is_finite()
            || self.maximum_round_trip_cost_fraction < 0.0
        {
            return Err("invalid_policy_cost_threshold".into());
        }
        if self.partial_exit_probability > self.full_exit_probability {
            return Err("partial_exit_above_full_exit".into());
        }
        if self.partial_exit_fraction <= 0.0 || self.partial_exit_fraction >= 1.0 {
            return Err("invalid_partial_exit_fraction".into());
        }
        if self.arrival_delay_ms == 0 || self.maximum_arrival_wait_ms < self.arrival_delay_ms {
            return Err("invalid_arrival_timing".into());
        }
        Ok(())
    }
}

fn default_minimum_familiarity() -> f64 {
    0.5
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OpportunityInput {
    pub champion_present: bool,
    pub calibrated: bool,
    pub familiarity: Option<f64>,
    pub data_completeness: Option<f64>,
    pub quality_sufficient: bool,
    pub graduation_probability: Option<f64>,
    pub momentum_probability: Option<f64>,
    pub collapse_probability: Option<f64>,
    pub exit_probability: Option<f64>,
    pub expected_net_edge: Option<f64>,
    pub round_trip_cost_fraction: Option<f64>,
    pub price_impact_bps: Option<u32>,
    pub liquidity_quote_atomic: Option<u128>,
    pub creator_risk: Option<f64>,
    pub wallet_risk: Option<f64>,
    pub cluster_risk: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpportunityDecision {
    pub action: OpportunityAction,
    pub reasons: Vec<String>,
}

pub fn evaluate_opportunity(
    policy: Option<&AutonomousPolicy>,
    input: &OpportunityInput,
) -> OpportunityDecision {
    let Some(policy) = policy else {
        return decision(OpportunityAction::Abstain, &["policy_artifact_absent"]);
    };
    let mut missing = Vec::new();
    if !input.champion_present {
        missing.push("champion_absent");
    }
    if !input.calibrated {
        missing.push("model_not_calibrated");
    }
    if input.familiarity.is_none() {
        missing.push("familiarity_withheld");
    }
    if input.data_completeness.is_none() {
        missing.push("data_completeness_missing");
    }
    if !input.quality_sufficient {
        missing.push("data_quality_insufficient");
    }
    if input.graduation_probability.is_none() {
        missing.push("graduation_probability_missing");
    }
    if input.momentum_probability.is_none() {
        missing.push("momentum_probability_missing");
    }
    if input.collapse_probability.is_none() {
        missing.push("collapse_probability_missing");
    }
    if input.exit_probability.is_none() {
        missing.push("exit_probability_missing");
    }
    if input.expected_net_edge.is_none() {
        missing.push("expected_net_edge_missing");
    }
    if input.round_trip_cost_fraction.is_none() {
        missing.push("cost_missing");
    }
    if input.price_impact_bps.is_none() {
        missing.push("impact_missing");
    }
    if input.liquidity_quote_atomic.is_none() {
        missing.push("liquidity_missing");
    }
    if input.creator_risk.is_none() {
        missing.push("creator_indicator_missing");
    }
    if input.wallet_risk.is_none() {
        missing.push("wallet_indicator_missing");
    }
    if input.cluster_risk.is_none() {
        missing.push("cluster_indicator_missing");
    }
    if !missing.is_empty() {
        return OpportunityDecision {
            action: OpportunityAction::Abstain,
            reasons: missing.into_iter().map(str::to_string).collect(),
        };
    }

    if input.familiarity.unwrap_or_default() <= 0.0
        || input.data_completeness.unwrap_or_default() < policy.minimum_data_completeness
    {
        return decision(OpportunityAction::Abstain, &["unfamiliar_or_incomplete"]);
    }

    let mut avoid = Vec::new();
    if input.collapse_probability.unwrap_or(1.0) > policy.maximum_collapse_probability {
        avoid.push("collapse_risk");
    }
    if input.exit_probability.unwrap_or(1.0) > policy.maximum_exit_probability_for_entry {
        avoid.push("exit_hazard");
    }
    if input.round_trip_cost_fraction.unwrap_or(1.0) > policy.maximum_round_trip_cost_fraction {
        avoid.push("cost_too_high");
    }
    if input.price_impact_bps.unwrap_or(u32::MAX) > policy.maximum_price_impact_bps {
        avoid.push("impact_too_high");
    }
    if input.liquidity_quote_atomic.unwrap_or_default() < policy.minimum_liquidity_quote_atomic {
        avoid.push("liquidity_too_low");
    }
    if input.creator_risk.unwrap_or(1.0) > policy.maximum_creator_risk {
        avoid.push("creator_risk");
    }
    if input.wallet_risk.unwrap_or(1.0) > policy.maximum_wallet_risk {
        avoid.push("wallet_risk");
    }
    if input.cluster_risk.unwrap_or(1.0) > policy.maximum_cluster_risk {
        avoid.push("cluster_risk");
    }
    if !avoid.is_empty() {
        return OpportunityDecision {
            action: OpportunityAction::Avoid,
            reasons: avoid.into_iter().map(str::to_string).collect(),
        };
    }

    let mut watch = Vec::new();
    if input.graduation_probability.unwrap_or_default() < policy.minimum_graduation_probability {
        watch.push("graduation_below_entry");
    }
    if input.momentum_probability.unwrap_or_default() < policy.minimum_momentum_probability {
        watch.push("momentum_below_entry");
    }
    if input.expected_net_edge.unwrap_or(f64::NEG_INFINITY) < policy.minimum_expected_net_edge {
        watch.push("net_edge_below_entry");
    }
    if !watch.is_empty() {
        OpportunityDecision {
            action: OpportunityAction::Watch,
            reasons: watch.into_iter().map(str::to_string).collect(),
        }
    } else {
        decision(OpportunityAction::Buy, &["all_entry_gates_passed"])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExitInput {
    pub collapse_probability: Option<f64>,
    pub exit_probability: Option<f64>,
    pub data_quality_sufficient: bool,
    pub liquidity_quote_atomic: Option<u128>,
    pub creator_sell_acceleration: Option<f64>,
    pub wallet_deterioration: Option<f64>,
    pub cluster_deterioration: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExitDecision {
    pub action: ExitAction,
    pub fraction: f64,
    pub reasons: Vec<String>,
}

pub fn evaluate_exit(policy: &AutonomousPolicy, input: &ExitInput) -> ExitDecision {
    let liquidity = input.liquidity_quote_atomic.unwrap_or_default();
    if !input.data_quality_sufficient || liquidity < policy.minimum_exit_liquidity_quote_atomic {
        return exit_decision(
            ExitAction::EmergencyExit,
            1.0,
            &["market_or_data_emergency"],
        );
    }
    if input.collapse_probability.unwrap_or_default() >= policy.emergency_collapse_probability {
        return exit_decision(ExitAction::EmergencyExit, 1.0, &["collapse_emergency"]);
    }
    let deterioration = input
        .creator_sell_acceleration
        .unwrap_or_default()
        .max(input.wallet_deterioration.unwrap_or_default())
        .max(input.cluster_deterioration.unwrap_or_default());
    if input.exit_probability.unwrap_or_default() >= policy.full_exit_probability
        || deterioration >= policy.full_exit_probability
    {
        return exit_decision(ExitAction::FullExit, 1.0, &["exit_hazard_or_deterioration"]);
    }
    if input.exit_probability.unwrap_or_default() >= policy.partial_exit_probability
        || deterioration >= policy.partial_exit_probability
    {
        return exit_decision(
            ExitAction::PartialExit,
            policy.partial_exit_fraction,
            &["partial_exit_hazard"],
        );
    }
    exit_decision(ExitAction::Hold, 0.0, &["hold_gates_passed"])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenLot {
    pub lot_id: String,
    pub remaining_base_atomic: u128,
    pub remaining_cost_atomic: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LotAllocation {
    pub lot_id: String,
    pub base_atomic: u128,
    pub allocated_cost_atomic: u128,
    pub net_proceeds_atomic: u128,
    pub realized_pnl_atomic: i128,
}

pub fn fifo_allocate(
    lots: &[OpenLot],
    sold_base_atomic: u128,
    net_proceeds_atomic: u128,
) -> Result<Vec<LotAllocation>, String> {
    let available = lots
        .iter()
        .try_fold(0u128, |sum, lot| sum.checked_add(lot.remaining_base_atomic))
        .ok_or_else(|| "lot_quantity_overflow".to_string())?;
    if sold_base_atomic == 0 || sold_base_atomic > available {
        return Err("insufficient_fifo_lots".into());
    }
    let mut remaining = sold_base_atomic;
    let mut proceeds_left = net_proceeds_atomic;
    let mut out = Vec::new();
    for lot in lots {
        if remaining == 0 {
            break;
        }
        if lot.remaining_base_atomic == 0 {
            continue;
        }
        let consumed = remaining.min(lot.remaining_base_atomic);
        let cost = if consumed == lot.remaining_base_atomic {
            lot.remaining_cost_atomic
        } else {
            lot.remaining_cost_atomic
                .checked_mul(consumed)
                .ok_or_else(|| "lot_cost_overflow".to_string())?
                / lot.remaining_base_atomic
        };
        let proceeds = if consumed == remaining {
            proceeds_left
        } else {
            net_proceeds_atomic
                .checked_mul(consumed)
                .ok_or_else(|| "proceeds_overflow".to_string())?
                / sold_base_atomic
        };
        let realized = i128::try_from(proceeds)
            .and_then(|p| i128::try_from(cost).map(|c| p - c))
            .map_err(|_| "realized_pnl_overflow".to_string())?;
        out.push(LotAllocation {
            lot_id: lot.lot_id.clone(),
            base_atomic: consumed,
            allocated_cost_atomic: cost,
            net_proceeds_atomic: proceeds,
            realized_pnl_atomic: realized,
        });
        remaining -= consumed;
        proceeds_left = proceeds_left.saturating_sub(proceeds);
    }
    if remaining != 0 {
        return Err("insufficient_fifo_lots".into());
    }
    Ok(out)
}

fn decision(action: OpportunityAction, reasons: &[&str]) -> OpportunityDecision {
    OpportunityDecision {
        action,
        reasons: reasons.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn exit_decision(action: ExitAction, fraction: f64, reasons: &[&str]) -> ExitDecision {
    ExitDecision {
        action,
        fraction,
        reasons: reasons.iter().map(|s| (*s).to_string()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> AutonomousPolicy {
        AutonomousPolicy {
            schema_version: 1,
            policy_version: "test-v1".into(),
            minimum_familiarity: 0.5,
            minimum_data_completeness: 0.8,
            minimum_graduation_probability: 0.65,
            minimum_momentum_probability: 0.6,
            maximum_collapse_probability: 0.2,
            maximum_exit_probability_for_entry: 0.3,
            minimum_expected_net_edge: 0.03,
            maximum_round_trip_cost_fraction: 0.06,
            maximum_price_impact_bps: 100,
            minimum_liquidity_quote_atomic: 1_000,
            maximum_creator_risk: 0.4,
            maximum_wallet_risk: 0.4,
            maximum_cluster_risk: 0.4,
            partial_exit_probability: 0.4,
            full_exit_probability: 0.7,
            emergency_collapse_probability: 0.85,
            minimum_exit_liquidity_quote_atomic: 500,
            partial_exit_fraction: 0.5,
            arrival_delay_ms: 1_000,
            maximum_arrival_wait_ms: 5_000,
        }
    }

    fn eligible() -> OpportunityInput {
        OpportunityInput {
            champion_present: true,
            calibrated: true,
            familiarity: Some(0.9),
            data_completeness: Some(0.95),
            quality_sufficient: true,
            graduation_probability: Some(0.8),
            momentum_probability: Some(0.75),
            collapse_probability: Some(0.1),
            exit_probability: Some(0.15),
            expected_net_edge: Some(0.08),
            round_trip_cost_fraction: Some(0.03),
            price_impact_bps: Some(50),
            liquidity_quote_atomic: Some(10_000),
            creator_risk: Some(0.1),
            wallet_risk: Some(0.1),
            cluster_risk: Some(0.1),
        }
    }

    #[test]
    fn validates_state_machine() {
        assert!(DurableOrderStatus::Decided.can_transition_to(DurableOrderStatus::Queued));
        assert!(DurableOrderStatus::Queued.can_transition_to(DurableOrderStatus::Arrived));
        assert!(DurableOrderStatus::Arrived.can_transition_to(DurableOrderStatus::Filled));
        assert!(DurableOrderStatus::Filled.can_transition_to(DurableOrderStatus::Position));
        assert!(!DurableOrderStatus::Decided.can_transition_to(DurableOrderStatus::Filled));
        assert!(!DurableOrderStatus::Rejected.can_transition_to(DurableOrderStatus::Filled));
    }

    #[test]
    fn withheld_model_abstains_without_heuristic_fallback() {
        let mut input = eligible();
        input.familiarity = None;
        input.graduation_probability = None;
        let result = evaluate_opportunity(Some(&policy()), &input);
        assert_eq!(result.action, OpportunityAction::Abstain);
        assert!(result.reasons.contains(&"familiarity_withheld".into()));
    }

    #[test]
    fn absent_policy_abstains() {
        assert_eq!(
            evaluate_opportunity(None, &eligible()).action,
            OpportunityAction::Abstain
        );
    }

    #[test]
    fn opportunity_actions_are_distinct() {
        assert_eq!(
            evaluate_opportunity(Some(&policy()), &eligible()).action,
            OpportunityAction::Buy
        );
        let mut watch = eligible();
        watch.momentum_probability = Some(0.2);
        assert_eq!(
            evaluate_opportunity(Some(&policy()), &watch).action,
            OpportunityAction::Watch
        );
        let mut avoid = eligible();
        avoid.collapse_probability = Some(0.9);
        assert_eq!(
            evaluate_opportunity(Some(&policy()), &avoid).action,
            OpportunityAction::Avoid
        );
    }

    #[test]
    fn exit_actions_follow_precedence() {
        let p = policy();
        assert_eq!(
            evaluate_exit(
                &p,
                &ExitInput {
                    data_quality_sufficient: false,
                    liquidity_quote_atomic: Some(10_000),
                    ..Default::default()
                }
            )
            .action,
            ExitAction::EmergencyExit
        );
        assert_eq!(
            evaluate_exit(
                &p,
                &ExitInput {
                    data_quality_sufficient: true,
                    liquidity_quote_atomic: Some(10_000),
                    exit_probability: Some(0.8),
                    ..Default::default()
                }
            )
            .action,
            ExitAction::FullExit
        );
        assert_eq!(
            evaluate_exit(
                &p,
                &ExitInput {
                    data_quality_sufficient: true,
                    liquidity_quote_atomic: Some(10_000),
                    exit_probability: Some(0.5),
                    ..Default::default()
                }
            )
            .action,
            ExitAction::PartialExit
        );
    }

    #[test]
    fn fifo_realized_pnl_is_exact_and_ordered() {
        let lots = vec![
            OpenLot {
                lot_id: "first".into(),
                remaining_base_atomic: 100,
                remaining_cost_atomic: 200,
            },
            OpenLot {
                lot_id: "second".into(),
                remaining_base_atomic: 100,
                remaining_cost_atomic: 400,
            },
        ];
        let allocations = fifo_allocate(&lots, 150, 750).unwrap();
        assert_eq!(allocations[0].lot_id, "first");
        assert_eq!(allocations[0].realized_pnl_atomic, 300);
        assert_eq!(allocations[1].allocated_cost_atomic, 200);
        assert_eq!(allocations[1].realized_pnl_atomic, 50);
        assert_eq!(
            allocations
                .iter()
                .map(|a| a.net_proceeds_atomic)
                .sum::<u128>(),
            750
        );
    }
}
