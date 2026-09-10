//! Automatic Strategy Diff. A pair is only an ablation when the listed economic
//! assumptions match. Anything unexpected is CONFOUNDED.

use crate::{
    book_simulator, exec_select_timing, is_exec_select, TimingRegime, ADAPTIVE_EXPERIMENT_ID,
    DEFAULT_EXPERIMENT_ID, EXECUTION_EDGE_ENTRY_ATOMIC, EXECUTION_EDGE_EXPERIMENT_ID,
    EXEC_SELECT_EXPERIMENT_ID, SIMULATOR_EXECUTION_TWIN, SIMULATOR_LEGACY,
    STRATEGY_CHAMPION_SHORT_HORIZON_V1, STRATEGY_CONTROL_FLOW_V1, STRATEGY_FAST_SCOUT_V1,
    STRATEGY_FULL_ML_SHADOW, STRATEGY_HEURISTIC_GRAPH_AI_V1, STRATEGY_HEURISTIC_V1,
    STRATEGY_INFORMATION_VALUE_EXPLORER_V1, STRATEGY_IV_SELECT_V1, STRATEGY_LOSS_FILTER_V1,
    STRATEGY_MARKET_CONFIRMED_V1, STRATEGY_MARKET_ONLY_SHADOW, STRATEGY_MICROSTRUCTURE_BASELINE_V1,
    STRATEGY_RANDOM_CONTROL_V1, STRATEGY_RANDOM_EXECUTION_CONTROL_V1, STRATEGY_RANDOM_SELECT_V1,
    STRATEGY_RESEARCH_EXPLORATION_V1, STRATEGY_SURVIVOR_PICKER_V1,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComparisonStatus {
    Comparable,
    Confounded,
    UnknownBook,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StrategyManifest {
    pub key: &'static str,
    pub experiment_id: &'static str,
    pub simulator: &'static str,
    pub timing_regime: Option<&'static str>,
    pub selection: &'static str,
    pub features: &'static [&'static str],
    pub uses_ml: bool,
    pub uses_ai: bool,
    pub uses_graph: bool,
    pub uses_holder: bool,
    pub uses_funding: bool,
    pub uses_creator: bool,
    pub continuous_risk_exits: bool,
    pub entry_atomic: Option<u128>,
    pub confidence_sizing: bool,
    pub frozen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StrategyDiff {
    pub a: &'static str,
    pub b: &'static str,
    pub status: ComparisonStatus,
    pub same: Vec<&'static str>,
    pub different: Vec<&'static str>,
    pub confounders: Vec<&'static str>,
}

const MARKET: &[&str] = &["unique_buyers_30", "buy_sell_imbalance_30", "return_30", "sell_count_30"];
const IV: &[&str] = &["prediction_uncertainty", "missingness_novelty", "regime_rarity", "ood_novelty"];
const LOSS: &[&str] = &[
    "unique_buyers_30",
    "buy_sell_imbalance_30",
    "return_30",
    "sell_count_30",
    "holder_top_1_share",
    "creator_risk",
    "same_slot_first10_share",
    "same_funder_first10_count",
];

pub fn strategy_manifest(key: &str) -> Option<StrategyManifest> {
    Some(match key {
        STRATEGY_RANDOM_CONTROL_V1 => manifest(
            STRATEGY_RANDOM_CONTROL_V1,
            ADAPTIVE_EXPERIMENT_ID,
            SIMULATOR_LEGACY,
            None,
            "seeded_hash",
            &[],
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            None,
            true,
        ),
        STRATEGY_INFORMATION_VALUE_EXPLORER_V1 => manifest(STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
            ADAPTIVE_EXPERIMENT_ID,
            SIMULATOR_LEGACY,
            None,
            "information_value",
            IV,
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            None,
            true,
        ),
        STRATEGY_CHAMPION_SHORT_HORIZON_V1 => manifest(STRATEGY_CHAMPION_SHORT_HORIZON_V1,
            ADAPTIVE_EXPERIMENT_ID,
            SIMULATOR_LEGACY,
            None,
            "champion_short_horizon",
            &["short_momentum_2m_v1", "adverse_move_2m_v1"],
            true,
            false,
            false,
            false,
            false,
            false,
            false,
            None,
            true,
        ),
        STRATEGY_RANDOM_EXECUTION_CONTROL_V1 => manifest(STRATEGY_RANDOM_EXECUTION_CONTROL_V1,
            EXECUTION_EDGE_EXPERIMENT_ID,
            SIMULATOR_EXECUTION_TWIN,
            None,
            "seeded_hash",
            &[],
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Some(EXECUTION_EDGE_ENTRY_ATOMIC),
            true,
        ),
        STRATEGY_MICROSTRUCTURE_BASELINE_V1 => manifest(STRATEGY_MICROSTRUCTURE_BASELINE_V1,
            EXECUTION_EDGE_EXPERIMENT_ID,
            SIMULATOR_EXECUTION_TWIN,
            None,
            "microstructure_rule_v1",
            MARKET,
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            Some(EXECUTION_EDGE_ENTRY_ATOMIC),
            true,
        ),
        STRATEGY_RANDOM_SELECT_V1 => select(STRATEGY_RANDOM_SELECT_V1, "seeded_hash", &[], TimingRegime::ConfirmedEarly, false, false, false),
        STRATEGY_MARKET_CONFIRMED_V1 => {
            select(STRATEGY_MARKET_CONFIRMED_V1, "microstructure_rule_v1", MARKET, TimingRegime::ConfirmedEarly, false, false, false)
        }
        STRATEGY_LOSS_FILTER_V1 => {
            select(STRATEGY_LOSS_FILTER_V1, "loss_filter_v1", LOSS, TimingRegime::ConfirmedEarly, true, true, true)
        }
        STRATEGY_IV_SELECT_V1 => {
            select(STRATEGY_IV_SELECT_V1, "information_value_features", IV, TimingRegime::ConfirmedEarly, false, false, false)
        }
        STRATEGY_FAST_SCOUT_V1 => {
            select(STRATEGY_FAST_SCOUT_V1, "microstructure_rule_v1", MARKET, TimingRegime::FastScout, false, false, false)
        }
        STRATEGY_SURVIVOR_PICKER_V1 => {
            select(STRATEGY_SURVIVOR_PICKER_V1, "microstructure_rule_v1", MARKET, TimingRegime::Survivor, false, false, false)
        }
        STRATEGY_CONTROL_FLOW_V1 => loop_book(STRATEGY_CONTROL_FLOW_V1, "control_flow", &[], false, false, false),
        STRATEGY_HEURISTIC_V1 => loop_book(STRATEGY_HEURISTIC_V1, "heuristic", MARKET, false, false, false),
        STRATEGY_RESEARCH_EXPLORATION_V1 => loop_book(STRATEGY_RESEARCH_EXPLORATION_V1, "exploration", MARKET, false, false, false),
        STRATEGY_HEURISTIC_GRAPH_AI_V1 => loop_book(STRATEGY_HEURISTIC_GRAPH_AI_V1, "heuristic_graph_ai", MARKET, true, true, true),
        STRATEGY_MARKET_ONLY_SHADOW => loop_book(STRATEGY_MARKET_ONLY_SHADOW, "shadow_market_ml", MARKET, false, false, false),
        STRATEGY_FULL_ML_SHADOW => loop_book(STRATEGY_FULL_ML_SHADOW, "shadow_full_ml", MARKET, true, false, true),
        _ => return None,
    })
}

fn select(
    key: &'static str,
    selection: &'static str,
    features: &'static [&'static str],
    regime: TimingRegime,
    holder: bool,
    funding: bool,
    creator: bool,
) -> StrategyManifest {
    StrategyManifest {
        key,
        experiment_id: EXEC_SELECT_EXPERIMENT_ID,
        simulator: SIMULATOR_EXECUTION_TWIN,
        timing_regime: Some(regime.as_str()),
        selection,
        features,
        uses_ml: false,
        uses_ai: false,
        uses_graph: false,
        uses_holder: holder,
        uses_funding: funding,
        uses_creator: creator,
        continuous_risk_exits: true,
        entry_atomic: Some(EXECUTION_EDGE_ENTRY_ATOMIC),
        confidence_sizing: false,
        frozen: false,
    }
}

fn loop_book(
    key: &'static str,
    selection: &'static str,
    features: &'static [&'static str],
    graph: bool,
    ai: bool,
    holder: bool,
) -> StrategyManifest {
    manifest(
        key,
        DEFAULT_EXPERIMENT_ID,
        SIMULATOR_LEGACY,
        None,
        selection,
        features,
        selection.contains("ml") || selection.contains("shadow"),
        ai,
        graph,
        holder,
        false,
        graph,
        false,
        None,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn manifest(
    key: &'static str,
    experiment_id: &'static str,
    simulator: &'static str,
    timing_regime: Option<&'static str>,
    selection: &'static str,
    features: &'static [&'static str],
    uses_ml: bool,
    uses_ai: bool,
    uses_graph: bool,
    uses_holder: bool,
    uses_funding: bool,
    uses_creator: bool,
    continuous_risk_exits: bool,
    entry_atomic: Option<u128>,
    frozen: bool,
) -> StrategyManifest {
    StrategyManifest {
        key,
        experiment_id,
        simulator,
        timing_regime,
        selection,
        features,
        uses_ml,
        uses_ai,
        uses_graph,
        uses_holder,
        uses_funding,
        uses_creator,
        continuous_risk_exits,
        entry_atomic,
        confidence_sizing: false,
        frozen,
    }
}

pub fn strategy_diff(a_key: &str, b_key: &str) -> StrategyDiff {
    let Some(a) = strategy_manifest(a_key) else {
        return StrategyDiff {
            a: intern_or_unknown(a_key),
            b: intern_or_unknown(b_key),
            status: ComparisonStatus::UnknownBook,
            same: vec![],
            different: vec![],
            confounders: vec!["unknown_book"],
        };
    };
    let Some(b) = strategy_manifest(b_key) else {
        return StrategyDiff {
            a: a.key,
            b: intern_or_unknown(b_key),
            status: ComparisonStatus::UnknownBook,
            same: vec![],
            different: vec![],
            confounders: vec!["unknown_book"],
        };
    };
    let mut same = Vec::new();
    let mut different = Vec::new();
    let mut confounders = Vec::new();
    cmp("experiment", a.experiment_id == b.experiment_id, &mut same, &mut different);
    cmp("simulator", a.simulator == b.simulator, &mut same, &mut different);
    if a.simulator != b.simulator {
        confounders.push("simulator");
    }
    cmp("entry_atomic", a.entry_atomic == b.entry_atomic, &mut same, &mut different);
    if a.entry_atomic != b.entry_atomic {
        confounders.push("entry_size");
    }
    cmp(
        "timing_regime",
        a.timing_regime == b.timing_regime,
        &mut same,
        &mut different,
    );
    if a.timing_regime != b.timing_regime {
        confounders.push("timing_regime");
    }
    cmp(
        "continuous_risk_exits",
        a.continuous_risk_exits == b.continuous_risk_exits,
        &mut same,
        &mut different,
    );
    if a.continuous_risk_exits != b.continuous_risk_exits {
        confounders.push("exit_policy");
    }
    cmp("confidence_sizing", a.confidence_sizing == b.confidence_sizing, &mut same, &mut different);
    cmp("selection", a.selection == b.selection, &mut same, &mut different);
    cmp("uses_ml", a.uses_ml == b.uses_ml, &mut same, &mut different);
    cmp("uses_ai", a.uses_ai == b.uses_ai, &mut same, &mut different);
    cmp("uses_graph", a.uses_graph == b.uses_graph, &mut same, &mut different);
    cmp("uses_holder", a.uses_holder == b.uses_holder, &mut same, &mut different);
    cmp("uses_funding", a.uses_funding == b.uses_funding, &mut same, &mut different);
    cmp("uses_creator", a.uses_creator == b.uses_creator, &mut same, &mut different);
    if a.selection != b.selection {
        different.push("selection_rule");
    }
    let status = if confounders.is_empty() {
        ComparisonStatus::Comparable
    } else {
        ComparisonStatus::Confounded
    };
    let _ = (book_simulator(a.key), is_exec_select(a.key), exec_select_timing(a.key));
    StrategyDiff {
        a: a.key,
        b: b.key,
        status,
        same,
        different,
        confounders,
    }
}

fn cmp(name: &'static str, equal: bool, same: &mut Vec<&'static str>, different: &mut Vec<&'static str>) {
    if equal {
        same.push(name);
    } else {
        different.push(name);
    }
}

fn intern_or_unknown(key: &str) -> &'static str {
    strategy_manifest(key).map(|m| m.key).unwrap_or("UNKNOWN")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matched_exec_select_pair_is_comparable() {
        let diff = strategy_diff(STRATEGY_RANDOM_SELECT_V1, STRATEGY_MARKET_CONFIRMED_V1);
        assert_eq!(diff.status, ComparisonStatus::Comparable);
        assert!(diff.same.contains(&"simulator"));
        assert!(diff.same.contains(&"timing_regime"));
        assert!(diff.same.contains(&"entry_atomic"));
        assert!(diff.different.contains(&"selection") || diff.different.contains(&"selection_rule"));
        assert!(diff.confounders.is_empty());
    }

    #[test]
    fn timing_mismatch_is_confounded() {
        let diff = strategy_diff(STRATEGY_MARKET_CONFIRMED_V1, STRATEGY_SURVIVOR_PICKER_V1);
        assert_eq!(diff.status, ComparisonStatus::Confounded);
        assert!(diff.confounders.contains(&"timing_regime"));
    }

    #[test]
    fn legacy_vs_twin_is_confounded() {
        let diff = strategy_diff(STRATEGY_RANDOM_CONTROL_V1, STRATEGY_RANDOM_EXECUTION_CONTROL_V1);
        assert_eq!(diff.status, ComparisonStatus::Confounded);
        assert!(diff.confounders.contains(&"simulator"));
    }

    #[test]
    fn loss_filter_adds_holder_and_funding_not_ai() {
        let manifest = strategy_manifest(STRATEGY_LOSS_FILTER_V1).unwrap();
        assert!(manifest.uses_holder);
        assert!(manifest.uses_funding);
        assert!(!manifest.uses_ai);
        assert!(!manifest.confidence_sizing);
        assert_eq!(manifest.simulator, SIMULATOR_EXECUTION_TWIN);
    }
}
