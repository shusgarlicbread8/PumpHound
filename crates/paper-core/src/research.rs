use crate::{
    evaluate_heuristic_opportunity, HeuristicOpportunityInput, HeuristicPolicyV1,
    OpportunityAction, OpportunityDecision,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STRATEGY_CONTROL_FLOW_V1: &str = "CONTROL_FLOW_V1";
pub const STRATEGY_HEURISTIC_V1: &str = "HEURISTIC_V1";
pub const STRATEGY_RESEARCH_EXPLORATION_V1: &str = "RESEARCH_EXPLORATION_V1";
pub const STRATEGY_HEURISTIC_GRAPH_AI_V1: &str = "HEURISTIC_GRAPH_AI_V1";
pub const STRATEGY_MARKET_ONLY_SHADOW: &str = "MARKET_ONLY_SHADOW";
pub const STRATEGY_FULL_ML_SHADOW: &str = "FULL_ML_SHADOW";
pub const SHARED_UNIVERSE_VERSION: &str = "SHARED_ELIGIBLE_V1";
pub const RESEARCH_FILTER_TIMEOUT_SECONDS: f64 = 45.0;
pub const SHADOW_MAXIMUM_EXIT_HAZARD: f64 = 0.45;
pub const SHADOW_MINIMUM_SHORT_MOMENTUM: f64 = 0.55;
pub const SHADOW_MAXIMUM_ADVERSE_MOVE: f64 = 0.50;
pub const EXIT_HAZARD_TARGET: &str = "exit_hazard_60s_v1";
pub const SHORT_MOMENTUM_TARGET: &str = "short_momentum_2m_v1";
pub const ADVERSE_MOVE_TARGET: &str = "adverse_move_2m_v1";
pub const EXPECTED_MFE_TARGET: &str = "expected_mfe_5m_v1";
pub const DEFAULT_EXPERIMENT_ID: &str = "ALPHA-LOOP-20260908";
pub const FEATURE_VERSION: &str = "features-v1";
pub const FEE_MODEL_VERSION: &str = "paper-pump-v1";
pub const LATENCY_CONFIG_VERSION: &str = "async-enrichment-v1";
const LAMPORTS_PER_SOL: i128 = 1_000_000_000;
const SHARED_MAXIMUM_AGE_SECONDS: f64 = 900.0;
const SHARED_MINIMUM_LIQUIDITY_QUOTE_ATOMIC: u128 = 1_000_000_000;
const GRAPH_TOP1_AVOID: f64 = 0.65;
const GRAPH_CLUSTER_HHI_AVOID: f64 = 0.55;
const GRAPH_CREATOR_LINKED_AVOID: f64 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrategyBook {
    pub key: &'static str,
    pub desk_name: &'static str,
    pub display_name: &'static str,
    pub uses_research_filter: bool,
    pub uses_shadow_ml_gate: bool,
    pub uses_control_flow: bool,
    pub uses_exploration_policy: bool,
    pub uses_local_graph_gate: bool,
    pub allows_enrichment: bool,
    pub requires_shadow_mode: bool,
    pub ml_status: &'static str,
    pub execution_authority: &'static str,
    pub ai_status: &'static str,
    pub capital_sol_atomic: i128,
}

pub const CONTROL_FLOW_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_CONTROL_FLOW_V1,
    desk_name: "CONTROL",
    display_name: "CONTROL",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: true,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "NOT_TRAINED",
    execution_authority: "CONTROL_FLOW_V1",
    ai_status: "NONE",
    capital_sol_atomic: 25 * LAMPORTS_PER_SOL,
};

pub const HEURISTIC_V1_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_HEURISTIC_V1,
    desk_name: "HEURISTIC",
    display_name: "HEURISTIC",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: true,
    requires_shadow_mode: false,
    ml_status: "NOT_TRAINED",
    execution_authority: "HEURISTIC_V1",
    ai_status: "ADVISORY",
    capital_sol_atomic: 25 * LAMPORTS_PER_SOL,
};

pub const RESEARCH_EXPLORATION_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_RESEARCH_EXPLORATION_V1,
    desk_name: "EXPLORATION",
    display_name: "EXPLORATION",
    uses_research_filter: false,
    uses_shadow_ml_gate: false,
    uses_control_flow: false,
    uses_exploration_policy: true,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: false,
    ml_status: "NOT_TRAINED",
    execution_authority: "RESEARCH_EXPLORATION_V1",
    ai_status: "NONE",
    capital_sol_atomic: 100 * LAMPORTS_PER_SOL,
};

pub const HEURISTIC_GRAPH_AI_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_HEURISTIC_GRAPH_AI_V1,
    desk_name: "GRAPH + AI",
    display_name: "GRAPH + AI",
    uses_research_filter: true,
    uses_shadow_ml_gate: false,
    uses_control_flow: true,
    uses_exploration_policy: false,
    uses_local_graph_gate: true,
    allows_enrichment: true,
    requires_shadow_mode: false,
    ml_status: "NOT_TRAINED",
    execution_authority: "LOCAL_GRAPH",
    ai_status: "ASYNC_ADVISORY",
    capital_sol_atomic: 25 * LAMPORTS_PER_SOL,
};

pub const MARKET_ONLY_SHADOW_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_MARKET_ONLY_SHADOW,
    desk_name: "MARKET ML",
    display_name: "MARKET ML",
    uses_research_filter: false,
    uses_shadow_ml_gate: true,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: false,
    allows_enrichment: false,
    requires_shadow_mode: true,
    ml_status: "SHADOW",
    execution_authority: "SHADOW_VIRTUAL",
    ai_status: "NONE",
    capital_sol_atomic: 25 * LAMPORTS_PER_SOL,
};

pub const FULL_ML_SHADOW_BOOK: StrategyBook = StrategyBook {
    key: STRATEGY_FULL_ML_SHADOW,
    desk_name: "FULL PUMPHOUND",
    display_name: "FULL PUMPHOUND",
    uses_research_filter: true,
    uses_shadow_ml_gate: true,
    uses_control_flow: false,
    uses_exploration_policy: false,
    uses_local_graph_gate: true,
    allows_enrichment: true,
    requires_shadow_mode: true,
    ml_status: "SHADOW",
    execution_authority: "SHADOW_LOCAL_GRAPH",
    ai_status: "ASYNC_ADVISORY",
    capital_sol_atomic: 25 * LAMPORTS_PER_SOL,
};

#[derive(Clone, Debug)]
pub struct ResearchFilterInput {
    pub investigation_ready: bool,
    pub intent_age_seconds: f64,
    pub grounded: bool,
    pub provider_unavailable: bool,
    pub degraded: bool,
    pub coordination_hypotheses_with_evidence: usize,
    pub creator_risks: usize,
    pub wallet_risks: usize,
    pub shared_inferred_funder_wallets: usize,
    pub timeout_seconds: f64,
}

#[derive(Clone, Debug)]
pub struct ShadowMlGateInput {
    pub exit_hazard: Option<f64>,
    pub short_momentum: Option<f64>,
    pub adverse_move: Option<f64>,
    pub maximum_exit_hazard: f64,
    pub minimum_short_momentum: f64,
    pub maximum_adverse_move: f64,
}

impl ShadowMlGateInput {
    pub fn from_exit_hazard(exit_hazard: Option<f64>, maximum_exit_hazard: f64) -> Self {
        Self {
            exit_hazard,
            short_momentum: None,
            adverse_move: None,
            maximum_exit_hazard,
            minimum_short_momentum: SHADOW_MINIMUM_SHORT_MOMENTUM,
            maximum_adverse_move: SHADOW_MAXIMUM_ADVERSE_MOVE,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalGraphInput {
    pub cluster_hhi: Option<f64>,
    pub holder_top_1_share: Option<f64>,
    pub creator_linked_supply_share: Option<f64>,
    pub creator_risk: Option<f64>,
    pub wallet_risk: Option<f64>,
    pub cluster_risk: Option<f64>,
}

pub fn parallel_books(shadow_ml: bool) -> Vec<StrategyBook> {
    let mut books = vec![
        CONTROL_FLOW_BOOK,
        HEURISTIC_V1_BOOK,
        RESEARCH_EXPLORATION_BOOK,
        HEURISTIC_GRAPH_AI_BOOK,
    ];
    if shadow_ml {
        books.push(MARKET_ONLY_SHADOW_BOOK);
        books.push(FULL_ML_SHADOW_BOOK);
    }
    books
}

pub fn book_by_key(key: &str) -> Option<StrategyBook> {
    [
        CONTROL_FLOW_BOOK,
        HEURISTIC_V1_BOOK,
        RESEARCH_EXPLORATION_BOOK,
        HEURISTIC_GRAPH_AI_BOOK,
        MARKET_ONLY_SHADOW_BOOK,
        FULL_ML_SHADOW_BOOK,
    ]
    .into_iter()
    .find(|book| book.key == key)
}

pub fn evaluate_shared_universe(input: &HeuristicOpportunityInput) -> OpportunityDecision {
    let mut missing = Vec::new();
    if input.age_seconds.is_none() {
        missing.push("age_unavailable");
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
        return decision(
            OpportunityAction::Abstain,
            missing.into_iter().map(str::to_string).collect(),
        );
    }
    if input.age_seconds.unwrap_or_default() > SHARED_MAXIMUM_AGE_SECONDS {
        return decision(
            OpportunityAction::Avoid,
            vec!["maximum_age_exceeded".into()],
        );
    }
    if input.liquidity_quote_atomic.unwrap_or_default() < SHARED_MINIMUM_LIQUIDITY_QUOTE_ATOMIC {
        return decision(
            OpportunityAction::Avoid,
            vec!["liquidity_below_shared_minimum".into()],
        );
    }
    decision(
        OpportunityAction::Buy,
        vec!["shared_universe_eligible".into()],
    )
}

pub fn evaluate_local_graph(input: &LocalGraphInput) -> OpportunityDecision {
    let mut avoid = Vec::new();
    if input
        .holder_top_1_share
        .is_some_and(|value| value > GRAPH_TOP1_AVOID)
    {
        avoid.push("cluster_concentration");
    }
    if input
        .cluster_hhi
        .is_some_and(|value| value > GRAPH_CLUSTER_HHI_AVOID)
    {
        avoid.push("cluster_concentration");
    }
    if input
        .creator_linked_supply_share
        .is_some_and(|value| value > GRAPH_CREATOR_LINKED_AVOID)
    {
        avoid.push("creator_risk");
    }
    let risk_sum = input.creator_risk.unwrap_or(0.0)
        + input.wallet_risk.unwrap_or(0.0)
        + input.cluster_risk.unwrap_or(0.0);
    if risk_sum >= 2.4 {
        avoid.push("graph_risk_concentration");
    }
    if !avoid.is_empty() {
        avoid.sort();
        avoid.dedup();
        return decision(
            OpportunityAction::Avoid,
            avoid.into_iter().map(str::to_string).collect(),
        );
    }
    if input.cluster_hhi.is_none()
        && input.holder_top_1_share.is_none()
        && input.creator_linked_supply_share.is_none()
        && input.creator_risk.is_none()
        && input.wallet_risk.is_none()
        && input.cluster_risk.is_none()
    {
        return decision(
            OpportunityAction::Buy,
            vec!["local_graph_unavailable_not_blocking".into()],
        );
    }
    decision(OpportunityAction::Buy, vec!["local_graph_accepted".into()])
}

pub fn local_graph_from_indicators(indicators: &Value) -> LocalGraphInput {
    LocalGraphInput {
        cluster_hhi: json_f64(indicators, "cluster_hhi"),
        holder_top_1_share: json_f64(indicators, "holder_top_1_share"),
        creator_linked_supply_share: json_f64(indicators, "creator_linked_supply_share"),
        creator_risk: json_f64(indicators, "creator_risk"),
        wallet_risk: json_f64(indicators, "wallet_risk"),
        cluster_risk: json_f64(indicators, "cluster_risk"),
    }
}

pub fn evaluate_independent_book(
    book: StrategyBook,
    heuristic_policy: &HeuristicPolicyV1,
    exploration_policy: &HeuristicPolicyV1,
    control_policy: &HeuristicPolicyV1,
    input: &HeuristicOpportunityInput,
    graph: &LocalGraphInput,
    shadow: Option<&ShadowMlGateInput>,
) -> OpportunityDecision {
    if book.uses_exploration_policy {
        return evaluate_heuristic_opportunity(exploration_policy, input);
    }
    if book.key == STRATEGY_CONTROL_FLOW_V1 {
        return evaluate_heuristic_opportunity(control_policy, input);
    }
    if book.key == STRATEGY_HEURISTIC_V1 {
        return evaluate_heuristic_opportunity(heuristic_policy, input);
    }

    let universe = evaluate_shared_universe(input);
    if universe.action != OpportunityAction::Buy {
        return universe;
    }

    let mut reasons = universe.reasons;
    if book.uses_local_graph_gate {
        let graph_decision = evaluate_local_graph(graph);
        if graph_decision.action != OpportunityAction::Buy {
            return graph_decision;
        }
        reasons.extend(graph_decision.reasons);
    }

    if book.uses_shadow_ml_gate {
        let gated = evaluate_shadow_ml_gate(shadow.unwrap_or(&ShadowMlGateInput {
            exit_hazard: None,
            short_momentum: None,
            adverse_move: None,
            maximum_exit_hazard: SHADOW_MAXIMUM_EXIT_HAZARD,
            minimum_short_momentum: SHADOW_MINIMUM_SHORT_MOMENTUM,
            maximum_adverse_move: SHADOW_MAXIMUM_ADVERSE_MOVE,
        }));
        let mut combined = reasons;
        combined.extend(gated.reasons);
        return OpportunityDecision {
            action: gated.action,
            reasons: combined,
        };
    }

    if book.uses_control_flow {
        let market = evaluate_heuristic_opportunity(control_policy, input);
        let mut combined = reasons;
        combined.extend(market.reasons);
        return OpportunityDecision {
            action: market.action,
            reasons: combined,
        };
    }

    decision(OpportunityAction::Abstain, reasons)
}

pub fn evaluate_shadow_ml_gate(input: &ShadowMlGateInput) -> OpportunityDecision {
    if input
        .adverse_move
        .is_some_and(|value| value.is_finite() && value > input.maximum_adverse_move)
    {
        return decision(
            OpportunityAction::Avoid,
            vec!["shadow_adverse_move_above_threshold".into()],
        );
    }
    if let Some(hazard) = input.exit_hazard.filter(|value| value.is_finite()) {
        if hazard > input.maximum_exit_hazard {
            return decision(
                OpportunityAction::Avoid,
                vec!["shadow_exit_hazard_above_threshold".into()],
            );
        }
    }
    if input
        .short_momentum
        .is_some_and(|value| value.is_finite() && value >= input.minimum_short_momentum)
    {
        return decision(
            OpportunityAction::Buy,
            vec!["shadow_short_momentum_accepted".into()],
        );
    }
    let Some(hazard) = input.exit_hazard.filter(|value| value.is_finite()) else {
        return decision(
            OpportunityAction::Abstain,
            vec!["shadow_exit_hazard_unavailable".into()],
        );
    };
    if hazard > input.maximum_exit_hazard {
        return decision(
            OpportunityAction::Avoid,
            vec!["shadow_exit_hazard_above_threshold".into()],
        );
    }
    decision(
        OpportunityAction::Buy,
        vec!["shadow_exit_hazard_accepted".into()],
    )
}

pub fn evaluate_research_filter(input: &ResearchFilterInput) -> OpportunityDecision {
    if !input.investigation_ready {
        if input.intent_age_seconds >= input.timeout_seconds {
            return decision(OpportunityAction::Avoid, vec!["research_timeout".into()]);
        }
        return decision(
            OpportunityAction::Watch,
            vec!["research_filter_pending".into()],
        );
    }
    if input.provider_unavailable {
        return decision(
            OpportunityAction::Avoid,
            vec!["research_investigator_unavailable".into()],
        );
    }
    if !input.grounded || input.degraded {
        return decision(
            OpportunityAction::Watch,
            vec!["research_not_grounded".into()],
        );
    }
    if input.shared_inferred_funder_wallets >= 3 {
        return decision(
            OpportunityAction::Avoid,
            vec!["research_shared_inferred_funder".into()],
        );
    }
    if input.coordination_hypotheses_with_evidence >= 2 {
        return decision(
            OpportunityAction::Avoid,
            vec!["research_coordination_evidence".into()],
        );
    }
    if input.creator_risks + input.wallet_risks >= 3 {
        return decision(
            OpportunityAction::Avoid,
            vec!["research_risk_concentration".into()],
        );
    }
    decision(
        OpportunityAction::Buy,
        vec!["research_filter_passed".into()],
    )
}

pub fn research_input_from_artifacts(
    investigation: Option<&Value>,
    intent_age_seconds: f64,
    shared_inferred_funder_wallets: usize,
) -> ResearchFilterInput {
    let Some(investigation) = investigation else {
        return ResearchFilterInput {
            investigation_ready: false,
            intent_age_seconds,
            grounded: false,
            provider_unavailable: false,
            degraded: false,
            coordination_hypotheses_with_evidence: 0,
            creator_risks: 0,
            wallet_risks: 0,
            shared_inferred_funder_wallets,
            timeout_seconds: RESEARCH_FILTER_TIMEOUT_SECONDS,
        };
    };
    let validation = investigation
        .get("validation")
        .cloned()
        .unwrap_or(Value::Null);
    let provider = investigation
        .get("provider")
        .and_then(Value::as_str)
        .unwrap_or("");
    let advisory = advisory_payload(investigation);
    ResearchFilterInput {
        investigation_ready: true,
        intent_age_seconds,
        grounded: validation.get("grounded").and_then(Value::as_bool) == Some(true),
        provider_unavailable: provider == "unavailable"
            || validation.get("error_code").and_then(Value::as_str)
                == Some("INVESTIGATOR_UNAVAILABLE"),
        degraded: advisory
            .and_then(|value| value.get("degraded"))
            .and_then(Value::as_bool)
            == Some(true)
            || role_degraded(investigation),
        coordination_hypotheses_with_evidence: count_supported_claims(
            advisory
                .and_then(|value| value.get("coordination_hypotheses"))
                .or_else(|| {
                    investigation
                        .pointer("/response/roles/committee/advisory/coordination_hypotheses")
                }),
        ),
        creator_risks: count_strings(
            advisory
                .and_then(|value| value.get("creator_risks"))
                .or_else(|| {
                    investigation.pointer("/response/roles/committee/advisory/creator_risks")
                }),
        ),
        wallet_risks: count_strings(
            advisory
                .and_then(|value| value.get("wallet_risks"))
                .or_else(|| {
                    investigation.pointer("/response/roles/committee/advisory/wallet_risks")
                }),
        ),
        shared_inferred_funder_wallets,
        timeout_seconds: RESEARCH_FILTER_TIMEOUT_SECONDS,
    }
}

pub fn investigation_features(investigation: Option<&Value>) -> Value {
    let Some(investigation) = investigation else {
        return Value::Null;
    };
    let advisory = advisory_payload(investigation)
        .cloned()
        .unwrap_or(Value::Null);
    let validation = investigation
        .get("validation")
        .cloned()
        .unwrap_or(Value::Null);
    serde_json::json!({
        "coordination_risk": count_supported_claims(advisory.get("coordination_hypotheses")),
        "creator_risk": count_strings(advisory.get("creator_risks")),
        "wallet_risk": count_strings(advisory.get("wallet_risks")),
        "organic_demand_confidence": json_f64(&advisory, "organic_demand_confidence"),
        "contradictory_evidence_count": count_supported_claims(advisory.get("contradictions"))
            + count_strings(advisory.get("contradictory_evidence")),
        "missing_information_severity": json_f64(&advisory, "missing_information_severity")
            .or_else(|| json_f64(&validation, "missing_information_severity")),
        "investigation_confidence": json_f64(&advisory, "confidence")
            .or_else(|| json_f64(&advisory, "investigation_confidence")),
        "grounded": validation.get("grounded").and_then(Value::as_bool),
        "degraded": advisory.get("degraded").and_then(Value::as_bool),
        "latency_ms": json_f64(&validation, "latency_ms")
            .or_else(|| json_f64(investigation, "latency_ms"))
    })
}

pub fn shared_inferred_funder_wallets(dossier: &Value) -> usize {
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    collect_funders(dossier, &mut counts);
    counts.into_values().filter(|count| *count >= 2).sum()
}

pub fn funnel_stage_for_reasons(reasons: &[String]) -> &'static str {
    if reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "age_unavailable" | "data_quality_insufficient" | "executable_cost_unavailable"
        )
    }) {
        return "STRUCTURAL";
    }
    if reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "liquidity_below_minimum"
                | "liquidity_below_shared_minimum"
                | "entry_impact_too_high"
                | "round_trip_cost_too_high"
        )
    }) {
        return "LIQUIDITY";
    }
    if reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "buyers_below_minimum"
                | "buy_imbalance_below_minimum"
                | "return_out_of_band"
                | "curve_progress_out_of_band"
                | "minimum_age_not_reached"
        )
    }) {
        return "MARKET_SIGNAL";
    }
    if reasons.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "cluster_concentration"
                | "creator_risk"
                | "graph_risk_concentration"
                | "research_coordination_evidence"
                | "research_shared_inferred_funder"
                | "research_risk_concentration"
        )
    }) {
        return "GRAPH";
    }
    if reasons.iter().any(|reason| reason.starts_with("shadow_")) {
        return "MODEL";
    }
    if reasons.iter().any(|reason| reason.starts_with("research_")) {
        return "AI";
    }
    "DECISION"
}

fn advisory_payload(investigation: &Value) -> Option<&Value> {
    investigation
        .pointer("/response/roles/committee/advisory")
        .or_else(|| investigation.pointer("/roles/committee/advisory"))
        .or_else(|| investigation.get("advisory"))
}

fn role_degraded(investigation: &Value) -> bool {
    investigation
        .pointer("/response/roles/committee/degraded")
        .and_then(Value::as_bool)
        == Some(true)
        || investigation
            .pointer("/roles/committee/degraded")
            .and_then(Value::as_bool)
            == Some(true)
}

fn count_supported_claims(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    item.get("supporting_evidence_ids")
                        .and_then(Value::as_array)
                        .is_some_and(|ids| !ids.is_empty())
                })
                .count()
        })
        .unwrap_or(0)
}

fn count_strings(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.as_str().is_some_and(|text| !text.trim().is_empty()))
                .count()
        })
        .unwrap_or(0)
}

fn collect_funders(value: &Value, counts: &mut std::collections::BTreeMap<String, usize>) {
    match value {
        Value::Object(map) => {
            let funder = map
                .get("funder")
                .and_then(Value::as_str)
                .or_else(|| {
                    map.get("funding_inference")
                        .and_then(|item| item.get("funder"))
                        .and_then(Value::as_str)
                })
                .or_else(|| map.get("inferred_initial_funder").and_then(Value::as_str));
            if map.contains_key("wallet") || map.contains_key("address") {
                if let Some(funder) = funder.filter(|value| !value.is_empty()) {
                    *counts.entry(funder.to_string()).or_insert(0) += 1;
                }
                return;
            }
            for nested in map.values() {
                collect_funders(nested, counts);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_funders(nested, counts);
            }
        }
        _ => {}
    }
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|item| {
        item.as_f64()
            .or_else(|| item.as_i64().map(|number| number as f64))
            .or_else(|| item.as_str().and_then(|text| text.parse().ok()))
    })
}

fn decision(action: OpportunityAction, reasons: Vec<String>) -> OpportunityDecision {
    OpportunityDecision { action, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn heuristic_policy() -> HeuristicPolicyV1 {
        policy_from("HEURISTIC_V1", 25, 0.2, 0.05, 0.5, 5, 50_000_000)
    }

    fn control_policy() -> HeuristicPolicyV1 {
        policy_from("CONTROL_FLOW_V1", 3, 0.0001, 0.0001, 5.0, 8, 50_000_000)
    }

    fn exploration_policy() -> HeuristicPolicyV1 {
        policy_from(
            "RESEARCH_EXPLORATION_V1",
            3,
            0.05,
            0.0,
            1.25,
            20,
            10_000_000,
        )
    }

    fn policy_from(
        version: &str,
        buyers: u64,
        imbalance: f64,
        min_return: f64,
        max_return: f64,
        positions: usize,
        size: u128,
    ) -> HeuristicPolicyV1 {
        let value = json!({
            "schema_version": 1,
            "strategy_version": version,
            "threshold_rationale": "unit test frozen policy",
            "scout_selection_rate": 1.0,
            "scout_cooldown_seconds": 60,
            "minimum_scout_cooldown_seconds": 30,
            "maximum_scout_cooldown_seconds": 300,
            "minimum_age_seconds": 10.0,
            "maximum_age_seconds": 900.0,
            "minimum_unique_buyers": buyers,
            "minimum_buy_sell_imbalance": imbalance,
            "minimum_return_30s": min_return,
            "maximum_return_30s": max_return,
            "minimum_curve_progress": null,
            "maximum_curve_progress": null,
            "minimum_curve_velocity_per_second": null,
            "minimum_data_quality_score": 0.5,
            "minimum_liquidity_quote_atomic": 1000,
            "maximum_entry_impact_bps": 200,
            "maximum_round_trip_cost_fraction": 0.12,
            "maximum_positions": positions,
            "fixed_sol_atomic": size,
            "arrival_delay_ms": 1000,
            "maximum_arrival_wait_ms": 5000,
            "maximum_hold_seconds": 180,
            "take_profit_fraction": 0.35,
            "stop_loss_fraction": 0.2,
            "trailing_drawdown_fraction": 0.15,
            "buy_flow_deterioration_threshold": 0.7,
            "sell_acceleration_threshold": 0.8,
            "imbalance_reversal_threshold": -0.25,
            "emergency_minimum_liquidity_quote_atomic": 500,
            "emergency_maximum_impact_bps": 500,
            "partial_exit_fraction": 0.5
        });
        HeuristicPolicyV1::from_json(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    fn eligible_input() -> HeuristicOpportunityInput {
        HeuristicOpportunityInput {
            age_seconds: Some(60.0),
            unique_buyers: Some(8),
            buy_sell_imbalance: Some(0.15),
            return_30s: Some(0.03),
            curve_progress: Some(0.4),
            data_quality_score: Some(0.9),
            quality_sufficient: true,
            liquidity_quote_atomic: Some(10_000_000_000),
            entry_impact_bps: Some(40),
            round_trip_cost_fraction: Some(0.03),
            scout_selected: true,
            ..Default::default()
        }
    }

    #[test]
    fn heuristic_mode_runs_control_heuristic_exploration_and_graph() {
        let books = parallel_books(false);
        assert_eq!(
            books.iter().map(|book| book.key).collect::<Vec<_>>(),
            vec![
                STRATEGY_CONTROL_FLOW_V1,
                STRATEGY_HEURISTIC_V1,
                STRATEGY_RESEARCH_EXPLORATION_V1,
                STRATEGY_HEURISTIC_GRAPH_AI_V1
            ]
        );
        let shadow = parallel_books(true);
        assert_eq!(shadow.len(), 6);
        assert!(shadow
            .iter()
            .any(|book| book.key == STRATEGY_MARKET_ONLY_SHADOW));
        assert!(shadow
            .iter()
            .any(|book| book.key == STRATEGY_FULL_ML_SHADOW));
    }

    #[test]
    fn books_decide_independently_of_heuristic_buy() {
        let input = eligible_input();
        let heuristic = evaluate_independent_book(
            HEURISTIC_V1_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &input,
            &LocalGraphInput::default(),
            None,
        );
        assert_eq!(heuristic.action, OpportunityAction::Watch);

        let control = evaluate_independent_book(
            CONTROL_FLOW_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &input,
            &LocalGraphInput::default(),
            None,
        );
        assert_eq!(control.action, OpportunityAction::Buy);

        let exploration = evaluate_independent_book(
            RESEARCH_EXPLORATION_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &input,
            &LocalGraphInput::default(),
            None,
        );
        assert_eq!(exploration.action, OpportunityAction::Buy);

        let market = evaluate_independent_book(
            MARKET_ONLY_SHADOW_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &input,
            &LocalGraphInput::default(),
            Some(&ShadowMlGateInput::from_exit_hazard(
                Some(0.2),
                SHADOW_MAXIMUM_EXIT_HAZARD,
            )),
        );
        assert_eq!(market.action, OpportunityAction::Buy);
    }

    #[test]
    fn market_ml_can_buy_a_token_the_heuristic_only_watched() {
        let input = eligible_input();
        let heuristic = evaluate_heuristic_opportunity(&heuristic_policy(), &input);
        assert_eq!(heuristic.action, OpportunityAction::Watch);
        let market = evaluate_independent_book(
            MARKET_ONLY_SHADOW_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &input,
            &LocalGraphInput::default(),
            Some(&ShadowMlGateInput::from_exit_hazard(
                Some(0.11),
                SHADOW_MAXIMUM_EXIT_HAZARD,
            )),
        );
        assert_eq!(market.action, OpportunityAction::Buy);
    }

    #[test]
    fn graph_book_does_not_wait_for_siliconflow() {
        let decision = evaluate_independent_book(
            HEURISTIC_GRAPH_AI_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &eligible_input(),
            &LocalGraphInput::default(),
            None,
        );
        assert_eq!(decision.action, OpportunityAction::Buy);
        assert!(!decision
            .reasons
            .iter()
            .any(|reason| reason == "research_filter_pending"));
    }

    #[test]
    fn local_graph_can_veto_without_llm() {
        let decision = evaluate_independent_book(
            HEURISTIC_GRAPH_AI_BOOK,
            &heuristic_policy(),
            &exploration_policy(),
            &control_policy(),
            &eligible_input(),
            &LocalGraphInput {
                holder_top_1_share: Some(0.91),
                ..Default::default()
            },
            None,
        );
        assert_eq!(decision.action, OpportunityAction::Avoid);
        assert!(decision
            .reasons
            .iter()
            .any(|reason| reason == "cluster_concentration"));
    }

    #[test]
    fn research_filter_waits_then_times_out() {
        let pending = evaluate_research_filter(&ResearchFilterInput {
            investigation_ready: false,
            intent_age_seconds: 10.0,
            grounded: false,
            provider_unavailable: false,
            degraded: false,
            coordination_hypotheses_with_evidence: 0,
            creator_risks: 0,
            wallet_risks: 0,
            shared_inferred_funder_wallets: 0,
            timeout_seconds: 45.0,
        });
        assert_eq!(pending.action, OpportunityAction::Watch);
        let timed_out = evaluate_research_filter(&ResearchFilterInput {
            investigation_ready: false,
            intent_age_seconds: 45.0,
            grounded: false,
            provider_unavailable: false,
            degraded: false,
            coordination_hypotheses_with_evidence: 0,
            creator_risks: 0,
            wallet_risks: 0,
            shared_inferred_funder_wallets: 0,
            timeout_seconds: 45.0,
        });
        assert_eq!(timed_out.action, OpportunityAction::Avoid);
        assert!(timed_out
            .reasons
            .iter()
            .any(|reason| reason == "research_timeout"));
    }

    #[test]
    fn research_filter_vetoes_on_structured_evidence_not_llm_orders() {
        let veto = evaluate_research_filter(&research_input_from_artifacts(
            Some(&json!({
                "provider": "siliconflow",
                "validation": {"grounded": true, "policy_effect": false},
                "response": {"roles": {"committee": {"advisory": {
                    "coordination_hypotheses": [
                        {"claim": "shared funder", "supporting_evidence_ids": ["e1"], "counter_evidence_ids": [], "uncertainty": "medium"},
                        {"claim": "co-buy", "supporting_evidence_ids": ["e2"], "counter_evidence_ids": [], "uncertainty": "medium"}
                    ],
                    "creator_risks": [],
                    "wallet_risks": []
                }, "degraded": false}}}
            })),
            8.0,
            0,
        ));
        assert_eq!(veto.action, OpportunityAction::Avoid);
        assert!(veto
            .reasons
            .iter()
            .any(|reason| reason == "research_coordination_evidence"));
    }

    #[test]
    fn research_filter_passes_grounded_low_risk_dossier() {
        let passed = evaluate_research_filter(&research_input_from_artifacts(
            Some(&json!({
                "provider": "siliconflow",
                "validation": {"grounded": true},
                "response": {"roles": {"committee": {"advisory": {
                    "coordination_hypotheses": [],
                    "creator_risks": ["one creator note"],
                    "wallet_risks": []
                }, "degraded": false}}}
            })),
            8.0,
            0,
        ));
        assert_eq!(passed.action, OpportunityAction::Buy);
        assert_eq!(passed.reasons, vec!["research_filter_passed".to_string()]);
    }

    #[test]
    fn unavailable_investigator_does_not_copy_the_baseline_buy() {
        let avoided = evaluate_research_filter(&research_input_from_artifacts(
            Some(&json!({
                "provider": "unavailable",
                "validation": {"grounded": false, "error_code": "INVESTIGATOR_UNAVAILABLE"}
            })),
            3.0,
            0,
        ));
        assert_eq!(avoided.action, OpportunityAction::Avoid);
    }

    #[test]
    fn shadow_gate_abstains_without_inventing_a_probability() {
        let missing = evaluate_shadow_ml_gate(&ShadowMlGateInput::from_exit_hazard(
            None,
            SHADOW_MAXIMUM_EXIT_HAZARD,
        ));
        assert_eq!(missing.action, OpportunityAction::Abstain);
        let high = evaluate_shadow_ml_gate(&ShadowMlGateInput::from_exit_hazard(
            Some(0.81),
            SHADOW_MAXIMUM_EXIT_HAZARD,
        ));
        assert_eq!(high.action, OpportunityAction::Avoid);
        let accepted = evaluate_shadow_ml_gate(&ShadowMlGateInput::from_exit_hazard(
            Some(0.22),
            SHADOW_MAXIMUM_EXIT_HAZARD,
        ));
        assert_eq!(accepted.action, OpportunityAction::Buy);
    }

    #[test]
    fn inferred_shared_funders_are_counted_without_ownership_claims() {
        let dossier = json!({
            "enrichment": {"wallets": [
                {"wallet": "A", "funding_inference": {"funder": "X"}},
                {"wallet": "B", "funding_inference": {"funder": "X"}},
                {"wallet": "C", "funding_inference": {"funder": "X"}},
                {"wallet": "D", "funding_inference": {"funder": "Y"}}
            ]}
        });
        assert_eq!(shared_inferred_funder_wallets(&dossier), 3);
    }

    #[test]
    fn siliconflow_features_are_numeric_not_prose() {
        let features = investigation_features(Some(&json!({
            "validation": {"grounded": true, "latency_ms": 850.0},
            "response": {"roles": {"committee": {"advisory": {
                "coordination_hypotheses": [
                    {"claim": "x", "supporting_evidence_ids": ["e1"]}
                ],
                "creator_risks": ["a"],
                "wallet_risks": [],
                "organic_demand_confidence": 0.4,
                "confidence": 0.62
            }}}}
        })));
        assert_eq!(features["coordination_risk"], 1);
        assert_eq!(features["creator_risk"], 1);
        assert_eq!(features["organic_demand_confidence"], 0.4);
        assert_eq!(features["investigation_confidence"], 0.62);
    }
}
