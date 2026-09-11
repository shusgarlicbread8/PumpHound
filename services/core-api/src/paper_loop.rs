use crate::model_client::{
    champions_compatible, missing_required_targets, target_version_map, AggregatePrediction,
    ChampionSet, ModelClient,
};
use crate::paper_ledger;
use crate::state::AppState;
use pumphound_domain::{
    now_utc, AssetAmount, Atomic, FeatureVector, PaperOrderRequest, PaperQuote, Pubkey, TradeSide,
    WSOL_MINT,
};
use pumphound_market_state::curve_progress;
use pumphound_paper_core::{
    adaptive_desks, book_by_key, evaluate_champion_short_horizon, evaluate_exec_select,
    evaluate_exit, evaluate_microstructure_baseline, evaluate_open_position_risk, exec_select_desks,
    execution_edge_desks, is_exec_select, score_information_value, uses_execution_twin,
    EXECUTION_EDGE_EXPERIMENT_ID, EXEC_SELECT_EXPERIMENT_ID, MICROSTRUCTURE_MAX_SELL_COUNT_30,
    MICROSTRUCTURE_MIN_IMBALANCE_30, MICROSTRUCTURE_MIN_RETURN_30,
    MICROSTRUCTURE_MIN_UNIQUE_BUYERS_30, STRATEGY_RANDOM_EXECUTION_CONTROL_V1,
    evaluate_heuristic_exit, evaluate_independent_book, evaluate_information_value_explorer,
    evaluate_opportunity, evaluate_random_control, evaluate_research_filter,
    evaluate_shared_universe, evaluate_technical_eligibility, funnel_stage_for_reasons,
    investigation_features, learning_books, learning_funnel_state, local_graph_from_indicators,
    parallel_books, quote_order, research_input_from_artifacts,
    shared_inferred_funder_wallets, AutonomousPolicy, ChampionInputQuality, ExitAction,
    ExitDecision, ExitInput, HeuristicExitInput, HeuristicOpportunityInput, HeuristicPolicyV1,
    InformationValueInput, LearningFunnelState, LearningPolicyV1, OpportunityAction, RiskSnapshot,
    OpportunityDecision, OpportunityInput, ShadowMlGateInput, StrategyBook,
    TechnicalEligibilityInput, ADAPTIVE_EXPERIMENT_ID, ADVERSE_MOVE_TARGET,
    CHAMPION_MAXIMUM_ADVERSE, CHAMPION_MAX_MISSINGNESS, CHAMPION_MAX_OOD,
    CHAMPION_MINIMUM_FAMILIARITY, CHAMPION_MINIMUM_MOMENTUM, DEFAULT_EXPERIMENT_ID,
    EXIT_HAZARD_TARGET, EXPECTED_MFE_TARGET, FEATURE_VERSION, FEE_MODEL_VERSION,
    LATENCY_CONFIG_VERSION, SHADOW_MAXIMUM_EXIT_HAZARD, SHARED_UNIVERSE_VERSION,
    SHORT_MOMENTUM_TARGET, STRATEGY_CHAMPION_SHORT_HORIZON_V1, STRATEGY_CONTROL_FLOW_V1,
    STRATEGY_INFORMATION_VALUE_EXPLORER_V1, STRATEGY_IV_SELECT_V1, STRATEGY_RANDOM_CONTROL_V1,
};
use serde_json::{json, Value};
use sqlx::Row;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use tracing::{info, warn};
use uuid::Uuid;

struct LoadedPolicy {
    policy: AutonomousPolicy,
    raw: Value,
    sha256: String,
}

struct LoadedHeuristicPolicy {
    policy: HeuristicPolicyV1,
    raw: Value,
    sha256: String,
}

struct LoadedLearningPolicy {
    policy: LearningPolicyV1,
    raw: Value,
    sha256: String,
}

struct LearningPolicies {
    random_control: LoadedLearningPolicy,
    information_value: LoadedLearningPolicy,
}

impl LearningPolicies {
    fn for_book(&self, book: StrategyBook) -> &LoadedLearningPolicy {
        if book.key == STRATEGY_RANDOM_CONTROL_V1 {
            &self.random_control
        } else {
            // Champion and explorer share the information-value execution
            // envelope (size, hold, impact). Book identity stays book.key.
            &self.information_value
        }
    }
}

struct ExperimentPolicies {
    heuristic: LoadedHeuristicPolicy,
    exploration: LoadedHeuristicPolicy,
    control: LoadedHeuristicPolicy,
    experiment_id: String,
}

impl ExperimentPolicies {
    fn for_book(&self, book: StrategyBook) -> &LoadedHeuristicPolicy {
        if book.uses_exploration_policy {
            &self.exploration
        } else if book.key == STRATEGY_CONTROL_FLOW_V1 {
            &self.control
        } else {
            &self.heuristic
        }
    }
}

const OPTIONAL_GRAPH_FEATURE_NAMES: [&str; 23] = [
    "holder_top_1_share",
    "holder_top_5_share",
    "holder_top_10_share",
    "holder_hhi",
    "cluster_hhi",
    "independent_cluster_count",
    "creator_linked_supply_share",
    "fresh_wallet_share",
    "immediate_funding_share",
    "funding_diversity",
    "cohort_recurrence",
    "co_buy_density",
    "weighted_degree_concentration",
    "cohort_modularity",
    "smart_wallet_retention",
    "wallet_skill_posterior_mean",
    "wallet_skill_effective_sample",
    "creator_graduation_posterior",
    "creator_collapse_posterior",
    "history_coverage_fraction",
    "same_slot_first10_share",
    "same_funder_first10_count",
    "first10_buyer_count",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatingMode {
    LiveWatch,
    LivePaperHeuristic,
    LivePaperShadowMl,
    LivePaperMl,
    Replay,
    Unsupported,
}

impl OperatingMode {
    pub fn parse(mode: &str) -> Self {
        match mode {
            "LIVE_WATCH" => Self::LiveWatch,
            "LIVE_PAPER_HEURISTIC" => Self::LivePaperHeuristic,
            "LIVE_PAPER_SHADOW_ML" => Self::LivePaperShadowMl,
            "LIVE_PAPER_ML" => Self::LivePaperMl,
            "REPLAY" => Self::Replay,
            _ => Self::Unsupported,
        }
    }

    pub fn allows_paper_orders(self) -> bool {
        matches!(
            self,
            Self::LivePaperHeuristic | Self::LivePaperShadowMl | Self::LivePaperMl | Self::Replay
        )
    }

    pub fn heuristic_controls(self) -> bool {
        matches!(self, Self::LivePaperHeuristic | Self::LivePaperShadowMl)
    }
}

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let model = ModelClient::from_env();
        let loaded_ml_policy = load_ml_policy().await;
        match &loaded_ml_policy {
            Ok(Some(policy)) => {
                if let Err(error) = paper_ledger::register_policy(
                    &state.stores,
                    &policy.policy.policy_version,
                    &policy.raw,
                    &policy.sha256,
                )
                .await
                {
                    warn!(%error, "could not register paper policy");
                }
            }
            Ok(None) => info!(
                "LIVE_PAPER_ML control policy absent; heuristic, shadow, and ALPHA-LEARN books still scan"
            ),
            Err(error) => warn!(
                %error,
                "LIVE_PAPER_ML control policy invalid; heuristic, shadow, and ALPHA-LEARN books still scan"
            ),
        }
        let ml_policy = loaded_ml_policy.ok().flatten();
        if let Err(error) = paper_ledger::archive_unused_default_portfolios(&state.stores).await {
            warn!(%error, "could not archive unused default paper portfolios");
        }
        let loaded_experiment = load_experiment_policies().await;
        match &loaded_experiment {
            Ok(experiment) => {
                let universe_config = json!({
                    "schema_version": 1,
                    "strategy": SHARED_UNIVERSE_VERSION,
                    "experiment_id": experiment.experiment_id
                });
                let universe_sha =
                    pumphound_domain::sha256_hex(universe_config.to_string().as_bytes());
                if let Err(error) = paper_ledger::register_policy(
                    &state.stores,
                    SHARED_UNIVERSE_VERSION,
                    &universe_config,
                    &universe_sha,
                )
                .await
                {
                    warn!(%error, "could not register shared universe policy");
                }
                for loaded in [
                    &experiment.heuristic,
                    &experiment.exploration,
                    &experiment.control,
                ] {
                    if let Err(error) = paper_ledger::register_policy(
                        &state.stores,
                        &loaded.policy.strategy_version,
                        &loaded.raw,
                        &loaded.sha256,
                    )
                    .await
                    {
                        warn!(%error, policy = %loaded.policy.strategy_version, "could not register experiment policy");
                    }
                }
                for book in parallel_books(true) {
                    let loaded = experiment.for_book(book);
                    let config = json!({
                        "schema_version": 1,
                        "experiment_id": experiment.experiment_id,
                        "strategy": book.key,
                        "strategy_version": book.key,
                        "signal_policy": loaded.policy.strategy_version,
                        "signal_policy_sha256": loaded.sha256,
                        "feature_version": FEATURE_VERSION,
                        "fee_model_version": FEE_MODEL_VERSION,
                        "latency_config": LATENCY_CONFIG_VERSION,
                        "uses_research_filter": book.uses_research_filter,
                        "uses_shadow_ml_gate": book.uses_shadow_ml_gate,
                        "independent_book": true,
                        "live_paper_ml_control": false
                    });
                    let sha256 = pumphound_domain::sha256_hex(config.to_string().as_bytes());
                    if let Err(error) =
                        paper_ledger::register_policy(&state.stores, book.key, &config, &sha256)
                            .await
                    {
                        warn!(%error, strategy = book.key, "could not register parallel strategy policy");
                    }
                }
                if let Err(error) = paper_ledger::ensure_research_track_invariant(
                    &state.stores,
                    DEFAULT_EXPERIMENT_ID,
                    true,
                    &json!({
                        "configs/heuristic-v1.json": experiment.heuristic.sha256,
                        "configs/research-exploration-v1.json": experiment.exploration.sha256,
                        "configs/control-flow-v1.json": experiment.control.sha256
                    }),
                    &json!(parallel_books(true)
                        .into_iter()
                        .map(|book| book.key)
                        .collect::<Vec<_>>()),
                    &json!({
                        "captured_before_alpha_learn": true,
                        "decisions": 2296,
                        "paper_fills": 797,
                        "simulation_runs": 6
                    }),
                )
                .await
                {
                    warn!(%error, "could not persist frozen experiment invariant");
                }
            }
            Err(error) => {
                warn!(%error, "experiment policies invalid; heuristic decisions disabled")
            }
        }
        let experiment = loaded_experiment.ok();
        let loaded_learning = load_learning_policies().await;
        match &loaded_learning {
            Ok(learning) => {
                for loaded in [&learning.random_control, &learning.information_value] {
                    if let Err(error) = paper_ledger::register_immutable_policy(
                        &state.stores,
                        &loaded.policy.strategy_version,
                        &loaded.raw,
                        &loaded.sha256,
                    )
                    .await
                    {
                        warn!(%error, policy = %loaded.policy.strategy_version, "could not register immutable learning policy");
                    } else if let Err(error) = paper_ledger::record_policy_transition(
                        &state.stores,
                        ADAPTIVE_EXPERIMENT_ID,
                        &loaded.policy.strategy_version,
                        "ACTIVE_PAPER",
                        "initial immutable ALPHA-LEARN paper policy",
                        &json!({
                            "content_sha256": loaded.sha256,
                            "automatic_promotion": false
                        }),
                    )
                    .await
                    {
                        warn!(%error, policy = %loaded.policy.strategy_version, "could not audit learning policy activation");
                    }
                }
                let champion_config = json!({
                    "schema_version": 1,
                    "policy_version": STRATEGY_CHAMPION_SHORT_HORIZON_V1,
                    "execution_envelope_policy": learning.information_value.policy.strategy_version,
                    "execution_envelope_sha256": learning.information_value.sha256,
                    "paper_only": true,
                    "automatic_promotion": false
                });
                let champion_sha = pumphound_domain::sha256_hex(
                    serde_json::to_string(&champion_config)
                        .unwrap_or_default()
                        .as_bytes(),
                );
                if let Err(error) = paper_ledger::register_policy(
                    &state.stores,
                    STRATEGY_CHAMPION_SHORT_HORIZON_V1,
                    &champion_config,
                    &champion_sha,
                )
                .await
                {
                    warn!(%error, "could not register champion book policy identity");
                }
                // EXECUTION-EDGE-V1 book identities: frozen; share RANDOM_CONTROL's execution
                // envelope; differ from ALPHA-LEARN only by simulator (and, for the
                // microstructure book, by the versioned selection rule).
                let twin_config = crate::execution_twin_bridge::default_config();
                for book in execution_edge_desks() {
                    let config = json!({
                        "schema_version": 1,
                        "policy_version": book.key,
                        "experiment_id": EXECUTION_EDGE_EXPERIMENT_ID,
                        "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION,
                        "landing_model_version": pumphound_execution_twin::landing::LANDING_MODEL_VERSION,
                        "twin_config": twin_config,
                        "execution_envelope_policy": learning.random_control.policy.strategy_version,
                        "execution_envelope_sha256": learning.random_control.sha256,
                        "selection": if book.key == STRATEGY_RANDOM_EXECUTION_CONTROL_V1 {
                            json!({"kind": "seeded_hash", "seed": learning.random_control.policy.deterministic_seed, "rate": learning.random_control.policy.selection_rate})
                        } else {
                            json!({"kind": "microstructure_rule_v1", "min_unique_buyers_30": MICROSTRUCTURE_MIN_UNIQUE_BUYERS_30, "min_imbalance_30": MICROSTRUCTURE_MIN_IMBALANCE_30, "min_return_30": MICROSTRUCTURE_MIN_RETURN_30, "max_sell_count_30": MICROSTRUCTURE_MAX_SELL_COUNT_30})
                        },
                        "paper_only": true,
                        "live_money": false,
                        "automatic_promotion": false
                    });
                    let sha = pumphound_domain::sha256_hex(
                        serde_json::to_string(&config).unwrap_or_default().as_bytes(),
                    );
                    if let Err(error) =
                        paper_ledger::register_immutable_policy(&state.stores, book.key, &config, &sha)
                            .await
                    {
                        warn!(%error, policy = book.key, "could not register execution-edge book policy");
                    }
                }
                if let Err(error) = paper_ledger::ensure_research_track_invariant(
                    &state.stores,
                    EXECUTION_EDGE_EXPERIMENT_ID,
                    true,
                    &json!({
                        "execution_envelope": learning.random_control.sha256,
                        "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION
                    }),
                    &json!(execution_edge_desks().map(|book| book.key)),
                    &json!({"paper_only": true, "automatic_promotion": false, "matched_legacy_book": STRATEGY_RANDOM_CONTROL_V1}),
                )
                .await
                {
                    warn!(%error, "could not register execution-edge track invariant");
                }
                for book in exec_select_desks() {
                    let regime = pumphound_paper_core::exec_select_timing(book.key);
                    let config = json!({
                        "schema_version": 1,
                        "policy_version": book.key,
                        "experiment_id": EXEC_SELECT_EXPERIMENT_ID,
                        "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION,
                        "landing_model_version": pumphound_execution_twin::landing::LANDING_MODEL_VERSION,
                        "twin_config": twin_config,
                        "execution_envelope_policy": learning.random_control.policy.strategy_version,
                        "execution_envelope_sha256": learning.random_control.sha256,
                        "timing_regime": regime.map(|r| r.as_str()),
                        "entry_atomic": pumphound_paper_core::EXECUTION_EDGE_ENTRY_ATOMIC.to_string(),
                        "confidence_sizing": false,
                        "continuous_risk_exits": true,
                        "paper_only": true,
                        "live_money": false,
                        "automatic_promotion": false
                    });
                    let sha = pumphound_domain::sha256_hex(
                        serde_json::to_string(&config).unwrap_or_default().as_bytes(),
                    );
                    if let Err(error) =
                        paper_ledger::register_immutable_policy(&state.stores, book.key, &config, &sha)
                            .await
                    {
                        warn!(%error, policy = book.key, "could not register exec-select book policy");
                    }
                }
                if let Err(error) = paper_ledger::ensure_research_track_invariant(
                    &state.stores,
                    EXEC_SELECT_EXPERIMENT_ID,
                    false,
                    &json!({
                        "execution_envelope": learning.random_control.sha256,
                        "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION
                    }),
                    &json!(exec_select_desks().map(|book| book.key)),
                    &json!({
                        "paper_only": true,
                        "automatic_promotion": false,
                        "picker_not_sniper": true,
                        "matched_confirmed_early": ["RANDOM_SELECT_V1","MARKET_CONFIRMED_V1","LOSS_FILTER_V1","IV_SELECT_V1"]
                    }),
                )
                .await
                {
                    warn!(%error, "could not register exec-select track invariant");
                }
                if let Err(error) = paper_ledger::ensure_research_track_invariant(
                    &state.stores,
                    ADAPTIVE_EXPERIMENT_ID,
                    false,
                    &json!({
                        STRATEGY_RANDOM_CONTROL_V1: learning.random_control.sha256,
                        STRATEGY_INFORMATION_VALUE_EXPLORER_V1: learning.information_value.sha256
                    }),
                    &json!(learning_books().map(|book| book.key)),
                    &json!({"paper_only": true, "automatic_promotion": false}),
                )
                .await
                {
                    warn!(%error, "could not register learning track invariant");
                }
            }
            Err(error) => warn!(%error, "learning policies invalid; ALPHA-LEARN disabled"),
        }
        let learning = loaded_learning.ok();
        // Settlement runs on its own cadence. When it shared the scan loop, a slow scan (several
        // books × open positions × Postgres round-trips) could exceed the 5s arrival window and
        // every order queued during that scan was rejected as REJECTED_ARRIVAL_DEADLINE — a
        // scheduler artefact mislabelled as an execution failure.
        {
            let state = state.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));
                loop {
                    tick.tick().await;
                    if !OperatingMode::parse(&state.mode).allows_paper_orders() {
                        continue;
                    }
                    if let Err(error) = settle_recoverable(&state).await {
                        warn!(%error, "paper recovery settlement failed");
                    }
                }
            });
        }
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            tick.tick().await;
            let mode = OperatingMode::parse(&state.mode);
            if !mode.allows_paper_orders() {
                continue;
            }
            if mode.heuristic_controls() {
                if let Some(policies) = experiment.as_ref() {
                    if let Err(error) = scan_heuristic_entries(&state, &model, policies, mode).await
                    {
                        warn!(%error, "heuristic opportunity scan failed");
                    }
                    if let Err(error) = scan_heuristic_exits(&state, &model, policies, mode).await {
                        warn!(%error, "heuristic exit scan failed");
                    }
                }
            } else {
                if let Err(error) = scan_ml_entries(&state, &model, ml_policy.as_ref()).await {
                    warn!(%error, "model opportunity scan failed");
                }
                if let (Some(policy), Ok(champions)) = (
                    ml_policy.as_ref(),
                    paper_ledger::champions(&state.stores).await,
                ) {
                    if missing_required_targets(&champions).is_empty() {
                        if let Err(error) = scan_ml_exits(&state, &model, policy, &champions).await
                        {
                            warn!(%error, "model exit scan failed");
                        }
                    }
                }
            }
            if let Some(learning) = learning.as_ref() {
                if let Err(error) = scan_learning_entries(&state, &model, learning).await {
                    warn!(%error, "adaptive learning opportunity scan failed");
                }
                if let Err(error) = scan_learning_exits(&state, learning).await {
                    warn!(%error, "adaptive learning exit scan failed");
                }
            }
            refresh_projection(&state).await;
        }
    });
}

pub fn autonomous_mode(mode: &str) -> bool {
    OperatingMode::parse(mode).allows_paper_orders()
}

pub fn mode_strategy_status(mode: OperatingMode) -> (&'static str, &'static str) {
    match mode {
        OperatingMode::LiveWatch => ("NONE", "NOT_TRAINED"),
        OperatingMode::LivePaperHeuristic => ("HEURISTIC_V1", "NOT_TRAINED"),
        OperatingMode::LivePaperShadowMl => ("HEURISTIC_V1", "SHADOW"),
        OperatingMode::LivePaperMl => ("ML_POLICY", "CONTROL"),
        OperatingMode::Replay => ("REPLAY", "CONTROL"),
        OperatingMode::Unsupported => ("NONE", "NOT_TRAINED"),
    }
}

async fn load_ml_policy() -> Result<Option<LoadedPolicy>, String> {
    let Some(path) = std::env::var("PUMPHOUND_PAPER_POLICY_PATH")
        .ok()
        .filter(|path| !path.trim().is_empty())
    else {
        return Ok(None);
    };
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("policy_read_failed:{e}"))?;
    let policy = AutonomousPolicy::from_json(&bytes)?;
    let raw: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("policy_json_failed:{e}"))?;
    Ok(Some(LoadedPolicy {
        policy,
        raw,
        sha256: pumphound_domain::sha256_hex(&bytes),
    }))
}

async fn load_experiment_policies() -> Result<ExperimentPolicies, String> {
    Ok(ExperimentPolicies {
        heuristic: load_named_policy(
            "PUMPHOUND_HEURISTIC_POLICY_PATH",
            "configs/heuristic-v1.json",
        )
        .await?,
        exploration: load_named_policy(
            "PUMPHOUND_EXPLORATION_POLICY_PATH",
            "configs/research-exploration-v1.json",
        )
        .await?,
        control: load_named_policy(
            "PUMPHOUND_CONTROL_POLICY_PATH",
            "configs/control-flow-v1.json",
        )
        .await?,
        experiment_id: std::env::var("PUMPHOUND_EXPERIMENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_EXPERIMENT_ID.to_string()),
    })
}

async fn load_learning_policies() -> Result<LearningPolicies, String> {
    Ok(LearningPolicies {
        random_control: load_learning_policy(
            "PUMPHOUND_RANDOM_CONTROL_POLICY_PATH",
            "configs/random-control-v1.json",
            STRATEGY_RANDOM_CONTROL_V1,
        )
        .await?,
        information_value: load_learning_policy(
            "PUMPHOUND_INFORMATION_VALUE_POLICY_PATH",
            "configs/information-value-explorer-v1.json",
            STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
        )
        .await?,
    })
}

async fn load_learning_policy(
    env_key: &str,
    default_path: &str,
    expected_strategy: &str,
) -> Result<LoadedLearningPolicy, String> {
    let path = std::env::var(env_key).unwrap_or_else(|_| default_path.to_string());
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| format!("learning_policy_read_failed:{path}:{error}"))?;
    let policy = LearningPolicyV1::from_json(&bytes, expected_strategy)?;
    let raw: Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("learning_policy_json_failed:{path}:{error}"))?;
    Ok(LoadedLearningPolicy {
        policy,
        raw,
        sha256: pumphound_domain::sha256_hex(&bytes),
    })
}

async fn load_named_policy(
    env_key: &str,
    default_path: &str,
) -> Result<LoadedHeuristicPolicy, String> {
    let path = std::env::var(env_key).unwrap_or_else(|_| default_path.to_string());
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|error| format!("policy_read_failed:{path}:{error}"))?;
    let policy = HeuristicPolicyV1::from_json(&bytes)?;
    let raw = serde_json::from_slice(&bytes)
        .map_err(|error| format!("policy_json_failed:{path}:{error}"))?;
    Ok(LoadedHeuristicPolicy {
        policy,
        raw,
        sha256: pumphound_domain::sha256_hex(&bytes),
    })
}

struct LiveDesk {
    book: StrategyBook,
    portfolio_id: Uuid,
    run_id: Uuid,
}

async fn ensure_parallel_desks(
    state: &AppState,
    experiment: &ExperimentPolicies,
    mode: OperatingMode,
) -> anyhow::Result<Vec<LiveDesk>> {
    let mut desks = Vec::new();
    for book in parallel_books(mode == OperatingMode::LivePaperShadowMl) {
        let loaded = experiment.for_book(book);
        let ml_status = if book.uses_shadow_ml_gate {
            "SHADOW"
        } else if mode == OperatingMode::LivePaperShadowMl {
            "SHADOW"
        } else {
            book.ml_status
        };
        let (portfolio_id, run_id) = paper_ledger::ensure_strategy_portfolio(
            &state.stores,
            &state.mode,
            book.key,
            book.desk_name,
            book.key,
            book.capital_sol_atomic,
            0,
            ml_status,
            book.ai_status,
            json!({
                "experiment_id": experiment.experiment_id,
                "experiment_frozen": true,
                "strategy_version": book.key,
                "feature_version": FEATURE_VERSION,
                "fee_model_version": FEE_MODEL_VERSION,
                "latency_config": LATENCY_CONFIG_VERSION,
                "ai_prompt_version": "committee-v1",
                "position_sizing": {
                    "fixed_sol_atomic": loaded.policy.fixed_sol_atomic,
                    "maximum_positions": loaded.policy.maximum_positions
                },
                "signal_policy": loaded.policy.strategy_version,
                "signal_policy_sha256": loaded.sha256,
                "execution_authority": book.execution_authority,
                "uses_research_filter": book.uses_research_filter,
                "uses_shadow_ml_gate": book.uses_shadow_ml_gate,
                "independent_book": true,
                "live_paper_ml_control": false
            }),
        )
        .await?;
        desks.push(LiveDesk {
            book,
            portfolio_id,
            run_id,
        });
    }
    Ok(desks)
}

async fn ensure_learning_desks(
    state: &AppState,
    learning: &LearningPolicies,
) -> anyhow::Result<Vec<LiveDesk>> {
    let mut desks = Vec::new();
    for book in adaptive_desks() {
        let loaded = learning.for_book(book);
        let (portfolio_id, run_id) = paper_ledger::ensure_strategy_portfolio(
            &state.stores,
            &state.mode,
            book.key,
            book.desk_name,
            book.key,
            book.capital_sol_atomic,
            0,
            book.ml_status,
            book.ai_status,
            json!({
                "experiment_id": ADAPTIVE_EXPERIMENT_ID,
                "experiment_frozen": false,
                "adaptive_learning": true,
                "paper_only": true,
                "automatic_promotion": false,
                "strategy_version": book.key,
                "execution_envelope_policy": loaded.policy.strategy_version,
                "policy_content_sha256": loaded.sha256,
                "feature_version": FEATURE_VERSION,
                "fee_model_version": FEE_MODEL_VERSION,
                "position_sizing": {
                    "fixed_sol_atomic": loaded.policy.execution.fixed_sol_atomic,
                    "maximum_positions": loaded.policy.execution.maximum_positions
                },
                "entry_authority": book.execution_authority,
                "shadow_predictions_cannot_change_control": true
            }),
        )
        .await?;
        desks.push(LiveDesk {
            book,
            portfolio_id,
            run_id,
        });
    }
    Ok(desks)
}

async fn ensure_exec_select_desks(
    state: &AppState,
    learning: &LearningPolicies,
) -> anyhow::Result<Vec<LiveDesk>> {
    let loaded = &learning.random_control;
    let twin_config = crate::execution_twin_bridge::default_config();
    let mut desks = Vec::new();
    for book in exec_select_desks() {
        let (portfolio_id, run_id) = paper_ledger::ensure_strategy_portfolio(
            &state.stores,
            &state.mode,
            book.key,
            book.desk_name,
            book.key,
            book.capital_sol_atomic,
            0,
            book.ml_status,
            book.ai_status,
            json!({
                "experiment_id": EXEC_SELECT_EXPERIMENT_ID,
                "experiment_frozen": false,
                "paper_only": true,
                "live_money": false,
                "picker_not_sniper": true,
                "strategy_version": book.key,
                "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION,
                "landing_model_version": pumphound_execution_twin::landing::LANDING_MODEL_VERSION,
                "twin_config": twin_config,
                "timing_regime": pumphound_paper_core::exec_select_timing(book.key).map(|r| r.as_str()),
                "execution_envelope_policy": loaded.policy.strategy_version,
                "policy_content_sha256": loaded.sha256,
                "feature_version": FEATURE_VERSION,
                "position_sizing": {
                    "fixed_sol_atomic": pumphound_paper_core::EXECUTION_EDGE_ENTRY_ATOMIC,
                    "maximum_positions": loaded.policy.execution.maximum_positions
                },
                "entry_authority": book.execution_authority,
                "confidence_sizing": false,
                "continuous_risk_exits": true,
                "shadow_predictions_cannot_change_control": true
            }),
        )
        .await?;
        desks.push(LiveDesk {
            book,
            portfolio_id,
            run_id,
        });
    }
    Ok(desks)
}

/// EXECUTION-EDGE-V1 desks. They share RANDOM_CONTROL's execution envelope (fixed size, hold,
/// impact caps) so RANDOM_CONTROL (legacy sim) vs RANDOM_EXECUTION_CONTROL (twin) differ ONLY in
/// the simulator, and MICROSTRUCTURE_BASELINE differs from the twin control ONLY in selection.
async fn ensure_execution_edge_desks(
    state: &AppState,
    learning: &LearningPolicies,
) -> anyhow::Result<Vec<LiveDesk>> {
    let loaded = &learning.random_control;
    let twin_config = crate::execution_twin_bridge::default_config();
    let mut desks = Vec::new();
    for book in execution_edge_desks() {
        let (portfolio_id, run_id) = paper_ledger::ensure_strategy_portfolio(
            &state.stores,
            &state.mode,
            book.key,
            book.desk_name,
            book.key,
            book.capital_sol_atomic,
            0,
            book.ml_status,
            book.ai_status,
            json!({
                "experiment_id": EXECUTION_EDGE_EXPERIMENT_ID,
                "experiment_frozen": true,
                "paper_only": true,
                "live_money": false,
                "strategy_version": book.key,
                "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION,
                "landing_model_version": pumphound_execution_twin::landing::LANDING_MODEL_VERSION,
                "twin_config": twin_config,
                "execution_envelope_policy": loaded.policy.strategy_version,
                "policy_content_sha256": loaded.sha256,
                "feature_version": FEATURE_VERSION,
                "position_sizing": {
                    "fixed_sol_atomic": loaded.policy.execution.fixed_sol_atomic,
                    "maximum_positions": loaded.policy.execution.maximum_positions
                },
                "entry_authority": book.execution_authority,
                "matched_legacy_book": STRATEGY_RANDOM_CONTROL_V1,
                "shadow_predictions_cannot_change_control": true
            }),
        )
        .await?;
        desks.push(LiveDesk {
            book,
            portfolio_id,
            run_id,
        });
    }
    Ok(desks)
}

const RECENT_LAUNCH_CANDIDATES_SQL: &str = r#"
SELECT launch_id, mint, creator, creation_time, known_at
FROM pumphound.launches l
WHERE mint NOT IN (
        'So11111111111111111111111111111111111111112',
        'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
      )
  AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
  AND NOT EXISTS (
       SELECT 1 FROM pumphound.entity_integrity_quarantines iq
       WHERE iq.entity_type='LAUNCH'
         AND iq.entity_id=l.launch_id::text
         AND iq.status='ACTIVE'
  )
  AND (
       known_at > now() - interval '20 minutes'
       OR mint IN (
        SELECT mint FROM pumphound.hot_set_memberships WHERE expires_at > now()
       )
   )
ORDER BY known_at DESC
LIMIT 80
"#;

// Must exceed market_rehydrate::MIN_REFETCH_INTERVAL (45s): a cold-market exit retry calls
// unstrand_position, which throttles repeat on-chain reads of the same market to protect the
// RPC provider. At the old 30s backoff, roughly a third of retries did nothing but hit that
// throttle and immediately fail with DATA_STALE, burning attempts on a market that was never
// actually re-checked. Checked live on 2026-09-09: 15 positions were stuck near the attempt cap
// this way. 50s clears the throttle window on every retry so each attempt is a real RPC check.
const EXIT_RETRY_BACKOFF_SECONDS: i64 = 50;
const EXIT_MAX_ATTEMPTS: i64 = 200;

/// Decision key for the next fixed-max-hold exit attempt, or `None` when an attempt is still
/// pending/filled, the last rejection is too recent, or the retry budget is exhausted.
async fn next_fixed_exit_decision_key(
    stores: &pumphound_storage::Stores,
    position_id: &str,
) -> anyhow::Result<Option<String>> {
    let row: Option<(i64, Option<String>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        r#"WITH attempts AS (
             SELECT d.decision_id, d.decision_at
             FROM pumphound.decisions d
             WHERE d.decision_kind='EXIT' AND (d.exact_inputs->>'position_id')=$1
               AND d.decision_key LIKE 'learn-exit:%:fixed-max-hold%'
           )
           SELECT (SELECT COUNT(*) FROM attempts)::bigint,
                  (SELECT o.status FROM attempts a
                     LEFT JOIN pumphound.paper_orders o ON o.decision_id=a.decision_id
                     ORDER BY a.decision_at DESC LIMIT 1),
                  (SELECT MAX(a.decision_at) FROM attempts a)"#,
    )
    .bind(position_id)
    .fetch_optional(&stores.postgres)
    .await?;
    let (attempts, last_status, last_at) = row.unwrap_or((0, None, None));
    if attempts == 0 {
        return Ok(Some(format!("learn-exit:{position_id}:fixed-max-hold")));
    }
    match last_status.as_deref() {
        // Terminal failure: eligible for retry after a short backoff.
        Some("REJECTED") | Some("FAILED") => {}
        // Pending, filled, or a decision that never produced an order (quote refused) —
        // the latter is also retried so a transient quote failure cannot strand a position.
        None => {}
        Some(_) => return Ok(None),
    }
    if attempts >= EXIT_MAX_ATTEMPTS {
        return Ok(None);
    }
    if let Some(last_at) = last_at {
        if (now_utc() - last_at).num_seconds() < EXIT_RETRY_BACKOFF_SECONDS {
            return Ok(None);
        }
    }
    Ok(Some(format!(
        "learn-exit:{position_id}:fixed-max-hold:retry{attempts}"
    )))
}

/// Outcome of trying to rehydrate a cold market for an exit quote.
enum ExitRehydrate {
    /// Current on-chain state, quotable.
    Snapshot(pumphound_domain::MarketSnapshot),
    /// Per-market RPC throttle is active; not new information, ask again later.
    Throttled,
    /// The state cannot be obtained honestly: graduated curve, unsupported protocol, RPC
    /// failure. Distinct from `Throttled` -- this is worth recording, not just retrying blind.
    Unavailable(String),
}

/// Rehydrate a cold market from its on-chain account so a max-hold exit can be quoted on real
/// current state. The caller records `Unavailable` reasons as a genuine (budget-counted,
/// visible) exit attempt instead of silently retrying forever -- see
/// `record_unrehydratable_exit_attempt`.
async fn unstrand_position(
    state: &Arc<AppState>,
    launch_id: Uuid,
    market: Pubkey,
    mint: &str,
    quote_mint: Pubkey,
) -> ExitRehydrate {
    let protocol: Option<String> = sqlx::query_scalar(
        "SELECT protocol FROM pumphound.launches WHERE launch_id=$1",
    )
    .bind(launch_id)
    .fetch_optional(&state.stores.postgres)
    .await
    .ok()
    .flatten();
    let protocol = match protocol.as_deref() {
        Some("pump_curve") => pumphound_domain::Protocol::PumpCurve,
        Some("pump_swap") => pumphound_domain::Protocol::PumpSwap,
        _ => return ExitRehydrate::Unavailable("launch_protocol_unknown".into()),
    };
    let Ok(mint_key) = Pubkey::from_base58(mint) else {
        return ExitRehydrate::Unavailable("invalid_mint".into());
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    else {
        return ExitRehydrate::Unavailable("http_client_build_failed".into());
    };
    let outcome =
        crate::market_rehydrate::rehydrate(&client, protocol, market, mint_key, quote_mint).await;
    match outcome {
        crate::market_rehydrate::Rehydrated::Curve(snapshot) => {
            state.markets.write().await.upsert_snapshot(snapshot.clone());
            tracing::info!(mint, slot = snapshot.as_of.slot.0, "rehydrated cold market from chain account for paper exit");
            ExitRehydrate::Snapshot(snapshot)
        }
        crate::market_rehydrate::Rehydrated::Unavailable(ref r) if r == "throttled" => {
            ExitRehydrate::Throttled
        }
        other => ExitRehydrate::Unavailable(other.reason()),
    }
}

fn exit_stall_log_throttle(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static T: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    T.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// A fixed-max-hold exit whose market cannot be rehydrated (graduated curve, unsupported
/// protocol, RPC failure, or no feature snapshot at all) used to just `continue` -- silently,
/// forever, with only a `debug!`-level log invisible at this service's configured level, and
/// with no decision ever recorded. Since no decision was ever written, the position could never
/// accumulate real attempts toward `EXIT_MAX_ATTEMPTS` either, so it neither retried nor ever
/// surfaced as exhausted: it sat open forever, holding book capacity, with no trace of why
/// (confirmed live 2026-09-09: position 01a086e1-6e4a-79c0-8851-b286ee95d387's token graduated
/// off the bonding curve minutes after entry; every scan tick since has silently no-op'd).
///
/// Record a genuine, budget-counted, no-fill attempt instead -- exactly like a rejected order --
/// so the existing retry/backoff machinery, and eventual exhaustion handling, applies here too.
async fn record_unrehydratable_exit_attempt(
    state: &Arc<AppState>,
    desk: &LiveDesk,
    experiment_id: &str,
    position_id: &str,
    launch_id: Uuid,
    reason: &str,
) -> anyhow::Result<()> {
    let throttle_key = format!("{position_id}:{reason}");
    {
        let mut throttle = exit_stall_log_throttle()
            .lock()
            .expect("exit stall throttle");
        if let Some(last) = throttle.get(&throttle_key) {
            if last.elapsed() < std::time::Duration::from_secs(600) {
                return Ok(());
            }
        }
        throttle.insert(throttle_key, std::time::Instant::now());
    }
    warn!(
        position_id,
        launch_id = %launch_id,
        reason,
        book = desk.book.key,
        "fixed-max-hold exit cannot rehydrate a market snapshot for this position; \
         recording a no-fill attempt instead of retrying silently forever"
    );
    let Some(decision_key) = next_fixed_exit_decision_key(&state.stores, position_id).await? else {
        return Ok(());
    };
    let feature_snapshot_id: Option<String> = sqlx::query_scalar(
        r#"SELECT feature_snapshot_id FROM pumphound.feature_snapshots
           WHERE launch_id=$1 ORDER BY knowledge_cutoff DESC LIMIT 1"#,
    )
    .bind(launch_id)
    .fetch_optional(&state.stores.postgres)
    .await?;
    let Some(feature_snapshot_id) = feature_snapshot_id else {
        return Ok(());
    };
    paper_ledger::persist_decision(
        &state.stores,
        Uuid::now_v7(),
        &decision_key,
        Some(desk.run_id),
        launch_id,
        None,
        &feature_snapshot_id,
        desk.book.key,
        Some(desk.book.key),
        "FULL_EXIT",
        "EXIT",
        &json!({}),
        &json!({
            "experiment_id": experiment_id,
            "strategy_version": desk.book.key,
            "position_id": position_id,
            "paper_only": true,
            "market_unrehydratable": true,
            "rehydrate_reason": reason
        }),
        &[
            "fixed_maximum_hold_reached".to_string(),
            "market_unrehydratable".to_string(),
        ],
        &json!({
            "experiment_id": experiment_id,
            "fixed_exit": true,
            "paper_only": true,
            "live_money": false,
            "market_unrehydratable": true,
            "rehydrate_reason": reason
        }),
    )
    .await?;
    Ok(())
}

fn identity_checked_snapshot(
    snapshot: Option<&pumphound_domain::MarketSnapshot>,
    expected_mint: &str,
) -> Option<pumphound_domain::MarketSnapshot> {
    if expected_mint == pumphound_domain::WSOL_MINT || expected_mint == pumphound_domain::USDC_MINT
    {
        return None;
    }
    snapshot
        .filter(|snapshot| snapshot.mint.to_string() == expected_mint)
        .cloned()
}

async fn scan_learning_entries(
    state: &Arc<AppState>,
    model: &ModelClient,
    learning: &LearningPolicies,
) -> anyhow::Result<()> {
    let desks = ensure_learning_desks(state, learning).await?;
    let execution_edge_desks_live = ensure_execution_edge_desks(state, learning).await?;
    let exec_select_desks_live = ensure_exec_select_desks(state, learning).await?;
    let readiness = crate::readiness::compute(state).await;
    if let Some(champion_desk) = desks
        .iter()
        .find(|desk| desk.book.key == STRATEGY_CHAMPION_SHORT_HORIZON_V1)
    {
        let minute = now_utc().timestamp() / 60;
        let _ = crate::model_truth::record_funnel(
            &state.stores,
            &format!("champion:{}:readiness:{minute}", champion_desk.run_id),
            champion_desk.run_id,
            None,
            None,
            "READINESS",
            if readiness.allow_new_entries {
                "entries_ready"
            } else {
                readiness
                    .reasons
                    .first()
                    .map(String::as_str)
                    .unwrap_or("entries_blocked")
            },
            &json!({"freshness": readiness.market_data == "READY", "entries_allowed": readiness.allow_new_entries}),
            &json!({"market_data": readiness.market_data, "database": readiness.database, "execution": readiness.execution, "reasons": readiness.reasons}),
        )
        .await;
    }
    if !readiness.allow_new_entries {
        tracing::info!("learning entries blocked: market/readiness not ready");
        return Ok(());
    }
    let rows = match sqlx::query(RECENT_LAUNCH_CANDIDATES_SQL)
        .fetch_all(&state.stores.postgres)
        .await
    {
        Ok(rows) => rows,
        Err(_) => {
            sqlx::query(
                r#"SELECT launch_id,mint,creator,creation_time,known_at
                   FROM pumphound.launches l
                   WHERE known_at > now() - interval '20 minutes'
                     AND mint NOT IN ($1,$2)
                     AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
                     AND NOT EXISTS (
                       SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                       WHERE iq.entity_type='LAUNCH'
                         AND iq.entity_id=l.launch_id::text
                         AND iq.status='ACTIVE'
                     )
                   ORDER BY known_at DESC LIMIT 80"#,
            )
            .bind(pumphound_domain::WSOL_MINT)
            .bind(pumphound_domain::USDC_MINT)
            .fetch_all(&state.stores.postgres)
            .await?
        }
    };
    let mut explorer_shadow_budget = 4usize;
    let shadow_models_available = paper_ledger::has_shadow_models(&state.stores)
        .await
        .unwrap_or(false);
    for row in rows {
        let launch_id: Uuid = row.get("launch_id");
        let mint: String = row.get("mint");
        let creator: String = row.get("creator");
        let creation_time: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("creation_time").ok().flatten();
        let first_observed_at: chrono::DateTime<chrono::Utc> = row.get("known_at");
        record_learning_state(
            state,
            format!("learn-observed:{launch_id}"),
            launch_id,
            None,
            None,
            None,
            LearningFunnelState::Observed,
            "launch_observed",
            false,
            creation_time,
            first_observed_at,
            None,
            None,
            &json!({"mint": mint}),
        )
        .await?;
        let snapshot = {
            let markets = state.markets.read().await;
            identity_checked_snapshot(markets.get(&mint), &mint)
        };
        let Some(snapshot) = snapshot else {
            let provisional_age = persisted_token_age_seconds(
                creation_time,
                Some(first_observed_at),
                None,
                now_utc(),
            );
            let permanently_too_old = provisional_age.is_some_and(|age| {
                age > learning.random_control.policy.execution.maximum_age_seconds
            });
            let funnel_state = if permanently_too_old {
                LearningFunnelState::TechnicallyInvalid
            } else {
                LearningFunnelState::WaitingInformation
            };
            let reason = if permanently_too_old {
                "maximum_age_exceeded"
            } else {
                "market_snapshot_unavailable"
            };
            record_learning_state(
                state,
                format!("learn-no-market:{launch_id}:{}", funnel_state.as_str()),
                launch_id,
                None,
                None,
                None,
                funnel_state,
                reason,
                !permanently_too_old,
                creation_time,
                first_observed_at,
                None,
                None,
                &json!({"provisional_age_seconds": provisional_age}),
            )
            .await?;
            continue;
        };
        let (mut vector, _) = match feature_snapshot(
            state,
            launch_id,
            &mint,
            &creator,
            creation_time,
            Some(first_observed_at),
            &snapshot,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                record_learning_state(
                    state,
                    format!(
                        "learn-waiting-feature:{launch_id}:{}",
                        decision_age_bucket(persisted_token_age_seconds(
                            creation_time,
                            Some(first_observed_at),
                            snapshot.as_of.event_time,
                            snapshot.as_of.knowledge_cutoff,
                        ))
                        .unwrap_or_default()
                    ),
                    launch_id,
                    None,
                    None,
                    None,
                    LearningFunnelState::WaitingInformation,
                    "feature_window_unavailable",
                    true,
                    creation_time,
                    first_observed_at,
                    None,
                    None,
                    &json!({"error": error.to_string()}),
                )
                .await?;
                continue;
            }
        };
        let quote_policy = &learning.random_control.policy.execution;
        vector.values.insert(
            "mark_price".into(),
            (snapshot.effective_base_atomic.0 > 0)
                .then(|| {
                    Value::from(
                        snapshot.effective_quote_atomic.0 as f64
                            / snapshot.effective_base_atomic.0 as f64,
                    )
                })
                .unwrap_or(Value::Null),
        );
        let request =
            heuristic_entry_request(desks[0].portfolio_id, launch_id, &snapshot, quote_policy)?;
        let decision_quote = quote_order(&request, &snapshot).ok();
        let (round_trip_cost_fraction, _estimated_exit_quote) =
            exact_round_trip_estimate(decision_quote.as_ref(), &request, &snapshot);
        vector.values.insert(
            "entry_impact_bps".into(),
            decision_quote
                .as_ref()
                .map(|quote| Value::from(quote.price_impact_bps))
                .unwrap_or(Value::Null),
        );
        vector.values.insert(
            "round_trip_cost_fraction".into(),
            round_trip_cost_fraction
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        vector.feature_snapshot_id = feature_snapshot_hash(&vector)?;
        paper_ledger::persist_feature_snapshot(
            &state.stores,
            launch_id,
            &mint,
            &vector,
            Some(desks[0].run_id),
        )
        .await?;
        let snapshot_at = now_utc();
        let snapshot_age_ms = (snapshot_at - snapshot.as_of.knowledge_cutoff).num_milliseconds();
        let market_snapshot_fresh =
            (0..=crate::readiness::STALE_MARKET_MS).contains(&snapshot_age_ms);
        let age_seconds = vector.values.get("age_seconds").and_then(Value::as_f64);
        let age_bucket = decision_age_bucket(age_seconds).unwrap_or_default();

        let control_desk = desks
            .iter()
            .find(|desk| desk.book.key == STRATEGY_RANDOM_CONTROL_V1)
            .ok_or_else(|| anyhow::anyhow!("random control desk missing"))?;
        let control_policy = &learning.random_control;
        let control_open = portfolio_exposure_count(
            &state.stores,
            control_desk.portfolio_id,
            control_desk.run_id,
        )
        .await?;
        let control_technical = evaluate_technical_eligibility(
            &control_policy.policy,
            &TechnicalEligibilityInput {
                age_seconds,
                quality_sufficient: quality_sufficient(&snapshot),
                liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
                round_trip_cost_fraction,
                open_positions: control_open,
            },
        );
        let control_decision = evaluate_random_control(
            &control_policy.policy,
            &launch_id.to_string(),
            &control_technical,
        );
        persist_learning_book_decision(
            state,
            control_desk,
            control_policy,
            launch_id,
            &vector,
            &snapshot,
            creation_time,
            first_observed_at,
            snapshot_at,
            age_bucket,
            &control_technical,
            &control_decision,
            &json!({
                "selection": "seeded_hash",
                "seed": control_policy.policy.deterministic_seed,
                "rate": control_policy.policy.selection_rate,
                "model_inputs": false
            }),
        )
        .await?;

        // EXECUTION-EDGE-V1: same technical gate inputs, twin simulator. Control mirrors the
        // legacy random control's seeded selection exactly; microstructure uses a transparent
        // market/curve-only rule. Neither consumes model output.
        for twin_desk in &execution_edge_desks_live {
            let twin_open = portfolio_exposure_count(
                &state.stores,
                twin_desk.portfolio_id,
                twin_desk.run_id,
            )
            .await?;
            let twin_technical = evaluate_technical_eligibility(
                &control_policy.policy,
                &TechnicalEligibilityInput {
                    age_seconds,
                    quality_sufficient: quality_sufficient(&snapshot),
                    liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                    entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
                    round_trip_cost_fraction,
                    open_positions: twin_open,
                },
            );
            let (twin_decision, evidence) =
                if twin_desk.book.key == STRATEGY_RANDOM_EXECUTION_CONTROL_V1 {
                    (
                        evaluate_random_control(
                            &control_policy.policy,
                            &launch_id.to_string(),
                            &twin_technical,
                        ),
                        json!({
                            "selection": "seeded_hash",
                            "seed": control_policy.policy.deterministic_seed,
                            "rate": control_policy.policy.selection_rate,
                            "model_inputs": false,
                            "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION
                        }),
                    )
                } else {
                    (
                        evaluate_microstructure_baseline(&twin_technical, &vector.values),
                        json!({
                            "selection": "microstructure_rule_v1",
                            "thresholds": {
                                "min_unique_buyers_30": MICROSTRUCTURE_MIN_UNIQUE_BUYERS_30,
                                "min_imbalance_30": MICROSTRUCTURE_MIN_IMBALANCE_30,
                                "min_return_30": MICROSTRUCTURE_MIN_RETURN_30,
                                "max_sell_count_30": MICROSTRUCTURE_MAX_SELL_COUNT_30
                            },
                            "model_inputs": false,
                            "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION
                        }),
                    )
                };
            persist_book_decision(
                state,
                twin_desk,
                control_policy,
                EXECUTION_EDGE_EXPERIMENT_ID,
                launch_id,
                &vector,
                &snapshot,
                creation_time,
                first_observed_at,
                snapshot_at,
                age_bucket,
                &twin_technical,
                &twin_decision,
                &evidence,
                Some(TwinRoute {
                    creator: Pubkey::from_base58(&creator).ok(),
                }),
            )
            .await?;
        }

        let serving_missingness = pumphound_paper_core::vector_serving_missingness(&vector.values);
        let select_regime_rarity = vector
            .values
            .get("curve_progress")
            .and_then(Value::as_f64)
            .map(|progress| ((progress - 0.5).abs() * 2.0).clamp(0.0, 1.0));
        let select_iv_input = InformationValueInput {
            prediction_uncertainty: None,
            missingness_novelty: serving_missingness,
            regime_rarity: select_regime_rarity,
            familiarity: None,
        };
        let select_iv_score =
            score_information_value(&learning.information_value.policy, &select_iv_input);
        let risk_snapshot = RiskSnapshot::from_values(&vector.values);
        for select_desk in &exec_select_desks_live {
            let select_open = portfolio_exposure_count(
                &state.stores,
                select_desk.portfolio_id,
                select_desk.run_id,
            )
            .await?;
            let select_technical = evaluate_technical_eligibility(
                &control_policy.policy,
                &TechnicalEligibilityInput {
                    age_seconds,
                    quality_sufficient: quality_sufficient(&snapshot),
                    liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                    entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
                    round_trip_cost_fraction,
                    open_positions: select_open,
                },
            );
            let select_policy = if select_desk.book.key == STRATEGY_IV_SELECT_V1 {
                &learning.information_value.policy
            } else {
                &control_policy.policy
            };
            let select_decision = evaluate_exec_select(
                select_desk.book,
                select_policy,
                &launch_id.to_string(),
                &select_technical,
                age_seconds,
                &vector.values,
                Some(&select_iv_score),
            );
            persist_book_decision(
                state,
                select_desk,
                control_policy,
                EXEC_SELECT_EXPERIMENT_ID,
                launch_id,
                &vector,
                &snapshot,
                creation_time,
                first_observed_at,
                snapshot_at,
                age_bucket,
                &select_technical,
                &select_decision,
                &json!({
                    "selection": select_desk.book.execution_authority,
                    "timing_regime": pumphound_paper_core::exec_select_timing(select_desk.book.key).map(|r| r.as_str()),
                    "information_value_input": select_iv_input,
                    "information_value_score": select_iv_score,
                    "risk_snapshot": risk_snapshot,
                    "model_inputs": false,
                    "confidence_sizing": false,
                    "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION
                }),
                Some(TwinRoute {
                    creator: Pubkey::from_base58(&creator).ok(),
                }),
            )
            .await?;
        }
        if let Some(champion_desk) = desks
            .iter()
            .find(|desk| desk.book.key == STRATEGY_CHAMPION_SHORT_HORIZON_V1)
        {
            let champion_policy = learning.for_book(champion_desk.book);
            let champion_open = portfolio_exposure_count(
                &state.stores,
                champion_desk.portfolio_id,
                champion_desk.run_id,
            )
            .await?;
            let champion_technical = evaluate_technical_eligibility(
                &champion_policy.policy,
                &TechnicalEligibilityInput {
                    age_seconds,
                    quality_sufficient: quality_sufficient(&snapshot),
                    liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                    entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
                    round_trip_cost_fraction,
                    open_positions: champion_open,
                },
            );
            let champions = paper_ledger::champions(&state.stores)
                .await
                .unwrap_or_default();
            let prediction_attempt = if champion_technical.action != OpportunityAction::Buy {
                Err("technical_gate_not_passed".to_string())
            } else if !champions.contains_key(SHORT_MOMENTUM_TARGET) {
                Err(format!("missing_champion:{SHORT_MOMENTUM_TARGET}"))
            } else {
                model.predict(&vector).await
            };
            let control_prediction = prediction_attempt.as_ref().ok();
            if let Some(prediction) = control_prediction {
                let _ =
                    paper_ledger::persist_control_predictions(&state.stores, launch_id, prediction)
                        .await;
            }

            // Validate every score against the database registry before it can
            // affect control. The serving process may have a stale local role.
            let validate_target = |target: &str| -> Result<(), String> {
                control_prediction
                    .ok_or_else(|| {
                        prediction_attempt
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(|| "prediction_unavailable".into())
                    })?
                    .validate_target(target, &vector, &champions)
            };
            let momentum_validation = validate_target(SHORT_MOMENTUM_TARGET);
            let adverse_validation = validate_target(ADVERSE_MOVE_TARGET);
            let expected_mfe_validation = validate_target(EXPECTED_MFE_TARGET);
            let validated_probability = |target: &str, validation: &Result<(), String>| {
                validation
                    .is_ok()
                    .then(|| {
                        control_prediction.and_then(|prediction| prediction.probability(target))
                    })
                    .flatten()
            };
            let momentum = validated_probability(SHORT_MOMENTUM_TARGET, &momentum_validation);
            let adverse = validated_probability(ADVERSE_MOVE_TARGET, &adverse_validation);
            let expected_mfe = validated_probability(EXPECTED_MFE_TARGET, &expected_mfe_validation);
            let champion_familiarity = momentum_validation
                .is_ok()
                .then(|| {
                    control_prediction.and_then(|prediction| {
                        prediction
                            .familiarity
                            .get(SHORT_MOMENTUM_TARGET)
                            .copied()
                            .flatten()
                    })
                })
                .flatten();
            // The champion's own familiarity artifact is the only currently
            // persisted OOD proxy. Explorer novelty must never gate control.
            let champion_ood =
                champion_familiarity.map(|familiarity| (1.0 - familiarity).clamp(0.0, 1.0));
            let champion_input_quality = ChampionInputQuality {
                missingness: Some(serving_missingness),
                ood: champion_ood,
            };
            let champion_decision = evaluate_champion_short_horizon(
                &champion_technical,
                momentum,
                adverse,
                expected_mfe,
                champion_familiarity,
                !market_snapshot_fresh,
                champion_input_quality,
            );
            let mut consumed_model_versions = serde_json::Map::new();
            for (target, validation) in [
                (SHORT_MOMENTUM_TARGET, &momentum_validation),
                (ADVERSE_MOVE_TARGET, &adverse_validation),
                (EXPECTED_MFE_TARGET, &expected_mfe_validation),
            ] {
                if validation.is_ok() {
                    if let Some(version) = control_prediction
                        .and_then(|prediction| prediction.target_model_versions.get(target))
                        .and_then(|version| version.as_deref())
                    {
                        consumed_model_versions.insert(target.into(), json!(version));
                    }
                }
            }
            let gates = json!({
                "technical": champion_technical.action == OpportunityAction::Buy,
                "prediction_available": momentum.is_some() && adverse.is_some(),
                "familiarity": champion_familiarity
                    .is_some_and(|value| value >= CHAMPION_MINIMUM_FAMILIARITY),
                "data_quality": serving_missingness <= CHAMPION_MAX_MISSINGNESS
                    && champion_ood.is_none_or(|value| value <= CHAMPION_MAX_OOD),
                "momentum": momentum
                    .is_some_and(|value| value >= CHAMPION_MINIMUM_MOMENTUM),
                "adverse": adverse
                    .is_some_and(|value| value <= CHAMPION_MAXIMUM_ADVERSE),
                "liquidity_and_impact": champion_technical.action == OpportunityAction::Buy,
                "freshness": market_snapshot_fresh,
                "capacity": champion_open < champion_policy.policy.execution.maximum_positions,
                "decision_buy": champion_decision.action == OpportunityAction::Buy
            });
            let evidence = json!({
                "prediction_attempt_error": prediction_attempt.as_ref().err(),
                "target_validation": {
                    (SHORT_MOMENTUM_TARGET): momentum_validation.as_ref().err(),
                    (ADVERSE_MOVE_TARGET): adverse_validation.as_ref().err(),
                    (EXPECTED_MFE_TARGET): expected_mfe_validation.as_ref().err()
                },
                "missingness": serving_missingness,
                "ood_proxy": champion_ood,
                "familiarity": champion_familiarity,
                "momentum": momentum,
                "adverse": adverse,
                "expected_mfe": expected_mfe,
                "market_snapshot_age_ms": snapshot_age_ms,
                "decision": champion_decision
            });
            let funnel_prefix =
                format!("champion:{}:{launch_id}:{age_bucket}", champion_desk.run_id);
            let _ = crate::model_truth::record_funnel(
                &state.stores,
                &format!("{funnel_prefix}:snapshot"),
                champion_desk.run_id,
                Some(launch_id),
                Some(&vector.feature_snapshot_id),
                "SNAPSHOT_SEEN",
                "feature_snapshot_persisted",
                &json!({}),
                &json!({"knowledge_cutoff": vector.as_of.knowledge_cutoff}),
            )
            .await;
            let prediction_reason = prediction_attempt
                .as_ref()
                .err()
                .cloned()
                .or_else(|| momentum_validation.as_ref().err().cloned())
                .or_else(|| adverse_validation.as_ref().err().cloned())
                .unwrap_or_else(|| "validated_control_prediction".into());
            let _ = crate::model_truth::record_funnel(
                &state.stores,
                &format!("{funnel_prefix}:prediction"),
                champion_desk.run_id,
                Some(launch_id),
                Some(&vector.feature_snapshot_id),
                "PREDICTION",
                &prediction_reason,
                &json!({"available": momentum.is_some() && adverse.is_some()}),
                &evidence,
            )
            .await;
            let _ = crate::model_truth::record_funnel(
                &state.stores,
                &format!("{funnel_prefix}:evaluation"),
                champion_desk.run_id,
                Some(launch_id),
                Some(&vector.feature_snapshot_id),
                "EVALUATION",
                champion_decision
                    .reasons
                    .first()
                    .map(String::as_str)
                    .unwrap_or("strategy_evaluated"),
                &gates,
                &evidence,
            )
            .await;
            persist_learning_book_decision(
                state,
                champion_desk,
                champion_policy,
                launch_id,
                &vector,
                &snapshot,
                creation_time,
                first_observed_at,
                snapshot_at,
                age_bucket,
                &champion_technical,
                &champion_decision,
                &json!({
                    "champion": {
                        "authority": "CONTROL",
                        "prediction_id": control_prediction.map(|prediction| prediction.prediction_id.clone()),
                        "shadow_cannot_create_orders": true,
                        "momentum": momentum,
                        "adverse": adverse,
                        "expected_mfe": expected_mfe,
                        "familiarity": champion_familiarity,
                        "missingness": serving_missingness,
                        "ood_proxy": champion_ood,
                        "model_versions": Value::Object(consumed_model_versions),
                        "target_validation": evidence["target_validation"]
                    }
                }),
            )
            .await?;
        }

        let explorer_desk = desks
            .iter()
            .find(|desk| desk.book.key == STRATEGY_INFORMATION_VALUE_EXPLORER_V1)
            .ok_or_else(|| anyhow::anyhow!("information value desk missing"))?;
        let explorer_policy = &learning.information_value;
        let explorer_open = portfolio_exposure_count(
            &state.stores,
            explorer_desk.portfolio_id,
            explorer_desk.run_id,
        )
        .await?;
        let explorer_technical = evaluate_technical_eligibility(
            &explorer_policy.policy,
            &TechnicalEligibilityInput {
                age_seconds,
                quality_sufficient: quality_sufficient(&snapshot),
                liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
                round_trip_cost_fraction,
                open_positions: explorer_open,
            },
        );
        let explorer_key = format!(
            "learn-entry:{}:{launch_id}:{age_bucket}:{}",
            ADAPTIVE_EXPERIMENT_ID, explorer_desk.book.key
        );
        if paper_ledger::decision_for_key(&state.stores, &explorer_key)
            .await?
            .is_some()
            || learning_has_active_exposure(
                &state.stores,
                explorer_desk.portfolio_id,
                explorer_desk.run_id,
                launch_id,
            )
            .await?
        {
            continue;
        }
        let shadow_prediction = if explorer_technical.action == OpportunityAction::Buy
            && shadow_models_available
            && explorer_shadow_budget > 0
        {
            explorer_shadow_budget -= 1;
            model.predict_shadow(&vector).await.ok()
        } else {
            None
        };
        let persisted_shadow_versions = if let Some(prediction) = shadow_prediction.as_ref() {
            paper_ledger::persist_shadow_predictions(&state.stores, launch_id, prediction)
                .await
                .unwrap_or_else(|error| {
                    warn!(error = %error, "failed to persist explorer shadow predictions");
                    json!({})
                })
        } else {
            json!({})
        };
        let uncertainty = shadow_prediction.as_ref().and_then(prediction_uncertainty);
        let familiarity = shadow_prediction
            .as_ref()
            .and_then(|prediction| prediction.minimum_familiarity());
        let regime_rarity = vector
            .values
            .get("curve_progress")
            .and_then(Value::as_f64)
            .map(|progress| ((progress - 0.5).abs() * 2.0).clamp(0.0, 1.0));
        let information_input = InformationValueInput {
            prediction_uncertainty: uncertainty,
            missingness_novelty: serving_missingness,
            regime_rarity,
            familiarity,
        };
        let information_score =
            score_information_value(&explorer_policy.policy, &information_input);
        let explorer_decision = evaluate_information_value_explorer(
            &explorer_policy.policy,
            &explorer_technical,
            &information_score,
        );
        persist_learning_book_decision(
            state,
            explorer_desk,
            explorer_policy,
            launch_id,
            &vector,
            &snapshot,
            creation_time,
            first_observed_at,
            snapshot_at,
            age_bucket,
            &explorer_technical,
            &explorer_decision,
            &json!({
                "information_value_input": information_input,
                "information_value_score": information_score,
                "shadow_prediction": shadow_prediction,
                "model_versions": persisted_shadow_versions,
                "control_decision_was_persisted_before_model_inference": true
            }),
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Marks a decision as routed through the Execution Twin (EXECUTION-EDGE books).
struct TwinRoute {
    creator: Option<Pubkey>,
}

#[allow(clippy::too_many_arguments)]
async fn persist_learning_book_decision(
    state: &Arc<AppState>,
    desk: &LiveDesk,
    loaded: &LoadedLearningPolicy,
    launch_id: Uuid,
    vector: &FeatureVector,
    snapshot: &pumphound_domain::MarketSnapshot,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_observed_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: chrono::DateTime<chrono::Utc>,
    age_bucket: u64,
    technical: &OpportunityDecision,
    decision: &OpportunityDecision,
    learning_evidence: &Value,
) -> anyhow::Result<()> {
    persist_book_decision(
        state,
        desk,
        loaded,
        ADAPTIVE_EXPERIMENT_ID,
        launch_id,
        vector,
        snapshot,
        creation_time,
        first_observed_at,
        snapshot_at,
        age_bucket,
        technical,
        decision,
        learning_evidence,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn persist_book_decision(
    state: &Arc<AppState>,
    desk: &LiveDesk,
    loaded: &LoadedLearningPolicy,
    experiment_id: &str,
    launch_id: Uuid,
    vector: &FeatureVector,
    snapshot: &pumphound_domain::MarketSnapshot,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_observed_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: chrono::DateTime<chrono::Utc>,
    age_bucket: u64,
    technical: &OpportunityDecision,
    decision: &OpportunityDecision,
    learning_evidence: &Value,
    twin: Option<TwinRoute>,
) -> anyhow::Result<()> {
    let technical_state = learning_funnel_state(technical);
    let retryable = technical_state == LearningFunnelState::WaitingInformation;
    if technical.action != OpportunityAction::Buy {
        record_state_for(
            state,
            experiment_id,
            format!(
                "learn-technical:{}:{}:{}:{}",
                launch_id,
                desk.book.key,
                age_bucket,
                technical_state.as_str()
            ),
            launch_id,
            Some(&vector.feature_snapshot_id),
            None,
            Some(desk.book.key),
            technical_state,
            technical
                .reasons
                .first()
                .map(String::as_str)
                .unwrap_or("technical_evaluation"),
            retryable,
            creation_time,
            first_observed_at,
            Some(snapshot_at),
            Some(now_utc()),
            &json!({"technical": technical}),
        )
        .await?;
        if retryable {
            return Ok(());
        }
    } else {
        record_state_for(
            state,
            experiment_id,
            format!("learn-evaluated:{launch_id}:{}:{age_bucket}", desk.book.key),
            launch_id,
            Some(&vector.feature_snapshot_id),
            None,
            Some(desk.book.key),
            LearningFunnelState::StrategyEvaluated,
            "technical_checks_passed",
            false,
            creation_time,
            first_observed_at,
            Some(snapshot_at),
            Some(now_utc()),
            &json!({"technical": technical}),
        )
        .await?;
    }
    let decision_key = format!(
        "learn-entry:{}:{launch_id}:{age_bucket}:{}",
        experiment_id, desk.book.key
    );
    if paper_ledger::decision_for_key(&state.stores, &decision_key)
        .await?
        .is_some()
        || learning_has_active_exposure(&state.stores, desk.portfolio_id, desk.run_id, launch_id)
            .await?
    {
        return Ok(());
    }
    let decision_at = now_utc();
    let prediction_id = learning_evidence
        .pointer("/champion/prediction_id")
        .or_else(|| learning_evidence.pointer("/shadow_prediction/prediction_id"))
        .and_then(Value::as_str);
    let model_versions = learning_evidence
        .pointer("/champion/model_versions")
        .or_else(|| learning_evidence.get("model_versions"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let simulator_version = if twin.is_some() {
        pumphound_execution_twin::SIMULATOR_VERSION
    } else {
        pumphound_paper_core::SIMULATOR_LEGACY
    };
    let exact_inputs = json!({
        "experiment_id": experiment_id,
        "simulator_version": simulator_version,
        "strategy_version": desk.book.key,
        "feature_snapshot": vector,
        "technical_eligibility": technical,
        "learning_evidence": learning_evidence,
        "prediction_id": prediction_id,
        "model_versions": model_versions.clone(),
        "market_snapshot": snapshot,
        "policy": loaded.raw,
        "policy_content_sha256": loaded.sha256,
        "execution_envelope_policy": loaded.policy.strategy_version,
        "paper_only": true,
        "live_money": false
    });
    let decision_id = paper_ledger::persist_decision(
        &state.stores,
        Uuid::now_v7(),
        &decision_key,
        Some(desk.run_id),
        launch_id,
        prediction_id,
        &vector.feature_snapshot_id,
        desk.book.key,
        Some(desk.book.key),
        paper_ledger::action_name(decision),
        "ENTRY",
        &model_versions,
        &exact_inputs,
        &decision.reasons,
        &json!({
            "experiment_id": experiment_id,
            "adaptive_learning": twin.is_none(),
            "simulator_version": simulator_version,
            "opportunity": decision,
            "execution_authority": desk.book.execution_authority,
            "shadow_non_interference": desk.book.key == STRATEGY_RANDOM_CONTROL_V1,
            "paper_only": true,
            "live_money": false
        }),
    )
    .await?;
    let final_state = learning_funnel_state(decision);
    record_state_for(
        state,
        experiment_id,
        format!("learn-decision:{decision_id}:{}", final_state.as_str()),
        launch_id,
        Some(&vector.feature_snapshot_id),
        Some(decision_id),
        Some(desk.book.key),
        final_state,
        decision
            .reasons
            .first()
            .map(String::as_str)
            .unwrap_or("strategy_evaluated"),
        false,
        creation_time,
        first_observed_at,
        Some(snapshot_at),
        Some(decision_at),
        &json!({"decision": decision, "learning_evidence": learning_evidence}),
    )
    .await?;
    if decision.action != OpportunityAction::Buy {
        return Ok(());
    }
    let policy = &loaded.policy.execution;
    let mut request = heuristic_entry_request(desk.portfolio_id, launch_id, snapshot, policy)?;
    request.decision_id = decision_id;
    request.client_request_id = decision_id;
    request.quote_id = Uuid::now_v7();
    request.policy_version = desk.book.key.to_string();
    if twin.is_some() {
        // See crates/paper-core/src/execution_edge.rs module doc: the shared control_policy's
        // fixed_sol_atomic (0.005 SOL) is right for the legacy-simulator book it was tuned for,
        // but at that size a single ATA-rent charge was ~42% of every EXECUTION-EDGE-V1 entry —
        // cost dominated by trade size, not by simulator realism or selection quality.
        request.input.atomic = Atomic(pumphound_paper_core::EXECUTION_EDGE_ENTRY_ATOMIC);
    }
    let queued = if let Some(route) = twin {
        let model_versions = model_versions
            .as_object()
            .map(|m| m.values().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        crate::execution_twin_bridge::queue_twin_order(
            state,
            &request,
            snapshot,
            experiment_id,
            desk.book.key,
            model_versions,
            &crate::execution_twin_bridge::default_config(),
            policy.maximum_arrival_wait_ms,
            decision_id,
            desk.run_id,
            route.creator,
        )
        .await?
        .is_some()
    } else if let Ok(mut quote) = quote_order(&request, snapshot) {
        quote.quote_id = request.quote_id;
        paper_ledger::queue_order(
            &state.stores,
            &request,
            &quote,
            policy.arrival_delay_ms,
            policy.maximum_arrival_wait_ms,
        )
        .await?;
        true
    } else {
        false
    };
    if queued {
        let _ = crate::hot_set::promote(
            &state.stores,
            &snapshot.mint.to_string(),
            Some(launch_id),
            "paper_entry",
            desk.book.key,
        )
        .await;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn record_learning_state(
    state: &AppState,
    idempotency_key: String,
    launch_id: Uuid,
    feature_snapshot_id: Option<&str>,
    decision_id: Option<Uuid>,
    strategy_version: Option<&str>,
    funnel_state: LearningFunnelState,
    reason_code: &str,
    retryable: bool,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_observed_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: Option<chrono::DateTime<chrono::Utc>>,
    decision_at: Option<chrono::DateTime<chrono::Utc>>,
    payload: &Value,
) -> anyhow::Result<()> {
    record_state_for(
        state,
        ADAPTIVE_EXPERIMENT_ID,
        idempotency_key,
        launch_id,
        feature_snapshot_id,
        decision_id,
        strategy_version,
        funnel_state,
        reason_code,
        retryable,
        creation_time,
        first_observed_at,
        snapshot_at,
        decision_at,
        payload,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn record_state_for(
    state: &AppState,
    experiment_id: &str,
    idempotency_key: String,
    launch_id: Uuid,
    feature_snapshot_id: Option<&str>,
    decision_id: Option<Uuid>,
    strategy_version: Option<&str>,
    funnel_state: LearningFunnelState,
    reason_code: &str,
    retryable: bool,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_observed_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: Option<chrono::DateTime<chrono::Utc>>,
    decision_at: Option<chrono::DateTime<chrono::Utc>>,
    payload: &Value,
) -> anyhow::Result<()> {
    paper_ledger::persist_lifecycle_event(
        &state.stores,
        &idempotency_key,
        experiment_id,
        launch_id,
        feature_snapshot_id,
        decision_id,
        strategy_version,
        funnel_state,
        reason_code,
        retryable,
        creation_time,
        first_observed_at,
        snapshot_at,
        decision_at,
        payload,
    )
    .await
}

async fn learning_has_active_exposure(
    stores: &pumphound_storage::Stores,
    portfolio_id: Uuid,
    run_id: Uuid,
    launch_id: Uuid,
) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS (
             SELECT 1 FROM pumphound.paper_positions p
             WHERE p.portfolio_id=$1 AND p.simulation_run_id=$2 AND p.launch_id=$3
               AND p.status IN ('OPEN','UNMARKABLE')
               AND NOT EXISTS (
                 SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                 WHERE iq.status='ACTIVE' AND (
                   (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                   OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
                 )
               )
             UNION ALL
             SELECT 1 FROM pumphound.paper_orders o
             JOIN pumphound.decisions d ON d.decision_id=o.decision_id
             WHERE o.portfolio_id=$1 AND o.simulation_run_id=$2 AND d.launch_id=$3
               AND o.side='buy'
               AND o.status IN ('DECIDED','QUEUED','ARRIVED','FILLED','POSITION')
               AND NOT EXISTS (
                 SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                 WHERE iq.status='ACTIVE' AND (
                   (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
                   OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
                 )
               )
           )"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(launch_id)
    .fetch_one(&stores.postgres)
    .await?)
}

fn prediction_uncertainty(prediction: &AggregatePrediction) -> Option<f64> {
    let values = prediction
        .probabilities
        .values()
        .filter_map(|value| *value)
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .map(|value| 1.0 - (2.0 * value - 1.0).abs())
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

async fn scan_learning_exits(
    state: &Arc<AppState>,
    learning: &LearningPolicies,
) -> anyhow::Result<()> {
    let mut desks = ensure_learning_desks(state, learning).await?;
    desks.extend(ensure_execution_edge_desks(state, learning).await?);
    desks.extend(ensure_exec_select_desks(state, learning).await?);
    for desk in desks {
        let twin_desk = uses_execution_twin(desk.book.key);
        let experiment_id = if is_exec_select(desk.book.key) {
            EXEC_SELECT_EXPERIMENT_ID
        } else if twin_desk {
            EXECUTION_EDGE_EXPERIMENT_ID
        } else {
            ADAPTIVE_EXPERIMENT_ID
        };
        let loaded = if twin_desk {
            &learning.random_control
        } else {
            learning.for_book(desk.book)
        };
        let policy = &loaded.policy.execution;
        for position in paper_ledger::open_positions_for(
            &state.stores,
            Some(desk.portfolio_id),
            Some(desk.run_id),
        )
        .await?
        {
            let launch_id = parse_uuid(&position, "launch_id")?;
            let position_id = value_str(&position, "position_id")?;
            let mint = value_str(&position, "base_mint")?;
            let opened_at = position
                .get("opened_at")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok())
                .unwrap_or_else(now_utc);
            let hold_reached = ((now_utc() - opened_at).num_seconds().max(0) as u64)
                >= policy.maximum_hold_seconds;
            let market = Pubkey::from_base58(value_str(&position, "market")?)?;
            let quote_mint = Pubkey::from_base58(value_str(&position, "quote_mint")?)?;
            let snapshot = {
                let markets = state.markets.read().await;
                identity_checked_snapshot(markets.get(mint), mint)
            };
            let snapshot = match snapshot {
                Some(snapshot) => Some(snapshot),
                None => {
                    if !hold_reached {
                        continue;
                    }
                    match unstrand_position(state, launch_id, market, mint, quote_mint).await {
                        ExitRehydrate::Snapshot(snapshot) => Some(snapshot),
                        ExitRehydrate::Throttled => None,
                        ExitRehydrate::Unavailable(reason) => {
                            record_unrehydratable_exit_attempt(
                                state,
                                &desk,
                                experiment_id,
                                position_id,
                                launch_id,
                                &reason,
                            )
                            .await?;
                            None
                        }
                    }
                }
            };
            let Some(snapshot) = snapshot else {
                continue;
            };
            let feature_snapshot_id: Option<String> = sqlx::query_scalar(
                r#"SELECT feature_snapshot_id FROM pumphound.feature_snapshots
                   WHERE launch_id=$1 ORDER BY knowledge_cutoff DESC LIMIT 1"#,
            )
            .bind(launch_id)
            .fetch_optional(&state.stores.postgres)
            .await?;
            let Some(feature_snapshot_id) = feature_snapshot_id else {
                record_unrehydratable_exit_attempt(
                    state,
                    &desk,
                    experiment_id,
                    position_id,
                    launch_id,
                    "no_feature_snapshot_for_launch",
                )
                .await?;
                continue;
            };
            let mut exit_reasons = vec!["fixed_maximum_hold_reached".to_string()];
            let mut risk_class: Option<&str> = None;
            let mut entry_risk = RiskSnapshot::default();
            let mut current_risk = RiskSnapshot::default();
            if is_exec_select(desk.book.key) {
                let current_values = sqlx::query_scalar::<_, Value>(
                    "SELECT values FROM pumphound.feature_snapshots WHERE feature_snapshot_id=$1",
                )
                .bind(&feature_snapshot_id)
                .fetch_optional(&state.stores.postgres)
                .await?;
                if let Some(Value::Object(values)) = current_values {
                    current_risk = RiskSnapshot::from_values(&values);
                }
                let entry_values = sqlx::query_scalar::<_, Value>(
                    r#"SELECT exact_inputs->'learning_evidence'->'risk_snapshot'
                       FROM pumphound.decisions
                       WHERE launch_id=$1 AND policy_version=$2 AND decision_kind='ENTRY' AND action='BUY'
                       ORDER BY decision_at DESC LIMIT 1"#,
                )
                .bind(launch_id)
                .bind(desk.book.key)
                .fetch_optional(&state.stores.postgres)
                .await?;
                if let Some(value) = entry_values {
                    if let Ok(parsed) = serde_json::from_value::<RiskSnapshot>(value) {
                        entry_risk = parsed;
                    }
                }
                if let pumphound_paper_core::PositionRisk::Exit { class, reasons } =
                    evaluate_open_position_risk(&entry_risk, &current_risk)
                {
                    risk_class = Some(class);
                    exit_reasons = reasons;
                    let _ = sqlx::query(
                        r#"INSERT INTO pumphound.position_risk_events
                           (risk_event_id, experiment_id, strategy_version, portfolio_id,
                            simulation_run_id, position_id, launch_id, mint, class, reason_codes,
                            entry_snapshot, current_snapshot, action)
                           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'FULL_EXIT')"#,
                    )
                    .bind(Uuid::now_v7())
                    .bind(experiment_id)
                    .bind(desk.book.key)
                    .bind(desk.portfolio_id)
                    .bind(desk.run_id)
                    .bind(parse_uuid(&position, "position_id")?)
                    .bind(launch_id)
                    .bind(mint)
                    .bind(class)
                    .bind(json!(exit_reasons))
                    .bind(json!(entry_risk))
                    .bind(json!(current_risk))
                    .execute(&state.stores.postgres)
                    .await;
                }
            }
            if !hold_reached && risk_class.is_none() {
                continue;
            }
            // A rejected exit (e.g. REJECTED_ARRIVAL_DEADLINE across a restart) must be retried,
            // otherwise the position is stranded forever and the book silently deadlocks at
            // position capacity. Each retry is a new immutable decision with its own key.
            let Some(decision_key) =
                next_fixed_exit_decision_key(&state.stores, position_id).await?
            else {
                continue;
            };
            let base_atomic: u128 = value_str(&position, "base_atomic")?.parse()?;
            let decision_id = paper_ledger::persist_decision(
                &state.stores,
                Uuid::now_v7(),
                &decision_key,
                Some(desk.run_id),
                launch_id,
                None,
                &feature_snapshot_id,
                desk.book.key,
                Some(desk.book.key),
                "FULL_EXIT",
                "EXIT",
                &json!({}),
                &json!({
                    "experiment_id": experiment_id,
                    "strategy_version": desk.book.key,
                    "fixed_exit_policy": {
                        "maximum_hold_seconds": policy.maximum_hold_seconds
                    },
                    "continuous_risk": risk_class,
                    "position_id": position_id,
                    "paper_only": true
                }),
                &exit_reasons,
                &json!({
                    "experiment_id": experiment_id,
                    "fixed_exit": true,
                    "paper_only": true,
                    "live_money": false
                }),
            )
            .await?;
            let request = PaperOrderRequest {
                client_request_id: decision_id,
                portfolio_id: desk.portfolio_id,
                launch_id: Some(launch_id),
                market,
                side: TradeSide::Sell,
                input: AssetAmount {
                    mint: snapshot.mint,
                    atomic: Atomic(base_atomic),
                    decimals: 6,
                },
                output_mint: quote_mint,
                max_slippage_bps: policy.emergency_maximum_impact_bps,
                quote_id: Uuid::now_v7(),
                policy_version: desk.book.key.to_string(),
                decision_id,
            };
            if twin_desk {
                let creator =
                    crate::execution_twin_bridge::launch_creator(&state.stores, launch_id).await;
                // Exits inherit the entry's trace so the whole position is one trace_id.
                let trace_id = sqlx::query_scalar::<_, Option<Uuid>>(
                    r#"SELECT o.trace_id FROM pumphound.paper_orders o
                       JOIN pumphound.paper_fills f ON f.order_id=o.order_id
                       JOIN pumphound.acquisition_lots l ON l.fill_id=f.fill_id
                       WHERE l.portfolio_id=$1 AND l.simulation_run_id=$2 AND l.base_mint=$3
                       ORDER BY l.acquired_at LIMIT 1"#,
                )
                .bind(desk.portfolio_id)
                .bind(desk.run_id)
                .bind(mint)
                .fetch_optional(&state.stores.postgres)
                .await?
                .flatten()
                .unwrap_or(decision_id);
                crate::execution_twin_bridge::queue_twin_order(
                    state,
                    &request,
                    &snapshot,
                    experiment_id,
                    desk.book.key,
                    Vec::new(),
                    &crate::execution_twin_bridge::default_config(),
                    policy.maximum_arrival_wait_ms,
                    trace_id,
                    desk.run_id,
                    creator,
                )
                .await?;
            } else if let Ok(mut quote) = quote_order(&request, &snapshot) {
                quote.quote_id = request.quote_id;
                paper_ledger::queue_order(
                    &state.stores,
                    &request,
                    &quote,
                    policy.arrival_delay_ms,
                    policy.maximum_arrival_wait_ms,
                )
                .await?;
            }
        }
    }
    Ok(())
}

fn entry_decision_key(
    experiment_id: &str,
    launch_id: Uuid,
    age_bucket: Option<u64>,
    strategy: &str,
) -> String {
    let bucket = age_bucket
        .map(|bucket| bucket.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("exp-entry:{experiment_id}:{launch_id}:{bucket}:{strategy}")
}

fn intent_decision_key(
    experiment_id: &str,
    launch_id: Uuid,
    age_bucket: Option<u64>,
    strategy: &str,
) -> String {
    let bucket = age_bucket
        .map(|bucket| bucket.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("exp-intent:{experiment_id}:{launch_id}:{bucket}:{strategy}")
}

fn universe_decision_key(experiment_id: &str, launch_id: Uuid, age_bucket: Option<u64>) -> String {
    let bucket = age_bucket
        .map(|bucket| bucket.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("universe:{experiment_id}:{launch_id}:{bucket}")
}

async fn scan_heuristic_entries(
    state: &Arc<AppState>,
    model: &ModelClient,
    experiment: &ExperimentPolicies,
    mode: OperatingMode,
) -> anyhow::Result<()> {
    let desks = ensure_parallel_desks(state, experiment, mode).await?;
    if !crate::readiness::allow_new_entries(state).await {
        tracing::info!("heuristic entries blocked: market/readiness not ready");
        return Ok(());
    }
    complete_pending_research(state, experiment, &desks).await?;
    let Some(quote_desk) = desks.first() else {
        return Ok(());
    };
    let shared_policy = &experiment.heuristic.policy;
    let rows = sqlx::query(RECENT_LAUNCH_CANDIDATES_SQL)
        .fetch_all(&state.stores.postgres)
        .await?;
    let mut queued_books = HashSet::<&str>::new();
    for row in rows {
        let launch_id: Uuid = row.get("launch_id");
        let mint: String = row.get("mint");
        let creator: String = row.get("creator");
        let creation_time: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("creation_time").ok().flatten();
        let first_seen_time: Option<chrono::DateTime<chrono::Utc>> = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("known_at")
            .ok()
            .flatten();
        let snapshot = {
            let markets = state.markets.read().await;
            identity_checked_snapshot(markets.get(&mint), &mint)
        };
        let Some(snapshot) = snapshot else {
            continue;
        };
        let (mut vector, indicators) = match feature_snapshot(
            state,
            launch_id,
            &mint,
            &creator,
            creation_time,
            first_seen_time,
            &snapshot,
        )
        .await
        {
            Ok(value) => value,
            Err(error) => {
                warn!(%launch_id, %error, "heuristic feature snapshot unavailable");
                continue;
            }
        };
        let age_seconds = vector.values.get("age_seconds").and_then(Value::as_f64);
        let age_bucket = decision_age_bucket(age_seconds);
        let request =
            heuristic_entry_request(quote_desk.portfolio_id, launch_id, &snapshot, shared_policy)?;
        let decision_quote = quote_order(&request, &snapshot).ok();
        let (round_trip_cost_fraction, estimated_exit_quote) =
            exact_round_trip_estimate(decision_quote.as_ref(), &request, &snapshot);
        vector.values.insert(
            "entry_impact_bps".into(),
            decision_quote
                .as_ref()
                .map(|quote| Value::from(quote.price_impact_bps))
                .unwrap_or(Value::Null),
        );
        vector.values.insert(
            "round_trip_cost_fraction".into(),
            round_trip_cost_fraction
                .map(Value::from)
                .unwrap_or(Value::Null),
        );
        vector.feature_snapshot_id = feature_snapshot_hash(&vector)?;
        paper_ledger::persist_feature_snapshot(
            &state.stores,
            launch_id,
            &mint,
            &vector,
            Some(quote_desk.run_id),
        )
        .await?;
        let curve_velocity_per_second = curve_velocity(&state.stores, launch_id, &vector).await?;
        let scout_hash = scout_selection_hash(launch_id, age_bucket.unwrap_or_default());
        let shared_input = HeuristicOpportunityInput {
            age_seconds,
            unique_buyers: vector
                .values
                .get("unique_buyers_lifetime")
                .and_then(Value::as_f64)
                .map(|value| value.max(0.0) as u64),
            buy_sell_imbalance: vector
                .values
                .get("buy_sell_imbalance_30")
                .and_then(Value::as_f64),
            return_30s: vector.values.get("return_30s").and_then(Value::as_f64),
            curve_progress: vector.values.get("curve_progress").and_then(Value::as_f64),
            curve_velocity_per_second,
            data_quality_score: Some(snapshot.quality.score),
            quality_sufficient: quality_sufficient(&snapshot),
            liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
            entry_impact_bps: decision_quote.as_ref().map(|quote| quote.price_impact_bps),
            round_trip_cost_fraction,
            open_positions: 0,
            scout_selected: true,
        };
        let universe = evaluate_shared_universe(&shared_input);
        persist_universe_decision(
            state,
            quote_desk.run_id,
            launch_id,
            age_bucket,
            &vector.feature_snapshot_id,
            &shared_input,
            &universe,
            &experiment.experiment_id,
        )
        .await?;
        let shadow_models_available = mode == OperatingMode::LivePaperShadowMl
            && paper_ledger::has_shadow_models(&state.stores).await?;
        let shared_shadow = if shadow_models_available && universe.action == OpportunityAction::Buy
        {
            let prediction = model.predict_shadow(&vector).await.ok();
            spawn_shadow_prediction(
                state.clone(),
                model.clone(),
                launch_id,
                quote_desk.run_id,
                vector.clone(),
                paper_ledger::action_name(&universe).to_string(),
                SHARED_UNIVERSE_VERSION.to_string(),
                "ENTRY",
            );
            prediction
        } else {
            None
        };
        let graph = local_graph_from_indicators(&indicators);
        let shadow_gate = shared_shadow.as_ref().map(shadow_gate_from_prediction);
        for desk in &desks {
            if queued_books.contains(desk.book.key) {
                continue;
            }
            let entry_key = entry_decision_key(
                &experiment.experiment_id,
                launch_id,
                age_bucket,
                desk.book.key,
            );
            if paper_ledger::decision_for_key(&state.stores, &entry_key)
                .await?
                .is_some()
            {
                continue;
            }
            let intent_key = intent_decision_key(
                &experiment.experiment_id,
                launch_id,
                age_bucket,
                desk.book.key,
            );
            if paper_ledger::decision_for_key(&state.stores, &intent_key)
                .await?
                .is_some()
            {
                continue;
            }
            let loaded = experiment.for_book(desk.book);
            let policy = &loaded.policy;
            let (sized_atomic, sizing_evidence) = confidence_scaled_atomic(
                policy.fixed_sol_atomic,
                desk.book.key,
                shared_shadow.as_ref(),
            );
            let open_positions =
                portfolio_exposure_count(&state.stores, desk.portfolio_id, desk.run_id).await?;
            let mut input = shared_input.clone();
            input.open_positions = open_positions;
            input.scout_selected = scout_selected(
                launch_id,
                age_bucket.unwrap_or_default(),
                policy.scout_selection_rate,
            );
            let book_decision = evaluate_independent_book(
                desk.book,
                &experiment.heuristic.policy,
                &experiment.exploration.policy,
                &experiment.control.policy,
                &input,
                &graph,
                shadow_gate.as_ref(),
            );
            let heuristic_evidence = json!({
                "heuristic_input": input,
                "market_state": {
                    "market_snapshot_id": decision_quote
                        .as_ref()
                        .map(|quote| quote.market_snapshot_id.clone()),
                    "market": snapshot.market,
                    "mint": snapshot.mint,
                    "quote_mint": snapshot.quote_mint,
                    "as_of_slot": snapshot.as_of.slot.0,
                    "event_time": snapshot.as_of.event_time,
                    "knowledge_cutoff": snapshot.as_of.knowledge_cutoff,
                    "estimated_liquidity_quote_atomic": snapshot.effective_quote_atomic.0,
                    "quality": snapshot.quality
                },
                "quote_state": {
                    "decision_quote_id": decision_quote.as_ref().map(|quote| quote.quote_id),
                    "entry_market_snapshot_id": decision_quote
                        .as_ref()
                        .map(|quote| quote.market_snapshot_id.clone()),
                    "entry_impact_bps": decision_quote
                        .as_ref()
                        .map(|quote| quote.price_impact_bps),
                    "estimated_exit_impact_bps": estimated_exit_quote
                        .as_ref()
                        .map(|quote| quote.price_impact_bps),
                    "entry_fee_components": decision_quote
                        .as_ref()
                        .map(|quote| quote.fee_components.clone())
                        .unwrap_or_default(),
                    "estimated_exit_fee_components": estimated_exit_quote
                        .as_ref()
                        .map(|quote| quote.fee_components.clone())
                        .unwrap_or_default(),
                    "round_trip_cost_fraction": round_trip_cost_fraction,
                    "entry_quote": decision_quote,
                    "estimated_exit_quote": estimated_exit_quote
                },
                "scout_selection": {
                    "hash": scout_hash,
                    "rate": policy.scout_selection_rate,
                    "selected": input.scout_selected,
                    "age_bucket_seconds": age_bucket
                },
                "operating_mode": state.mode,
                "experiment_id": experiment.experiment_id,
                "strategy_version": desk.book.key,
                "signal_policy": policy.strategy_version,
                "policy_content_sha256": loaded.sha256,
                "shared_universe": universe,
                "funnel_stage": funnel_stage_for_reasons(&book_decision.reasons)
            });
            let exact_inputs = json!({
                "experiment_id": experiment.experiment_id,
                "strategy_version": desk.book.key,
                "operating_mode": state.mode,
                "policy_content_sha256": loaded.sha256,
                "feature_snapshot": vector,
                "heuristic_input": heuristic_evidence["heuristic_input"],
                "market_state": heuristic_evidence["market_state"],
                "quote_state": heuristic_evidence["quote_state"],
                "scout_selection": heuristic_evidence["scout_selection"],
                "decision_quote": heuristic_evidence["quote_state"]["entry_quote"],
                "round_trip_cost_fraction": round_trip_cost_fraction,
                "creator_wallet_cluster_indicators": indicators,
                "local_graph": graph,
                "policy": loaded.raw,
                "ml_status": desk.book.ml_status,
                "ai_status": desk.book.ai_status,
                "capital": "SIMULATED",
                "live_money": false,
                "signal_policy": policy.strategy_version,
                "independent_book": true,
                "shadow_prediction": shared_shadow,
                "position_sizing": sizing_evidence,
                "funnel_stage": funnel_stage_for_reasons(&book_decision.reasons)
            });
            ensure_complete_heuristic_evidence(&exact_inputs)?;
            let action_name = paper_ledger::action_name(&book_decision);
            let decision_id = paper_ledger::persist_decision(
                &state.stores,
                Uuid::now_v7(),
                &entry_key,
                Some(desk.run_id),
                launch_id,
                None,
                &vector.feature_snapshot_id,
                desk.book.key,
                Some(desk.book.key),
                action_name,
                "ENTRY",
                &json!({}),
                &exact_inputs,
                &book_decision.reasons,
                &json!({
                    "experiment_id": experiment.experiment_id,
                    "strategy_version": desk.book.key,
                    "operating_mode": state.mode,
                    "policy_content_sha256": loaded.sha256,
                    "ml_status": desk.book.ml_status,
                    "execution_authority": desk.book.execution_authority,
                    "opportunity": book_decision,
                    "shared_universe": universe,
                    "heuristic_evidence": heuristic_evidence,
                    "research_filter": json!({"status": "ASYNC"}),
                    "live_money": false
                }),
            )
            .await?;
            if desk.book.allows_enrichment && book_decision.action == OpportunityAction::Buy {
                crate::candidate_enrichment::spawn_for_candidate(
                    state.clone(),
                    decision_id,
                    action_name,
                );
            }
            if book_decision.action != OpportunityAction::Buy {
                continue;
            }
            let mut book_request =
                heuristic_entry_request(desk.portfolio_id, launch_id, &snapshot, policy)?;
            book_request.input.atomic = Atomic(sized_atomic);
            let Some(mut quote) = quote_order(&book_request, &snapshot).ok() else {
                continue;
            };
            // A scaled-up size can push slippage past what this book's own policy allows even
            // though the base size would have cleared it — re-check rather than trust the
            // pre-sizing decision, and skip the trade (never silently fall back to the base size,
            // which would hide that scaling pushed this past the book's own risk limit).
            if quote.price_impact_bps > policy.maximum_entry_impact_bps {
                continue;
            }
            book_request.decision_id = decision_id;
            book_request.client_request_id = decision_id;
            book_request.quote_id = Uuid::now_v7();
            book_request.policy_version = desk.book.key.to_string();
            quote.quote_id = book_request.quote_id;
            paper_ledger::queue_order(
                &state.stores,
                &book_request,
                &quote,
                policy.arrival_delay_ms,
                policy.maximum_arrival_wait_ms,
            )
            .await?;
            let _ = crate::hot_set::promote(
                &state.stores,
                &snapshot.mint.to_string(),
                Some(launch_id),
                "paper_entry",
                desk.book.key,
            )
            .await;
            queued_books.insert(desk.book.key);
        }
    }
    Ok(())
}

fn shadow_gate_from_prediction(prediction: &AggregatePrediction) -> ShadowMlGateInput {
    ShadowMlGateInput {
        exit_hazard: prediction.probability(EXIT_HAZARD_TARGET),
        short_momentum: prediction.probability(SHORT_MOMENTUM_TARGET),
        adverse_move: prediction.probability(ADVERSE_MOVE_TARGET),
        maximum_exit_hazard: SHADOW_MAXIMUM_EXIT_HAZARD,
        minimum_short_momentum: pumphound_paper_core::SHADOW_MINIMUM_SHORT_MOMENTUM,
        maximum_adverse_move: pumphound_paper_core::SHADOW_MAXIMUM_ADVERSE_MOVE,
    }
}

async fn persist_universe_decision(
    state: &AppState,
    _run_id: Uuid,
    launch_id: Uuid,
    age_bucket: Option<u64>,
    feature_snapshot_id: &str,
    input: &HeuristicOpportunityInput,
    universe: &OpportunityDecision,
    experiment_id: &str,
) -> anyhow::Result<()> {
    let key = universe_decision_key(experiment_id, launch_id, age_bucket);
    if paper_ledger::decision_for_key(&state.stores, &key)
        .await?
        .is_some()
    {
        return Ok(());
    }
    paper_ledger::persist_decision(
        &state.stores,
        Uuid::now_v7(),
        &key,
        None,
        launch_id,
        None,
        feature_snapshot_id,
        SHARED_UNIVERSE_VERSION,
        Some(SHARED_UNIVERSE_VERSION),
        paper_ledger::action_name(universe),
        "ENTRY",
        &json!({}),
        &json!({
            "experiment_id": experiment_id,
            "heuristic_input": input,
            "shared_universe": universe,
            "funnel_stage": funnel_stage_for_reasons(&universe.reasons)
        }),
        &universe.reasons,
        &json!({
            "experiment_id": experiment_id,
            "shared_universe": true,
            "live_money": false
        }),
    )
    .await?;
    Ok(())
}

async fn complete_pending_research(
    state: &Arc<AppState>,
    experiment: &ExperimentPolicies,
    desks: &[LiveDesk],
) -> anyhow::Result<()> {
    for intent in paper_ledger::pending_research_intents(&state.stores).await? {
        let Some(strategy) = intent
            .get("heuristic_version")
            .and_then(Value::as_str)
            .and_then(book_by_key)
        else {
            continue;
        };
        let Some(desk) = desks.iter().find(|desk| desk.book.key == strategy.key) else {
            continue;
        };
        let launch_id = parse_uuid(&intent, "launch_id")?;
        let intent_id = parse_uuid(&intent, "decision_id")?;
        let decision_at = intent
            .get("decision_at")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok())
            .unwrap_or_else(now_utc);
        let exact_inputs = intent.get("exact_inputs").cloned().unwrap_or(Value::Null);
        let age_seconds = exact_inputs
            .pointer("/heuristic_input/age_seconds")
            .and_then(Value::as_f64);
        let entry_key = entry_decision_key(
            &experiment.experiment_id,
            launch_id,
            decision_age_bucket(age_seconds),
            strategy.key,
        );
        if paper_ledger::decision_for_key(&state.stores, &entry_key)
            .await?
            .is_some()
        {
            continue;
        }
        let investigation =
            paper_ledger::latest_launch_investigation(&state.stores, launch_id, decision_at)
                .await?;
        let dossier = crate::candidate_enrichment::dossier(&state.stores, intent_id)
            .await?
            .unwrap_or(Value::Null);
        let filter = evaluate_research_filter(&research_input_from_artifacts(
            investigation.as_ref(),
            (now_utc() - decision_at).num_milliseconds() as f64 / 1_000.0,
            shared_inferred_funder_wallets(&dossier),
        ));
        if filter.action == OpportunityAction::Watch
            && filter
                .reasons
                .iter()
                .any(|reason| reason == "research_filter_pending")
        {
            continue;
        }
        let mint = exact_inputs
            .pointer("/market_state/mint")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let snapshot = if let Some(mint) = mint.as_deref() {
            identity_checked_snapshot(state.markets.read().await.get(mint), mint)
        } else {
            None
        };
        let feature_snapshot_id = intent
            .get("feature_snapshot_id")
            .and_then(Value::as_str)
            .unwrap_or("research-filter");
        let decision_id = paper_ledger::persist_decision(
            &state.stores,
            Uuid::now_v7(),
            &entry_key,
            Some(desk.run_id),
            launch_id,
            None,
            feature_snapshot_id,
            strategy.key,
            Some(strategy.key),
            paper_ledger::action_name(&filter),
            "ENTRY",
            &json!({}),
            &exact_inputs,
            &filter.reasons,
            &json!({
                "strategy_version": strategy.key,
                "execution_authority": strategy.execution_authority,
                "research_filter": {
                    "status": "COMPLETED",
                    "intent_decision_id": intent_id,
                    "investigation_id": investigation
                        .as_ref()
                        .and_then(|value| value.get("investigation_id").cloned())
                },
                "opportunity": filter,
                "live_money": false
            }),
        )
        .await?;
        if filter.action != OpportunityAction::Buy {
            continue;
        }
        let Some(snapshot) = snapshot else {
            continue;
        };
        let loaded = experiment.for_book(strategy);
        let mut request =
            heuristic_entry_request(desk.portfolio_id, launch_id, &snapshot, &loaded.policy)?;
        request.decision_id = decision_id;
        request.client_request_id = decision_id;
        request.policy_version = strategy.key.to_string();
        if let Ok(mut quote) = quote_order(&request, &snapshot) {
            request.quote_id = Uuid::now_v7();
            quote.quote_id = request.quote_id;
            paper_ledger::queue_order(
                &state.stores,
                &request,
                &quote,
                loaded.policy.arrival_delay_ms,
                loaded.policy.maximum_arrival_wait_ms,
            )
            .await?;
        }
    }
    Ok(())
}

async fn scan_heuristic_exits(
    state: &Arc<AppState>,
    model: &ModelClient,
    experiment: &ExperimentPolicies,
    mode: OperatingMode,
) -> anyhow::Result<()> {
    let desks = ensure_parallel_desks(state, experiment, mode).await?;
    for desk in &desks {
        let loaded = experiment.for_book(desk.book);
        let policy = &loaded.policy;
        let ml_status = desk.book.ml_status;
        for position in paper_ledger::open_positions_for(
            &state.stores,
            Some(desk.portfolio_id),
            Some(desk.run_id),
        )
        .await?
        {
            let launch_id = parse_uuid(&position, "launch_id")?;
            let portfolio_id = parse_uuid(&position, "portfolio_id")?;
            let run_id = parse_uuid(&position, "simulation_run_id")?;
            let position_id = value_str(&position, "position_id")?;
            let mint = value_str(&position, "base_mint")?;
            let market = Pubkey::from_base58(value_str(&position, "market")?)?;
            let quote_mint = Pubkey::from_base58(value_str(&position, "quote_mint")?)?;
            let base_atomic: u128 = value_str(&position, "base_atomic")?.parse()?;
            let cost_atomic: u128 = value_str(&position, "cost_quote_atomic")?.parse()?;
            let opened_at = position
                .get("opened_at")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<chrono::DateTime<chrono::Utc>>().ok())
                .unwrap_or_else(now_utc);
            let snapshot = {
                let markets = state.markets.read().await;
                identity_checked_snapshot(
                    markets
                        .get(mint)
                        .or_else(|| markets.get(&market.to_string())),
                    mint,
                )
            };
            let Some(snapshot) = snapshot else {
                continue;
            };
            let cutoff = snapshot.as_of.event_time.unwrap_or_else(now_utc);
            let window = cutoff.timestamp().max(0) as u64 / policy.scout_cooldown_seconds;
            let decision_key = format!(
                "heuristic-exit:{}:{}:{}",
                position_id, window, desk.book.key
            );
            if paper_ledger::decision_for_key(&state.stores, &decision_key)
                .await?
                .is_some()
            {
                continue;
            }
            let launch_row = sqlx::query(
                "SELECT creator,creation_time,known_at AS first_seen_time
                 FROM pumphound.launches WHERE launch_id=$1",
            )
            .bind(launch_id)
            .fetch_one(&state.stores.postgres)
            .await?;
            let creator: String = launch_row.get("creator");
            let creation_time: Option<chrono::DateTime<chrono::Utc>> =
                launch_row.try_get("creation_time").ok().flatten();
            let first_seen_time: Option<chrono::DateTime<chrono::Utc>> =
                launch_row.try_get("first_seen_time").ok();
            let (vector, indicators) = feature_snapshot(
                state,
                launch_id,
                mint,
                &creator,
                creation_time,
                first_seen_time,
                &snapshot,
            )
            .await?;
            paper_ledger::persist_feature_snapshot(
                &state.stores,
                launch_id,
                mint,
                &vector,
                Some(run_id),
            )
            .await?;
            let mark_request = PaperOrderRequest {
                client_request_id: Uuid::nil(),
                portfolio_id,
                launch_id: Some(launch_id),
                market,
                side: TradeSide::Sell,
                input: AssetAmount {
                    mint: snapshot.mint,
                    atomic: Atomic(base_atomic),
                    decimals: 6,
                },
                output_mint: quote_mint,
                max_slippage_bps: policy.emergency_maximum_impact_bps,
                quote_id: Uuid::nil(),
                policy_version: desk.book.key.to_string(),
                decision_id: Uuid::nil(),
            };
            let mark_quote = quote_order(&mark_request, &snapshot).ok();
            let mark_return_fraction = mark_quote.as_ref().and_then(|quote| {
                (cost_atomic > 0).then(|| {
                    (quote.expected_output.atomic.0 as f64 - cost_atomic as f64)
                        / cost_atomic as f64
                })
            });
            let previous_peak =
                paper_ledger::heuristic_peak_return(&state.stores, position_id).await?;
            let peak_return_fraction = match (previous_peak, mark_return_fraction) {
                (Some(previous), Some(current)) => Some(previous.max(current)),
                (previous, current) => previous.or(current),
            };
            let imbalance = vector
                .values
                .get("buy_sell_imbalance_30")
                .and_then(Value::as_f64);
            let derived_sell_acceleration = flow_change(&vector, false);
            let input = HeuristicExitInput {
                hold_seconds: (cutoff - opened_at).num_seconds().max(0) as u64,
                mark_return_fraction,
                peak_return_fraction,
                buy_flow_deterioration: flow_change(&vector, true).map(|change| (-change).max(0.0)),
                sell_acceleration: match (
                    indicators
                        .get("creator_sell_acceleration")
                        .and_then(Value::as_f64),
                    derived_sell_acceleration,
                ) {
                    (Some(stored), Some(derived)) => Some(stored.max(derived)),
                    (stored, derived) => stored.or(derived),
                },
                buy_sell_imbalance: imbalance,
                liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
                exit_impact_bps: mark_quote.as_ref().map(|quote| quote.price_impact_bps),
            };
            let mut exit = evaluate_heuristic_exit(policy, &input);
            let mut ai_features = Value::Null;
            if desk.book.uses_research_filter {
                if let Ok(Some(investigation)) =
                    paper_ledger::latest_launch_investigation(&state.stores, launch_id, opened_at)
                        .await
                {
                    ai_features = investigation_features(Some(&investigation));
                    let filter = evaluate_research_filter(&research_input_from_artifacts(
                        Some(&investigation),
                        (now_utc() - opened_at).num_milliseconds() as f64 / 1_000.0,
                        0,
                    ));
                    if filter.action == OpportunityAction::Avoid
                        && !matches!(exit.action, ExitAction::EmergencyExit)
                    {
                        exit = ExitDecision {
                            action: ExitAction::FullExit,
                            fraction: 1.0,
                            reasons: filter.reasons,
                        };
                    }
                }
            }
            let decision_id = paper_ledger::persist_decision(
                &state.stores,
                Uuid::now_v7(),
                &decision_key,
                Some(run_id),
                launch_id,
                None,
                &vector.feature_snapshot_id,
                desk.book.key,
                Some(desk.book.key),
                exit_action_name(exit.action),
                "EXIT",
                &json!({}),
                &json!({
                    "strategy_version": desk.book.key,
                    "feature_snapshot": vector,
                    "heuristic_exit_input": input,
                    "market_snapshot": snapshot,
                    "executable_mark_quote": mark_quote,
                    "mark_return_fraction": mark_return_fraction,
                    "peak_return_fraction": peak_return_fraction,
                    "experiment_id": experiment.experiment_id,
                    "policy": loaded.raw,
                    "ml_status": ml_status,
                    "investigation_features": ai_features,
                    "capital": "SIMULATED",
                    "live_money": false
                }),
                &exit.reasons,
                &json!({
                    "strategy_version": desk.book.key,
                    "execution_authority": desk.book.execution_authority,
                    "exit": exit,
                    "position_id": position_id,
                    "unknown_mark_does_not_invent_pnl": mark_return_fraction.is_none()
                }),
            )
            .await?;
            if mode == OperatingMode::LivePaperShadowMl {
                spawn_shadow_prediction(
                    state.clone(),
                    model.clone(),
                    launch_id,
                    run_id,
                    vector.clone(),
                    exit_action_name(exit.action).into(),
                    desk.book.key.to_string(),
                    "EXIT",
                );
            }
            if exit.action == ExitAction::Hold {
                continue;
            }
            let sell_atomic = if exit.action == ExitAction::PartialExit {
                ((base_atomic as f64) * exit.fraction).floor() as u128
            } else {
                base_atomic
            };
            if sell_atomic == 0 {
                continue;
            }
            let mut request = mark_request;
            request.client_request_id = decision_id;
            request.decision_id = decision_id;
            request.quote_id = Uuid::now_v7();
            request.input.atomic = Atomic(sell_atomic);
            if let Ok(mut quote) = quote_order(&request, &snapshot) {
                quote.quote_id = request.quote_id;
                paper_ledger::queue_order(
                    &state.stores,
                    &request,
                    &quote,
                    policy.arrival_delay_ms,
                    policy.maximum_arrival_wait_ms,
                )
                .await?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn spawn_shadow_prediction(
    state: Arc<AppState>,
    model: ModelClient,
    launch_id: Uuid,
    run_id: Uuid,
    vector: FeatureVector,
    heuristic_action: String,
    heuristic_strategy: String,
    decision_kind: &'static str,
) {
    tokio::spawn(async move {
        let result = async {
            if !paper_ledger::has_shadow_models(&state.stores).await? {
                return Ok::<(), anyhow::Error>(());
            }
            let prediction = match model.predict_shadow(&vector).await {
                Ok(prediction) => prediction,
                Err(error) => {
                    paper_ledger::persist_decision(
                        &state.stores,
                        Uuid::now_v7(),
                        &format!(
                            "shadow:{}:{}:{}:unavailable",
                            decision_kind, launch_id, vector.feature_snapshot_id
                        ),
                        Some(run_id),
                        launch_id,
                        None,
                        &vector.feature_snapshot_id,
                        &heuristic_strategy,
                        Some("ML_SHADOW"),
                        "ABSTAIN",
                        decision_kind,
                        &json!({}),
                        &json!({
                            "ml_status": "SHADOW",
                            "shadow_error": error,
                            "feature_snapshot": vector,
                            "heuristic_action": heuristic_action,
                            "execution_effect": "NONE"
                        }),
                        &["shadow_prediction_unavailable".into()],
                        &json!({
                            "shadow": true,
                            "execution_effect": "NONE",
                            "actual_strategy": heuristic_strategy,
                            "actual_action_unchanged": heuristic_action,
                            "live_money": false
                        }),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let versions =
                paper_ledger::persist_shadow_predictions(&state.stores, launch_id, &prediction)
                    .await?;
            let reasons = if versions.as_object().is_some_and(|value| value.is_empty()) {
                vec!["shadow_outputs_not_registered".into()]
            } else {
                vec!["shadow_observation_only".into()]
            };
            paper_ledger::persist_decision(
                &state.stores,
                Uuid::now_v7(),
                &format!(
                    "shadow:{}:{}:{}:{}",
                    decision_kind, launch_id, vector.feature_snapshot_id, prediction.prediction_id
                ),
                Some(run_id),
                launch_id,
                Some(&prediction.prediction_id),
                &vector.feature_snapshot_id,
                &heuristic_strategy,
                Some("ML_SHADOW"),
                "ABSTAIN",
                decision_kind,
                &versions,
                &json!({
                    "aggregate_prediction": prediction,
                    "feature_snapshot": vector,
                    "heuristic_action": heuristic_action,
                    "execution_effect": "NONE",
                    "ml_status": "SHADOW"
                }),
                &reasons,
                &json!({
                    "shadow": true,
                    "execution_effect": "NONE",
                    "actual_strategy": heuristic_strategy,
                    "actual_action_unchanged": heuristic_action,
                    "live_money": false
                }),
            )
            .await?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            warn!(%launch_id, %error, "shadow prediction unavailable; heuristic decision unchanged");
        }
    });
}

async fn portfolio_exposure_count(
    stores: &pumphound_storage::Stores,
    portfolio_id: Uuid,
    run_id: Uuid,
) -> anyhow::Result<usize> {
    let count: i64 = sqlx::query_scalar(
        r#"SELECT
             (SELECT COUNT(*) FROM pumphound.paper_positions p
              WHERE p.portfolio_id=$1 AND p.simulation_run_id=$2 AND p.status IN ('OPEN','UNMARKABLE')
                AND NOT EXISTS (
                  SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                  WHERE iq.status='ACTIVE' AND (
                    (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                    OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
                  )
                ))
             +
             (SELECT COUNT(*) FROM pumphound.paper_orders o
              JOIN pumphound.decisions d ON d.decision_id=o.decision_id
              WHERE o.portfolio_id=$1 AND o.simulation_run_id=$2 AND o.side='buy'
                AND o.status IN ('DECIDED','QUEUED','ARRIVED')
                AND NOT EXISTS (
                  SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                  WHERE iq.status='ACTIVE' AND (
                    (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
                    OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
                  )
                ))"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_one(&stores.postgres)
    .await?;
    Ok(usize::try_from(count).unwrap_or(usize::MAX))
}

async fn curve_velocity(
    stores: &pumphound_storage::Stores,
    launch_id: Uuid,
    vector: &FeatureVector,
) -> anyhow::Result<Option<f64>> {
    let current = vector.values.get("curve_progress").and_then(Value::as_f64);
    let Some(current) = current else {
        return Ok(None);
    };
    let previous = sqlx::query(
        r#"SELECT values->>'curve_progress' AS progress,knowledge_cutoff
           FROM pumphound.feature_snapshots
           WHERE launch_id=$1 AND feature_snapshot_id<>$2
             AND knowledge_cutoff < $3 AND values->>'curve_progress' IS NOT NULL
           ORDER BY knowledge_cutoff DESC LIMIT 1"#,
    )
    .bind(launch_id)
    .bind(&vector.feature_snapshot_id)
    .bind(vector.as_of.knowledge_cutoff)
    .fetch_optional(&stores.postgres)
    .await?;
    let Some(previous) = previous else {
        return Ok(None);
    };
    let progress = previous
        .try_get::<Option<String>, _>("progress")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<f64>().ok());
    let at: chrono::DateTime<chrono::Utc> = previous.get("knowledge_cutoff");
    let elapsed = (vector.as_of.knowledge_cutoff - at).num_milliseconds() as f64 / 1_000.0;
    Ok(progress
        .filter(|_| elapsed > 0.0)
        .map(|value| (current - value) / elapsed))
}

const DECISION_AGE_BUCKETS_SECONDS: [u64; 8] = [5, 10, 15, 30, 60, 120, 300, 900];

fn decision_age_bucket(age_seconds: Option<f64>) -> Option<u64> {
    let age_seconds = age_seconds.filter(|age| age.is_finite() && *age >= 0.0)?;
    DECISION_AGE_BUCKETS_SECONDS
        .into_iter()
        .find(|upper_bound| age_seconds < *upper_bound as f64)
        .or(Some(900))
}

fn scout_selection_hash(launch_id: Uuid, age_bucket: u64) -> String {
    pumphound_domain::sha256_hex(format!("{launch_id}:{age_bucket}:HEURISTIC_V1").as_bytes())
}

fn scout_selected(launch_id: Uuid, age_bucket: u64, rate: f64) -> bool {
    if rate >= 1.0 {
        return true;
    }
    let hash = scout_selection_hash(launch_id, age_bucket);
    let bucket = u64::from_str_radix(&hash[..16], 16).unwrap_or(u64::MAX);
    (bucket as f64 / u64::MAX as f64) < rate
}

fn ensure_complete_heuristic_evidence(exact_inputs: &Value) -> anyhow::Result<()> {
    for path in [
        "/heuristic_input",
        "/market_state/market_snapshot_id",
        "/market_state/knowledge_cutoff",
        "/market_state/estimated_liquidity_quote_atomic",
        "/quote_state/entry_impact_bps",
        "/quote_state/estimated_exit_impact_bps",
        "/quote_state/entry_fee_components",
        "/quote_state/estimated_exit_fee_components",
        "/quote_state/round_trip_cost_fraction",
        "/scout_selection/hash",
        "/scout_selection/rate",
        "/operating_mode",
        "/strategy_version",
        "/policy_content_sha256",
    ] {
        if exact_inputs.pointer(path).is_none() {
            anyhow::bail!("heuristic_exact_inputs_missing:{path}");
        }
    }
    Ok(())
}

fn flow_change(vector: &FeatureVector, buy: bool) -> Option<f64> {
    let current = vector
        .values
        .get(if buy { "buy_quote_30" } else { "sell_quote_30" })
        .and_then(Value::as_f64)?;
    let sixty = vector
        .values
        .get(if buy { "buy_quote_60" } else { "sell_quote_60" })
        .and_then(Value::as_f64)?;
    let previous = (sixty - current).max(0.0);
    (previous > 0.0).then(|| ((current - previous) / previous).clamp(-1.0, 1.0))
}

fn heuristic_entry_request(
    portfolio_id: Uuid,
    launch_id: Uuid,
    snapshot: &pumphound_domain::MarketSnapshot,
    policy: &HeuristicPolicyV1,
) -> anyhow::Result<PaperOrderRequest> {
    Ok(PaperOrderRequest {
        client_request_id: Uuid::now_v7(),
        portfolio_id,
        launch_id: Some(launch_id),
        market: snapshot.market,
        side: TradeSide::Buy,
        input: AssetAmount {
            mint: Pubkey::from_base58(WSOL_MINT)?,
            atomic: Atomic(policy.fixed_sol_atomic),
            decimals: 9,
        },
        output_mint: snapshot.mint,
        max_slippage_bps: policy.maximum_entry_impact_bps,
        quote_id: Uuid::now_v7(),
        policy_version: policy.strategy_version.clone(),
        decision_id: Uuid::now_v7(),
    })
}

// Every book used to bet the exact same fixed_sol_atomic on every trade regardless of how strong
// the setup looked — the shadow model could tell you "this one's a coin flip" or "this one looks
// good" and it made zero difference to size. This scales the base size by the shadow prediction's
// edge (short-momentum probability net of adverse-move probability), the same momentum-minus-
// downside idea evaluate_opportunity already uses for expected_net_edge on the ML-controlled path.
//
// Bounded to a narrow band (0.4x-1.8x) on purpose: this is fractional-Kelly in spirit — bet more
// on more conviction — not literal Kelly (f* = edge/odds), because a single mis-calibrated shadow
// reading should never be able to zero out a book's exposure or blow it out to a dangerous size.
// CONTROL_FLOW_V1 is excluded: it exists specifically as a flat-bet baseline other books are
// measured against, and confidence-weighting it would defeat that purpose.
const SIZE_SCALING_MIN_MULTIPLIER: f64 = 0.4;
const SIZE_SCALING_MAX_MULTIPLIER: f64 = 1.8;
const SIZE_SCALING_EDGE_AT_MAX: f64 = 0.3;

/// GATE 9 (confidence inversion): docs/PRODUCT_TRUTH_EVIDENCE.md records the shadow
/// `momentum - adverse` edge correlating *inversely* with realized return. Until the
/// walk-forward monotonicity diagnostic (`python -m pumphound_ml.backtest.sizing_multiplier`)
/// shows higher edge ⇒ non-worse realized economics, confidence-derived sizing is disabled and
/// every book bets the fixed base size. Re-enable explicitly with
/// `PUMPHOUND_CONFIDENCE_SIZING=enabled` once that evidence exists. The evidence payload keeps
/// the band the model *would* have applied so the decision is auditable, not silently altered.
///
/// Investigated 2026-09-09 (`python -m pumphound_ml.backtest.edge_diagnosis`), gate NOT lifted:
/// a plausible mechanism exists — in `outcome_labels`, launches whose ground-truth
/// `adverse_move_2m_v1` fired (a 10%+ dip within 2 minutes) went on to hit the 5-minute pump
/// label at ~49% vs ~25% for launches that never dipped, so subtracting `adverse` may be
/// penalizing exactly the volatility this heavy-tailed asset's big winners come from. But
/// walk-forward testing of that fix (and of momentum alone, and of adverse alone) against real
/// realized returns found no formula with a *stable* sign across time folds — every candidate,
/// including the current one, flips sign between the two out-of-sample folds. Full-sample
/// correlations looked more decisive (current -0.13, adverse-flipped +0.12) but did not survive
/// the same out-of-sample split, i.e. that gap is consistent with in-sample noise, not a fix.
/// Separately: the ground-truth `short_momentum_2m_v1` label is a strong predictor of 5-minute
/// return (~-7% vs ~+97% average by label value) — but the *shadow model's predicted*
/// probability of that same label is not (~-0.02 correlation with realized return). That gap
/// says the shadow model itself is the likelier problem, not the edge formula's arithmetic: no
/// combination of a miscalibrated model's two outputs is going to fix a calibration problem.
/// Likely real fix: retrain on post-cost twin labels once `pumphound_ml.postcost` has enough
/// EXECUTION-EDGE-V1 volume, not a sign flip here.
fn confidence_sizing_enabled() -> bool {
    std::env::var("PUMPHOUND_CONFIDENCE_SIZING")
        .map(|v| v.eq_ignore_ascii_case("enabled"))
        .unwrap_or(false)
}

fn confidence_scaled_atomic(
    base_atomic: u128,
    book_key: &str,
    shadow: Option<&AggregatePrediction>,
) -> (u128, Option<Value>) {
    let (scaled, evidence) = confidence_band_atomic(base_atomic, book_key, shadow);
    if confidence_sizing_enabled() {
        return (scaled, evidence);
    }
    let evidence = evidence.map(|mut e| {
        e["applied"] = json!(false);
        e["disabled_reason"] =
            json!("GATE9_confidence_inversion_unresolved: shadow edge not shown monotone with realized return");
        e["would_have_scaled_atomic"] = e["scaled_atomic"].clone();
        e["scaled_atomic"] = json!(base_atomic.to_string());
        e
    });
    (base_atomic, evidence)
}

/// Pure sizing band (unit-tested); `confidence_scaled_atomic` decides whether it is applied.
fn confidence_band_atomic(
    base_atomic: u128,
    book_key: &str,
    shadow: Option<&AggregatePrediction>,
) -> (u128, Option<Value>) {
    if book_key == STRATEGY_CONTROL_FLOW_V1 {
        return (base_atomic, None);
    }
    let Some(prediction) = shadow else {
        return (base_atomic, None);
    };
    let (Some(momentum), Some(adverse)) = (
        prediction.probability(SHORT_MOMENTUM_TARGET),
        prediction.probability(ADVERSE_MOVE_TARGET),
    ) else {
        return (base_atomic, None);
    };
    let edge = momentum - adverse;
    let t = (edge / SIZE_SCALING_EDGE_AT_MAX).clamp(0.0, 1.0);
    let multiplier = SIZE_SCALING_MIN_MULTIPLIER
        + t * (SIZE_SCALING_MAX_MULTIPLIER - SIZE_SCALING_MIN_MULTIPLIER);
    let scaled = ((base_atomic as f64) * multiplier).round().max(1.0) as u128;
    (
        scaled,
        Some(json!({
            "base_atomic": base_atomic.to_string(),
            "scaled_atomic": scaled.to_string(),
            "multiplier": multiplier,
            "shadow_momentum_probability": momentum,
            "shadow_adverse_move_probability": adverse,
            "edge": edge,
            "min_multiplier": SIZE_SCALING_MIN_MULTIPLIER,
            "max_multiplier": SIZE_SCALING_MAX_MULTIPLIER,
            "edge_at_max_multiplier": SIZE_SCALING_EDGE_AT_MAX
        })),
    )
}

async fn scan_ml_entries(
    state: &Arc<AppState>,
    model: &ModelClient,
    loaded_policy: Option<&LoadedPolicy>,
) -> anyhow::Result<()> {
    let champions = paper_ledger::champions(&state.stores).await?;
    let missing_targets = missing_required_targets(&champions);
    let portfolio = if let Some(policy) = loaded_policy.filter(|_| missing_targets.is_empty()) {
        let fixed = fixed_order_atomic()?;
        Some(
            paper_ledger::ensure_default_portfolio(
                &state.stores,
                &state.mode,
                &policy.policy.policy_version,
                fixed.saturating_mul(500) as i128,
                0,
            )
            .await?,
        )
    } else {
        None
    };
    let rows = sqlx::query(RECENT_LAUNCH_CANDIDATES_SQL)
        .fetch_all(&state.stores.postgres)
        .await?;
    for row in rows {
        let launch_id: Uuid = row.get("launch_id");
        let mint: String = row.get("mint");
        let creator: String = row.get("creator");
        let creation_time: Option<chrono::DateTime<chrono::Utc>> =
            row.try_get("creation_time").ok().flatten();
        let first_seen_time: Option<chrono::DateTime<chrono::Utc>> = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("known_at")
            .ok()
            .flatten();
        let snap = {
            let markets = state.markets.read().await;
            identity_checked_snapshot(markets.get(&mint), &mint)
        };
        let Some(snapshot) = snap else {
            continue;
        };
        let (vector, indicators) = match feature_snapshot(
            state,
            launch_id,
            &mint,
            &creator,
            creation_time,
            first_seen_time,
            &snapshot,
        )
        .await
        {
            Ok(value) => value,
            Err(_) => continue,
        };
        let (portfolio_id, run_id) = portfolio.unzip();
        paper_ledger::persist_feature_snapshot(&state.stores, launch_id, &mint, &vector, run_id)
            .await?;

        let Some(policy) = loaded_policy else {
            persist_abstention(
                state,
                launch_id,
                run_id,
                &vector,
                paper_ledger::SYSTEM_ABSTAIN_POLICY,
                &json!({}),
                "policy_artifact_absent",
                json!({"snapshot": vector}),
            )
            .await?;
            continue;
        };
        if !missing_targets.is_empty() {
            let reason = format!("missing_champions:{}", missing_targets.join(","));
            persist_abstention(
                state,
                launch_id,
                run_id,
                &vector,
                &policy.policy.policy_version,
                &json!(target_version_map(&champions)),
                &reason,
                json!({"snapshot": vector, "missing_targets": missing_targets}),
            )
            .await?;
            continue;
        }
        if let Err(reason) = champions_compatible(&champions, &vector.schema_version) {
            persist_abstention(
                state,
                launch_id,
                run_id,
                &vector,
                &policy.policy.policy_version,
                &json!(target_version_map(&champions)),
                &reason,
                json!({
                    "snapshot": vector,
                    "target_model_versions": target_version_map(&champions),
                    "champion_manifests": champions.iter().map(|(target, champion)| {
                        (target.clone(), champion.manifest.clone())
                    }).collect::<std::collections::BTreeMap<_,_>>()
                }),
            )
            .await?;
            continue;
        }
        let prediction = match model.predict(&vector).await {
            Ok(prediction) => prediction,
            Err(reason) => {
                persist_abstention(
                    state,
                    launch_id,
                    run_id,
                    &vector,
                    &policy.policy.policy_version,
                    &json!(target_version_map(&champions)),
                    &reason,
                    json!({
                        "snapshot": vector,
                        "target_model_versions": target_version_map(&champions)
                    }),
                )
                .await?;
                continue;
            }
        };
        let prediction_error = prediction
            .validate_for(&vector, &champions, policy.policy.minimum_familiarity)
            .err();
        if let Some(reason) = prediction_error.as_deref() {
            persist_abstention(
                state,
                launch_id,
                run_id,
                &vector,
                &policy.policy.policy_version,
                &json!(target_version_map(&champions)),
                reason,
                json!({
                    "snapshot": vector,
                    "aggregate_prediction": prediction,
                    "expected_target_model_versions": target_version_map(&champions)
                }),
            )
            .await?;
            continue;
        }
        paper_ledger::persist_predictions(&state.stores, launch_id, &prediction, &champions)
            .await?;
        if snapshot.quote_mint.to_string() != WSOL_MINT {
            persist_abstention(
                state,
                launch_id,
                run_id,
                &vector,
                &policy.policy.policy_version,
                &json!(target_version_map(&champions)),
                "fixed_sol_size_quote_mismatch",
                json!({
                    "snapshot": vector,
                    "prediction": prediction,
                    "quote_mint": snapshot.quote_mint
                }),
            )
            .await?;
            continue;
        }
        let quote_request = entry_request(
            portfolio_id.expect("portfolio exists with policy and champion"),
            launch_id,
            &snapshot,
            &policy.policy,
        )?;
        let decision_quote = quote_order(&quote_request, &snapshot).ok();
        let round_trip_cost_fraction =
            exact_round_trip_cost_fraction(decision_quote.as_ref(), &quote_request, &snapshot);
        let input = opportunity_input(
            &prediction,
            prediction_error.as_deref(),
            decision_quote.as_ref(),
            round_trip_cost_fraction,
            &snapshot,
            &indicators,
        );
        let decision = evaluate_opportunity(Some(&policy.policy), &input);
        let exact_inputs = json!({
            "feature_snapshot": vector,
            "aggregate_prediction": prediction,
            "opportunity_input": input,
            "expected_net_edge_definition": "momentum_p-collapse_p-exit_hazard_p-round_trip_cost_fraction",
            "decision_quote": decision_quote,
            "round_trip_cost_fraction": round_trip_cost_fraction,
            "liquidity_quote_atomic": snapshot.effective_quote_atomic,
            "creator_wallet_cluster_indicators": indicators,
            "policy": policy.raw,
            "live_money": false
        });
        let decision_key = format!(
            "entry:{}:{}:{}:{}",
            launch_id,
            vector.feature_snapshot_id,
            pumphound_domain::sha256_hex(
                serde_json::to_string(&target_version_map(&champions))?.as_bytes()
            ),
            policy.policy.policy_version
        );
        let action_name = paper_ledger::action_name(&decision);
        let decision_id = paper_ledger::persist_decision(
            &state.stores,
            Uuid::now_v7(),
            &decision_key,
            run_id,
            launch_id,
            Some(&prediction.prediction_id),
            &vector.feature_snapshot_id,
            &policy.policy.policy_version,
            None,
            action_name,
            "ENTRY",
            &json!(target_version_map(&champions)),
            &exact_inputs,
            &decision.reasons,
            &json!({"opportunity": decision, "live_money": false}),
        )
        .await?;
        crate::investigator::spawn_for_decision(state.clone(), decision_id, action_name);
        if decision.action != OpportunityAction::Buy {
            continue;
        }
        let Some(mut quote) = decision_quote else {
            continue;
        };
        let mut request = quote_request;
        request.decision_id = decision_id;
        request.client_request_id = decision_id;
        request.quote_id = Uuid::now_v7();
        quote.quote_id = request.quote_id;
        let order_id = paper_ledger::queue_order(
            &state.stores,
            &request,
            &quote,
            policy.policy.arrival_delay_ms,
            policy.policy.maximum_arrival_wait_ms,
        )
        .await?;
        info!(%order_id, %launch_id, "autonomous paper order queued");
        break;
    }
    Ok(())
}

async fn scan_ml_exits(
    state: &Arc<AppState>,
    model: &ModelClient,
    loaded_policy: &LoadedPolicy,
    champions: &ChampionSet,
) -> anyhow::Result<()> {
    for position in paper_ledger::open_positions(&state.stores).await? {
        let launch_id = parse_uuid(&position, "launch_id")?;
        let portfolio_id = parse_uuid(&position, "portfolio_id")?;
        let run_id = parse_uuid(&position, "simulation_run_id")?;
        let mint = value_str(&position, "base_mint")?;
        let market = Pubkey::from_base58(value_str(&position, "market")?)?;
        let quote_mint = Pubkey::from_base58(value_str(&position, "quote_mint")?)?;
        let base_atomic: u128 = value_str(&position, "base_atomic")?.parse()?;
        let snapshot = {
            let markets = state.markets.read().await;
            identity_checked_snapshot(
                markets
                    .get(mint)
                    .or_else(|| markets.get(&market.to_string())),
                mint,
            )
        };
        let Some(snapshot) = snapshot else {
            continue;
        };
        let launch_row = sqlx::query(
            "SELECT creator,creation_time,known_at AS first_seen_time
                 FROM pumphound.launches WHERE launch_id=$1",
        )
        .bind(launch_id)
        .fetch_one(&state.stores.postgres)
        .await?;
        let creator: String = launch_row.get("creator");
        let creation_time: Option<chrono::DateTime<chrono::Utc>> =
            launch_row.try_get("creation_time").ok().flatten();
        let first_seen_time: Option<chrono::DateTime<chrono::Utc>> =
            launch_row.try_get("first_seen_time").ok();
        let (vector, indicators) = match feature_snapshot(
            state,
            launch_id,
            mint,
            &creator,
            creation_time,
            first_seen_time,
            &snapshot,
        )
        .await
        {
            Ok(value) => value,
            Err(_) => continue,
        };
        paper_ledger::persist_feature_snapshot(
            &state.stores,
            launch_id,
            mint,
            &vector,
            Some(run_id),
        )
        .await?;
        if champions_compatible(champions, &vector.schema_version).is_err() {
            continue;
        }
        let prediction = match model.predict(&vector).await {
            Ok(p)
                if p.validate_for(&vector, champions, loaded_policy.policy.minimum_familiarity)
                    .is_ok() =>
            {
                p
            }
            _ => continue,
        };
        paper_ledger::persist_predictions(&state.stores, launch_id, &prediction, champions).await?;
        let exit_input = ExitInput {
            collapse_probability: prediction.probability("severe_collapse_1h_v1"),
            exit_probability: prediction.probability("exit_hazard_60s_v1"),
            data_quality_sufficient: quality_sufficient(&snapshot),
            liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
            creator_sell_acceleration: indicator(&vector, "creator_sell_acceleration"),
            wallet_deterioration: indicator(&vector, "wallet_deterioration"),
            cluster_deterioration: indicator(&vector, "cluster_deterioration"),
        };
        let exit = evaluate_exit(&loaded_policy.policy, &exit_input);
        let decision_key = format!(
            "exit:{}:{}:{}:{}",
            position["position_id"],
            vector.feature_snapshot_id,
            pumphound_domain::sha256_hex(
                serde_json::to_string(&target_version_map(champions))?.as_bytes()
            ),
            loaded_policy.policy.policy_version
        );
        let decision_id = paper_ledger::persist_decision(
            &state.stores,
            Uuid::now_v7(),
            &decision_key,
            Some(run_id),
            launch_id,
            Some(&prediction.prediction_id),
            &vector.feature_snapshot_id,
            &loaded_policy.policy.policy_version,
            None,
            exit_action_name(exit.action),
            "EXIT",
            &json!(target_version_map(champions)),
            &json!({
                "feature_snapshot": vector,
                "prediction": prediction,
                "exit_input": exit_input,
                "indicators": indicators,
                "market_snapshot": snapshot,
                "live_money": false
            }),
            &exit.reasons,
            &json!({"exit": exit, "position_id": position["position_id"]}),
        )
        .await?;
        if exit.action == ExitAction::Hold {
            continue;
        }
        let sell_atomic = if exit.action == ExitAction::PartialExit {
            ((base_atomic as f64) * exit.fraction).floor() as u128
        } else {
            base_atomic
        };
        if sell_atomic == 0 {
            continue;
        }
        let request = PaperOrderRequest {
            client_request_id: decision_id,
            portfolio_id,
            launch_id: Some(launch_id),
            market,
            side: TradeSide::Sell,
            input: AssetAmount {
                mint: snapshot.mint,
                atomic: Atomic(sell_atomic),
                decimals: 6,
            },
            output_mint: quote_mint,
            max_slippage_bps: loaded_policy.policy.maximum_price_impact_bps,
            quote_id: Uuid::now_v7(),
            policy_version: loaded_policy.policy.policy_version.clone(),
            decision_id,
        };
        if let Ok(mut quote) = quote_order(&request, &snapshot) {
            quote.quote_id = request.quote_id;
            paper_ledger::queue_order(
                &state.stores,
                &request,
                &quote,
                loaded_policy.policy.arrival_delay_ms,
                loaded_policy.policy.maximum_arrival_wait_ms,
            )
            .await?;
        }
    }
    Ok(())
}

async fn settle_recoverable(state: &Arc<AppState>) -> anyhow::Result<()> {
    for order in paper_ledger::recoverable_orders(&state.stores).await? {
        if now_utc() > order.arrival_deadline_at {
            paper_ledger::reject_order(&state.stores, order.order_id, "REJECTED_ARRIVAL_DEADLINE")
                .await?;
            continue;
        }
        if let Some(twin) = crate::execution_twin_bridge::twin_payload(&order.payload) {
            if order.status == "ARRIVED" {
                // Twin orders are settled atomically after record_arrival; an ARRIVED twin order
                // means a crash between the two steps. Finish with the stored arrival quote.
                let slot = state.health.read().await.last_slot as i64;
                paper_ledger::settle_arrived(&state.stores, order.order_id, slot).await?;
                continue;
            }
            let arrival_snapshot = {
                let markets = state.markets.read().await;
                identity_checked_snapshot(
                    markets
                        .get(&order.request.market.to_string())
                        .or_else(|| markets.get(&twin.intent.mint.to_string())),
                    &twin.intent.mint.to_string(),
                )
            };
            if let Err(error) = crate::execution_twin_bridge::settle_twin_order(
                state,
                order.order_id,
                &order.request,
                &twin,
                arrival_snapshot.as_ref(),
            )
            .await
            {
                tracing::warn!(%error, order_id=%order.order_id, "twin settlement failed");
            }
            continue;
        }
        if order.status == "ARRIVED" && order.arrival_quote.is_some() {
            let slot = state.health.read().await.last_slot as i64;
            paper_ledger::settle_arrived(&state.stores, order.order_id, slot).await?;
            continue;
        }
        let snapshot = {
            let markets = state.markets.read().await;
            let expected_mint = match order.request.side {
                TradeSide::Buy => order.request.output_mint.to_string(),
                TradeSide::Sell => order.request.input.mint.to_string(),
            };
            identity_checked_snapshot(
                markets
                    .get(&order.request.market.to_string())
                    .or_else(|| markets.get(&expected_mint)),
                &expected_mint,
            )
        };
        let Some(snapshot) = snapshot else {
            paper_ledger::reject_order(&state.stores, order.order_id, "REJECTED_STALE_STATE")
                .await?;
            continue;
        };
        match quote_order(&order.request, &snapshot) {
            Ok(quote) => {
                paper_ledger::record_arrival(&state.stores, order.order_id, &snapshot, &quote)
                    .await?;
                paper_ledger::settle_arrived(
                    &state.stores,
                    order.order_id,
                    snapshot.as_of.slot.0 as i64,
                )
                .await?;
            }
            Err(error) => {
                paper_ledger::reject_order(
                    &state.stores,
                    order.order_id,
                    &format!("REJECTED_LIQUIDITY:{error}"),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn feature_snapshot(
    state: &Arc<AppState>,
    launch_id: Uuid,
    mint: &str,
    creator: &str,
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_seen_time: Option<chrono::DateTime<chrono::Utc>>,
    snapshot: &pumphound_domain::MarketSnapshot,
) -> anyhow::Result<(FeatureVector, Value)> {
    let age = persisted_token_age_seconds(
        creation_time,
        first_seen_time,
        snapshot.as_of.event_time,
        snapshot.as_of.knowledge_cutoff,
    );
    let in_memory_buyers = {
        let windows = state.windows.read().await;
        windows
            .get(mint)
            .map(|window| window.unique_buyers() as u64)
            .unwrap_or_default()
    };
    let durable_buyers: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(DISTINCT source_node)::bigint
           FROM pumphound.graph_facts
           WHERE asset_mint=$1 AND edge_type='BOUGHT'
             AND slot <= $2 AND known_at <= $3 AND superseded_at IS NULL
             AND canonicality <> 'orphaned'"#,
    )
    .bind(mint)
    .bind(snapshot.as_of.slot.0 as i64)
    .bind(snapshot.as_of.knowledge_cutoff)
    .fetch_one(&state.stores.postgres)
    .await?;
    let buyers = lifetime_buyers(in_memory_buyers, durable_buyers);
    let mut vector = {
        let windows = state.windows.read().await;
        let Some(window) = windows.get(mint) else {
            anyhow::bail!("feature_window_unavailable");
        };
        window.compute(
            &launch_id.to_string(),
            60,
            snapshot.as_of.clone(),
            age,
            curve_progress(snapshot),
            buyers as usize,
            Some(creator),
        )
    };
    vector.quality = snapshot.quality.clone();
    vector.values.insert(
        "unique_buyers_in_memory".into(),
        Value::from(in_memory_buyers),
    );
    vector.values.insert(
        "unique_buyers_durable".into(),
        Value::from(durable_buyers.max(0) as u64),
    );
    vector
        .values
        .insert("unique_buyers_lifetime".into(), Value::from(buyers));
    vector.values.insert(
        "return_30s".into(),
        vector
            .values
            .get("return_30")
            .cloned()
            .unwrap_or(Value::Null),
    );
    let indicators = intelligence_indicators(state, creator, mint, &snapshot.as_of).await?;
    for name in [
        "creator_risk",
        "wallet_risk",
        "cluster_risk",
        "creator_sell_acceleration",
        "wallet_deterioration",
        "cluster_deterioration",
    ] {
        vector.values.insert(
            name.into(),
            indicators.get(name).cloned().unwrap_or(Value::Null),
        );
    }
    for name in OPTIONAL_GRAPH_FEATURE_NAMES {
        vector.values.insert(
            name.into(),
            indicators.get(name).cloned().unwrap_or(Value::Null),
        );
    }
    vector.feature_snapshot_id = feature_snapshot_hash(&vector)?;
    Ok((vector, indicators))
}

fn token_age_seconds(
    launch_time: Option<chrono::DateTime<chrono::Utc>>,
    cutoff_time: chrono::DateTime<chrono::Utc>,
) -> Option<f64> {
    launch_time
        .filter(|launch_time| *launch_time <= cutoff_time)
        .map(|launch_time| (cutoff_time - launch_time).num_milliseconds().max(0) as f64 / 1_000.0)
}

fn persisted_token_age_seconds(
    creation_time: Option<chrono::DateTime<chrono::Utc>>,
    first_seen_time: Option<chrono::DateTime<chrono::Utc>>,
    event_cutoff: Option<chrono::DateTime<chrono::Utc>>,
    knowledge_cutoff: chrono::DateTime<chrono::Utc>,
) -> Option<f64> {
    match creation_time {
        Some(creation_time) => {
            let cutoff = event_cutoff
                .filter(|cutoff| *cutoff >= creation_time)
                .unwrap_or(knowledge_cutoff);
            token_age_seconds(Some(creation_time), cutoff)
        }
        None => token_age_seconds(first_seen_time, knowledge_cutoff),
    }
}

fn lifetime_buyers(in_memory: u64, durable: i64) -> u64 {
    in_memory.max(u64::try_from(durable.max(0)).unwrap_or(u64::MAX))
}

async fn intelligence_indicators(
    state: &AppState,
    creator: &str,
    mint: &str,
    as_of: &pumphound_domain::ObservationPoint,
) -> anyhow::Result<Value> {
    let mut indicators = OPTIONAL_GRAPH_FEATURE_NAMES
        .into_iter()
        .map(|name| (name.to_string(), Value::Null))
        .collect::<serde_json::Map<_, _>>();
    for name in [
        "creator_risk",
        "wallet_risk",
        "cluster_risk",
        "creator_sell_acceleration",
        "wallet_deterioration",
        "cluster_deterioration",
    ] {
        indicators.insert(name.into(), Value::Null);
    }
    let row = sqlx::query(
        r#"SELECT
             cp.attributes->'risk_score' AS creator_risk,
             wp.attributes->'risk_score' AS wallet_risk,
             cp.attributes->'cluster_risk' AS cluster_risk,
             cp.attributes->'sell_acceleration' AS creator_sell_acceleration,
             wp.attributes->'deterioration' AS wallet_deterioration,
             cp.attributes->'cluster_deterioration' AS cluster_deterioration,
             to_jsonb(cp.graduation_rate) AS creator_graduation_posterior,
             to_jsonb(cp.collapse_rate) AS creator_collapse_posterior
           FROM pumphound.creator_profiles cp
           LEFT JOIN pumphound.wallet_profiles wp
             ON wp.wallet=cp.creator
            AND wp.last_seen_slot <= $2
            AND wp.updated_at <= $3
           WHERE cp.creator=$1
             AND (cp.last_seen_slot IS NULL OR cp.last_seen_slot <= $2)
             AND cp.updated_at <= $3"#,
    )
    .bind(creator)
    .bind(as_of.slot.0 as i64)
    .bind(as_of.knowledge_cutoff)
    .fetch_optional(&state.stores.postgres)
    .await?;
    if let Some(row) = row {
        for name in [
            "creator_risk",
            "wallet_risk",
            "cluster_risk",
            "creator_sell_acceleration",
            "wallet_deterioration",
            "cluster_deterioration",
            "creator_graduation_posterior",
            "creator_collapse_posterior",
        ] {
            indicators.insert(name.into(), json_number(&row, name));
        }
    }
    // launches has no updated_at column, so holder_concentration_computed_at (stored inside
    // attributes by Stores::refresh_holder_concentration) is what bounds this read to the
    // decision's own knowledge cutoff instead — a replayed historical decision must not see a
    // concentration score computed after the fact.
    let holder_row = sqlx::query(
        r#"SELECT attributes->'holder_top_1_share' AS holder_top_1_share
           FROM pumphound.launches
           WHERE mint=$1
             AND (attributes->>'holder_concentration_computed_at')::timestamptz <= $2"#,
    )
    .bind(mint)
    .bind(as_of.knowledge_cutoff)
    .fetch_optional(&state.stores.postgres)
    .await?;
    if let Some(row) = holder_row {
        indicators.insert(
            "holder_top_1_share".into(),
            json_number(&row, "holder_top_1_share"),
        );
    }
    let cluster = crate::intelligence::funding_cluster_for_mint(
        &state.stores,
        mint,
        creator,
        as_of.slot.0 as i64,
    )
    .await;
    // Only write new named keys. Do not alias cluster counts onto
    // immediate_funding_share / funding_diversity — those have different
    // dictionary meanings and frozen books must keep seeing UNKNOWN until
    // a real funded-by adapter exists.
    for (name, key) in [
        ("same_slot_first10_share", "same_slot_first10_share"),
        ("same_funder_first10_count", "same_funder_first10_count"),
        ("first10_buyer_count", "first10_count"),
    ] {
        indicators.insert(name.into(), cluster.get(key).cloned().unwrap_or(Value::Null));
    }
    let reputation = sqlx::query(
        r#"SELECT sample_count,posterior,coverage
           FROM pumphound.reputation_snapshots
           WHERE subject_type='wallet' AND subject_id=$1
             AND as_of_slot <= $2 AND knowledge_cutoff <= $3
           ORDER BY as_of_slot DESC,knowledge_cutoff DESC LIMIT 1"#,
    )
    .bind(creator)
    .bind(as_of.slot.0 as i64)
    .bind(as_of.knowledge_cutoff)
    .fetch_optional(&state.stores.postgres)
    .await?;
    if let Some(row) = reputation {
        let posterior: Value = row.get("posterior");
        let coverage: Value = row.get("coverage");
        indicators.insert(
            "wallet_skill_posterior_mean".into(),
            posterior.get("mean").cloned().unwrap_or(Value::Null),
        );
        indicators.insert(
            "wallet_skill_effective_sample".into(),
            Value::from(row.get::<i64, _>("sample_count")),
        );
        indicators.insert(
            "history_coverage_fraction".into(),
            coverage
                .get("fraction")
                .or_else(|| coverage.get("history_coverage_fraction"))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    Ok(Value::Object(indicators))
}

fn feature_snapshot_hash(vector: &FeatureVector) -> anyhow::Result<String> {
    let canonical_values = vector
        .values
        .clone()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let canonical = json!({
        "launch_id": vector.launch_id,
        "observation_window_seconds": vector.observation_window_seconds,
        "schema_version": vector.schema_version,
        "as_of": {
            "slot": vector.as_of.slot.0,
            "tx_index": vector.as_of.tx_index,
            "event_index": vector.as_of.event_index,
            "event_time": vector.as_of.event_time,
            "knowledge_cutoff": vector.as_of.knowledge_cutoff,
            "commitment": vector.as_of.commitment
        },
        "values": canonical_values,
        "dependency_ids": vector.dependency_ids
    });
    Ok(pumphound_domain::sha256_hex(&serde_json::to_vec(
        &canonical,
    )?))
}

fn opportunity_input(
    prediction: &AggregatePrediction,
    prediction_error: Option<&str>,
    quote: Option<&PaperQuote>,
    round_trip_cost_fraction: Option<f64>,
    snapshot: &pumphound_domain::MarketSnapshot,
    indicators: &Value,
) -> OpportunityInput {
    let graduation = prediction.probability("graduated_by_6h_v1");
    let momentum = prediction.probability("up_50_before_down_20_15m_v1");
    let collapse = prediction.probability("severe_collapse_1h_v1");
    let exit = prediction.probability("exit_hazard_60s_v1");
    let policy_net_edge = momentum
        .zip(collapse)
        .zip(exit)
        .zip(round_trip_cost_fraction)
        .map(|(((momentum, collapse), exit), cost)| momentum - collapse - exit - cost);
    OpportunityInput {
        champion_present: prediction_error.is_none(),
        calibrated: prediction_error.is_none(),
        familiarity: prediction.minimum_familiarity(),
        data_completeness: Some(snapshot.quality.score),
        quality_sufficient: quality_sufficient(snapshot),
        graduation_probability: graduation,
        momentum_probability: momentum,
        collapse_probability: collapse,
        exit_probability: exit,
        expected_net_edge: policy_net_edge,
        round_trip_cost_fraction,
        price_impact_bps: quote.map(|q| q.price_impact_bps),
        liquidity_quote_atomic: Some(snapshot.effective_quote_atomic.0),
        creator_risk: indicators.get("creator_risk").and_then(Value::as_f64),
        wallet_risk: indicators.get("wallet_risk").and_then(Value::as_f64),
        cluster_risk: indicators.get("cluster_risk").and_then(Value::as_f64),
    }
}

fn exact_round_trip_cost_fraction(
    entry_quote: Option<&PaperQuote>,
    entry_request: &PaperOrderRequest,
    snapshot: &pumphound_domain::MarketSnapshot,
) -> Option<f64> {
    exact_round_trip_estimate(entry_quote, entry_request, snapshot).0
}

fn exact_round_trip_estimate(
    entry_quote: Option<&PaperQuote>,
    entry_request: &PaperOrderRequest,
    snapshot: &pumphound_domain::MarketSnapshot,
) -> (Option<f64>, Option<PaperQuote>) {
    let Some(entry_quote) = entry_quote else {
        return (None, None);
    };
    let gross_input = entry_request.input.atomic.0;
    if gross_input == 0 {
        return (None, None);
    }
    let sell_request = PaperOrderRequest {
        client_request_id: Uuid::nil(),
        portfolio_id: entry_request.portfolio_id,
        launch_id: entry_request.launch_id,
        market: entry_request.market,
        side: TradeSide::Sell,
        input: entry_quote.expected_output.clone(),
        output_mint: entry_request.input.mint,
        max_slippage_bps: entry_request.max_slippage_bps,
        quote_id: Uuid::nil(),
        policy_version: entry_request.policy_version.clone(),
        decision_id: Uuid::nil(),
    };
    let Some(sell_quote) = quote_order(&sell_request, snapshot).ok() else {
        return (None, None);
    };
    let fraction = Some(
        gross_input.saturating_sub(sell_quote.expected_output.atomic.0) as f64 / gross_input as f64,
    );
    (fraction, Some(sell_quote))
}

fn entry_request(
    portfolio_id: Uuid,
    launch_id: Uuid,
    snapshot: &pumphound_domain::MarketSnapshot,
    policy: &AutonomousPolicy,
) -> anyhow::Result<PaperOrderRequest> {
    Ok(PaperOrderRequest {
        client_request_id: Uuid::now_v7(),
        portfolio_id,
        launch_id: Some(launch_id),
        market: snapshot.market,
        side: TradeSide::Buy,
        input: AssetAmount {
            mint: Pubkey::from_base58(WSOL_MINT)?,
            atomic: Atomic(fixed_order_atomic()?),
            decimals: 9,
        },
        output_mint: snapshot.mint,
        max_slippage_bps: policy.maximum_price_impact_bps,
        quote_id: Uuid::now_v7(),
        policy_version: policy.policy_version.clone(),
        decision_id: Uuid::now_v7(),
    })
}

fn fixed_order_atomic() -> anyhow::Result<u128> {
    let value = std::env::var("PUMPHOUND_PAPER_ORDER_SOL_ATOMIC")
        .unwrap_or_else(|_| "50000000".into())
        .parse::<u128>()?;
    if value == 0 {
        anyhow::bail!("paper order size must be positive");
    }
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
async fn persist_abstention(
    state: &AppState,
    launch_id: Uuid,
    run_id: Option<Uuid>,
    vector: &FeatureVector,
    policy_version: &str,
    model_versions: &Value,
    reason: &str,
    exact_inputs: Value,
) -> anyhow::Result<()> {
    let key = format!(
        "entry-abstain:{}:{}:{}:{}",
        launch_id, vector.feature_snapshot_id, policy_version, reason
    );
    paper_ledger::persist_decision(
        &state.stores,
        Uuid::now_v7(),
        &key,
        run_id,
        launch_id,
        None,
        &vector.feature_snapshot_id,
        policy_version,
        None,
        "ABSTAIN",
        "ENTRY",
        model_versions,
        &exact_inputs,
        &[reason.to_string()],
        &json!({"autonomous": true, "order_created": false, "live_money": false}),
    )
    .await?;
    Ok(())
}

fn quality_sufficient(snapshot: &pumphound_domain::MarketSnapshot) -> bool {
    let snapshot_age_ms = (now_utc() - snapshot.as_of.knowledge_cutoff).num_milliseconds();
    let fresh = (0..=crate::readiness::STALE_MARKET_MS).contains(&snapshot_age_ms);
    fresh
        && matches!(
            snapshot.quality.grade,
            pumphound_domain::QualityGrade::A | pumphound_domain::QualityGrade::B
        )
        && !matches!(
            snapshot.quality.coverage,
            pumphound_domain::Coverage::Unknown
        )
}

fn indicator(vector: &FeatureVector, name: &str) -> Option<f64> {
    vector.values.get(name).and_then(Value::as_f64)
}

fn json_number(row: &sqlx::postgres::PgRow, name: &str) -> Value {
    row.try_get::<Option<Value>, _>(name)
        .ok()
        .flatten()
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        })
        .map(Value::from)
        .unwrap_or(Value::Null)
}

fn exit_action_name(action: ExitAction) -> &'static str {
    match action {
        ExitAction::Hold => "HOLD",
        ExitAction::PartialExit => "PARTIAL_EXIT",
        ExitAction::FullExit => "FULL_EXIT",
        ExitAction::EmergencyExit => "EMERGENCY_EXIT",
    }
}

fn parse_uuid(value: &Value, key: &str) -> anyhow::Result<Uuid> {
    Ok(Uuid::parse_str(value_str(value, key)?)?)
}

fn value_str<'a>(value: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing {key}"))
}

async fn refresh_projection(state: &AppState) {
    match paper_ledger::hydrate(&state.stores).await {
        Ok(paper) => *state.paper.write().await = paper,
        Err(error) => warn!(%error, "paper projection refresh failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn live_watch_cannot_run_autonomy() {
        assert!(!autonomous_mode("LIVE_WATCH"));
        assert!(autonomous_mode("LIVE_PAPER_HEURISTIC"));
        assert!(autonomous_mode("LIVE_PAPER_SHADOW_ML"));
        assert!(autonomous_mode("LIVE_PAPER_ML"));
        assert!(autonomous_mode("REPLAY"));
    }

    fn shadow_prediction(momentum: f64, adverse: f64) -> AggregatePrediction {
        AggregatePrediction {
            status: "SHADOW".into(),
            authority: Some("SHADOW".into()),
            prediction_id: "p".into(),
            feature_snapshot_id: "s".into(),
            launch_id: "l".into(),
            observation_window_seconds: 120,
            probabilities: BTreeMap::from([
                (SHORT_MOMENTUM_TARGET.into(), Some(momentum)),
                (ADVERSE_MOVE_TARGET.into(), Some(adverse)),
            ]),
            target_model_versions: BTreeMap::new(),
            familiarity: BTreeMap::new(),
            withheld_reasons: BTreeMap::new(),
            explanations: BTreeMap::new(),
        }
    }

    #[test]
    fn gate9_confidence_sizing_is_disabled_by_default_and_records_why() {
        // Regression for GATE 9: until the inversion is resolved with evidence, a strong shadow
        // signal must not change the bet size, and the decision must say so.
        std::env::remove_var("PUMPHOUND_CONFIDENCE_SIZING");
        let prediction = shadow_prediction(0.95, 0.05);
        let (atomic, evidence) =
            confidence_scaled_atomic(50_000_000, "FULL_ML_SHADOW", Some(&prediction));
        assert_eq!(atomic, 50_000_000, "confidence must not influence size while GATE 9 is open");
        let evidence = evidence.expect("evidence retained for audit");
        assert_eq!(evidence["applied"], json!(false));
        assert!(evidence["disabled_reason"].as_str().unwrap().starts_with("GATE9"));
        assert_ne!(evidence["would_have_scaled_atomic"], json!("50000000"));
    }

    #[test]
    fn no_shadow_prediction_keeps_the_base_size() {
        let (atomic, evidence) = confidence_scaled_atomic(50_000_000, "FULL_ML_SHADOW", None);
        assert_eq!(atomic, 50_000_000);
        assert!(evidence.is_none());
    }

    #[test]
    fn control_flow_book_is_never_scaled_even_with_a_strong_signal() {
        let prediction = shadow_prediction(0.9, 0.0);
        let (atomic, evidence) =
            confidence_scaled_atomic(50_000_000, STRATEGY_CONTROL_FLOW_V1, Some(&prediction));
        assert_eq!(atomic, 50_000_000, "control baseline must stay flat-bet");
        assert!(evidence.is_none());
    }

    #[test]
    fn breakeven_edge_sizes_at_the_floor_multiplier() {
        // momentum == adverse -> edge == 0 -> minimum multiplier, not the base size unscaled.
        let prediction = shadow_prediction(0.5, 0.5);
        let (atomic, evidence) =
            confidence_band_atomic(50_000_000, "FULL_ML_SHADOW", Some(&prediction));
        assert_eq!(
            atomic,
            (50_000_000.0 * SIZE_SCALING_MIN_MULTIPLIER).round() as u128
        );
        assert!(evidence.is_some());
    }

    #[test]
    fn strong_edge_sizes_at_the_ceiling_multiplier_not_beyond() {
        // edge = 0.9, far past SIZE_SCALING_EDGE_AT_MAX -> clamps at the max multiplier.
        let prediction = shadow_prediction(0.95, 0.05);
        let (atomic, _) = confidence_band_atomic(50_000_000, "FULL_ML_SHADOW", Some(&prediction));
        assert_eq!(
            atomic,
            (50_000_000.0 * SIZE_SCALING_MAX_MULTIPLIER).round() as u128
        );
    }

    #[test]
    fn mid_edge_sizes_strictly_between_floor_and_ceiling() {
        let low = confidence_band_atomic(
            50_000_000,
            "FULL_ML_SHADOW",
            Some(&shadow_prediction(0.5, 0.5)),
        )
        .0;
        let mid = confidence_band_atomic(
            50_000_000,
            "FULL_ML_SHADOW",
            Some(&shadow_prediction(0.65, 0.5)),
        )
        .0;
        let high = confidence_band_atomic(
            50_000_000,
            "FULL_ML_SHADOW",
            Some(&shadow_prediction(0.8, 0.5)),
        )
        .0;
        assert!(low < mid, "more edge must size up, not down or flat");
        assert!(mid < high, "more edge must keep sizing up monotonically");
    }

    #[test]
    fn missing_target_probability_falls_back_to_base_size() {
        let prediction = AggregatePrediction {
            status: "SHADOW".into(),
            authority: Some("SHADOW".into()),
            prediction_id: "p".into(),
            feature_snapshot_id: "s".into(),
            launch_id: "l".into(),
            observation_window_seconds: 120,
            probabilities: BTreeMap::from([(SHORT_MOMENTUM_TARGET.into(), Some(0.9))]), // adverse missing
            target_model_versions: BTreeMap::new(),
            familiarity: BTreeMap::new(),
            withheld_reasons: BTreeMap::new(),
            explanations: BTreeMap::new(),
        };
        let (atomic, evidence) =
            confidence_scaled_atomic(50_000_000, "FULL_ML_SHADOW", Some(&prediction));
        assert_eq!(atomic, 50_000_000, "can't scale on a half-known edge");
        assert!(evidence.is_none());
    }

    #[test]
    fn shadow_ml_never_changes_heuristic_action() {
        let heuristic_action = OpportunityAction::Buy;
        let shadow_payload = json!({
            "actual_action_unchanged": paper_ledger::action_name(
                &pumphound_paper_core::OpportunityDecision {
                    action: heuristic_action,
                    reasons: vec![]
                }
            ),
            "execution_effect": "NONE"
        });
        assert_eq!(heuristic_action, OpportunityAction::Buy);
        assert_eq!(shadow_payload["execution_effect"], "NONE");
    }

    #[test]
    fn ml_mode_remains_champion_controlled() {
        assert_eq!(
            mode_strategy_status(OperatingMode::LivePaperMl),
            ("ML_POLICY", "CONTROL")
        );
        assert!(!missing_required_targets(&ChampionSet::new()).is_empty());
    }

    #[test]
    fn decision_window_key_is_restart_stable() {
        let launch_id = Uuid::nil();
        let window = decision_age_bucket(Some(42.0)).unwrap();
        assert_eq!(
            entry_decision_key(
                "ALPHA-LOOP-20260908",
                launch_id,
                Some(window),
                "HEURISTIC_V1"
            ),
            "exp-entry:ALPHA-LOOP-20260908:00000000-0000-0000-0000-000000000000:60:HEURISTIC_V1"
        );
        assert_eq!(
            entry_decision_key(
                "ALPHA-LOOP-20260908",
                launch_id,
                Some(window),
                "HEURISTIC_GRAPH_AI_V1"
            ),
            "exp-entry:ALPHA-LOOP-20260908:00000000-0000-0000-0000-000000000000:60:HEURISTIC_GRAPH_AI_V1"
        );
    }

    #[test]
    fn high_shadow_hazard_does_not_change_heuristic_buy() {
        let high_hazard = AggregatePrediction {
            status: "SHADOW_OOD".into(),
            authority: Some("SHADOW".into()),
            prediction_id: "p".into(),
            feature_snapshot_id: "s".into(),
            launch_id: "l".into(),
            observation_window_seconds: 60,
            probabilities: BTreeMap::from([(EXIT_HAZARD_TARGET.into(), Some(0.9))]),
            target_model_versions: BTreeMap::new(),
            familiarity: BTreeMap::new(),
            withheld_reasons: BTreeMap::new(),
            explanations: BTreeMap::new(),
        };
        let gate = shadow_gate_from_prediction(&high_hazard);
        assert_eq!(gate.exit_hazard, Some(0.9));
        assert_eq!(
            pumphound_paper_core::evaluate_shadow_ml_gate(&gate).action,
            OpportunityAction::Avoid
        );
    }

    #[test]
    fn parallel_books_match_operating_mode() {
        assert_eq!(parallel_books(false).len(), 4);
        assert_eq!(parallel_books(true).len(), 6);
    }

    #[test]
    fn frequent_trades_do_not_reset_token_age() {
        let launched_at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let first_trade = launched_at + Duration::seconds(12);
        let latest_trade = launched_at + Duration::seconds(78);
        assert_eq!(
            token_age_seconds(Some(launched_at), first_trade),
            Some(12.0)
        );
        assert_eq!(
            token_age_seconds(Some(launched_at), latest_trade),
            Some(78.0)
        );
        assert_eq!(token_age_seconds(None, latest_trade), None);

        let first_seen = latest_trade + Duration::seconds(5);
        let knowledge_cutoff = first_seen + Duration::seconds(20);
        assert_eq!(
            persisted_token_age_seconds(
                None,
                Some(first_seen),
                Some(latest_trade),
                knowledge_cutoff
            ),
            Some(20.0)
        );
    }

    #[test]
    fn restart_retains_durable_lifetime_buyers() {
        assert_eq!(lifetime_buyers(0, 37), 37);
        assert_eq!(lifetime_buyers(41, 37), 41);
    }

    #[test]
    fn age_windows_are_fixed_maturation_buckets() {
        assert_eq!(
            [0.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0]
                .map(|age| decision_age_bucket(Some(age)).unwrap()),
            [5, 10, 15, 30, 60, 120, 300, 900]
        );
        assert_eq!(decision_age_bucket(Some(901.0)), Some(900));
        assert_eq!(decision_age_bucket(None), None);
    }

    #[test]
    fn exact_heuristic_inputs_are_complete() {
        let exact_inputs = json!({
            "heuristic_input": {},
            "market_state": {
                "market_snapshot_id": "snapshot",
                "knowledge_cutoff": "2026-09-07T00:00:00Z",
                "estimated_liquidity_quote_atomic": 1_000
            },
            "quote_state": {
                "entry_impact_bps": 10,
                "estimated_exit_impact_bps": 11,
                "entry_fee_components": [],
                "estimated_exit_fee_components": [],
                "round_trip_cost_fraction": 0.01
            },
            "scout_selection": {"hash": "hash", "rate": 0.1},
            "operating_mode": "LIVE_PAPER_HEURISTIC",
            "strategy_version": "HEURISTIC_V1",
            "policy_content_sha256": "policy-hash"
        });
        ensure_complete_heuristic_evidence(&exact_inputs).unwrap();
        let mut incomplete = exact_inputs;
        incomplete["market_state"]
            .as_object_mut()
            .unwrap()
            .remove("knowledge_cutoff");
        assert!(ensure_complete_heuristic_evidence(&incomplete).is_err());
    }

    #[test]
    fn snapshot_hash_changes_with_values_and_cutoff() {
        let cutoff = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let mut vector = FeatureVector {
            feature_snapshot_id: String::new(),
            launch_id: "launch".into(),
            observation_window_seconds: 60,
            as_of: pumphound_domain::ObservationPoint {
                slot: pumphound_domain::Slot(7),
                tx_index: None,
                event_index: None,
                event_time: Some(cutoff),
                observed_at: cutoff,
                knowledge_cutoff: cutoff,
                commitment: pumphound_domain::Commitment::Confirmed,
            },
            schema_version: "features-v1".into(),
            values: serde_json::Map::from_iter([("buy_count_5".into(), json!(1.0))]),
            dependency_ids: vec![],
            quality: pumphound_domain::DataQuality::live(0, 0, true),
        };
        let original = feature_snapshot_hash(&vector).unwrap();
        vector.values.insert("buy_count_5".into(), json!(2.0));
        let changed_value = feature_snapshot_hash(&vector).unwrap();
        vector.values.insert("buy_count_5".into(), json!(1.0));
        vector.as_of.knowledge_cutoff += Duration::milliseconds(1);
        let changed_cutoff = feature_snapshot_hash(&vector).unwrap();
        assert_ne!(original, changed_value);
        assert_ne!(original, changed_cutoff);
    }

    #[test]
    fn stale_event_cutoff_uses_chain_creation_and_knowledge_time() {
        let creation = Utc.timestamp_opt(1_700_000_100, 0).unwrap();
        let stale_event = creation - Duration::seconds(20);
        let knowledge = creation + Duration::seconds(45);
        assert_eq!(
            persisted_token_age_seconds(
                Some(creation),
                Some(creation + Duration::seconds(1)),
                Some(stale_event),
                knowledge
            ),
            Some(45.0)
        );
    }

    #[test]
    fn frozen_policy_files_match_captured_hashes() {
        assert_eq!(
            pumphound_domain::sha256_hex(include_bytes!("../../../configs/control-flow-v1.json")),
            "f9c8cfe5c623c58678fb5eaab537dcf0d2d34f1683953b71aa89a7afb8dcd8ac"
        );
        assert_eq!(
            pumphound_domain::sha256_hex(include_bytes!(
                "../../../configs/research-exploration-v1.json"
            )),
            "68712126baea5736dc281b5169dfb37cf6c202b8385c82717703c1dfb23f265b"
        );
        assert_eq!(
            pumphound_domain::sha256_hex(include_bytes!("../../../configs/heuristic-v1.json")),
            "aae6ba27467af0932036468121af6bb11e9de680c10612b2db84a96be3c2a944"
        );
    }

    #[test]
    fn learning_decision_keys_cannot_collide_with_frozen_experiment() {
        let launch_id = Uuid::nil();
        let frozen = entry_decision_key(DEFAULT_EXPERIMENT_ID, launch_id, Some(30), "HEURISTIC_V1");
        let learning = format!(
            "learn-entry:{}:{launch_id}:30:{}",
            ADAPTIVE_EXPERIMENT_ID, STRATEGY_RANDOM_CONTROL_V1
        );
        assert_ne!(frozen, learning);
        assert!(learning.contains(ADAPTIVE_EXPERIMENT_ID));
    }
}
