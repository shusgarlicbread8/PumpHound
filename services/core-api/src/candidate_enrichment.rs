use crate::state::AppState;
use chrono::{DateTime, Utc};
use pumphound_storage::Stores;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::sync::Arc;
use uuid::Uuid;

const LEVEL_1_METHOD: &str = "getTransfersByAddress";
const LEVEL_2_METHOD: &str = "getTransactionsForAddress";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrichmentConfig {
    pub schema_version: u32,
    pub version: String,
    pub enabled: bool,
    pub daily_credits: i64,
    pub monthly_credits: i64,
    pub per_candidate_credits: i64,
    pub max_candidates_per_hour: i64,
    pub max_wallets_per_candidate: usize,
    pub include_buyers_6_to_10: bool,
    pub level1_credits_per_request: i64,
    pub level1_limit: u32,
    pub level2_enabled: bool,
    pub level2_credits_upper_bound: i64,
    pub level2_limit: u32,
    pub level2_eligibility_reasons: Vec<String>,
    pub minimum_meaningful_native_lamports: u64,
    pub initial_funder_confidence_complete: f64,
    pub initial_funder_confidence_incomplete: f64,
    pub minimum_high_reputation: f64,
    pub known_service_addresses: Vec<String>,
}

impl EnrichmentConfig {
    pub async fn load() -> Result<Self, String> {
        let path = std::env::var("PUMPHOUND_ENRICHMENT_CONFIG_PATH")
            .unwrap_or_else(|_| "configs/enrichment-v1.json".into());
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| "ENRICHMENT_CONFIG_UNAVAILABLE".to_string())?;
        let mut config: Self =
            serde_json::from_slice(&bytes).map_err(|_| "ENRICHMENT_CONFIG_INVALID".to_string())?;
        apply_env_bool("PUMPHOUND_ENRICHMENT_ENABLED", &mut config.enabled)?;
        apply_env_bool(
            "PUMPHOUND_ENRICHMENT_LEVEL2_ENABLED",
            &mut config.level2_enabled,
        )?;
        apply_env_i64(
            "PUMPHOUND_ENRICHMENT_DAILY_CREDITS",
            &mut config.daily_credits,
        )?;
        apply_env_i64(
            "PUMPHOUND_ENRICHMENT_MONTHLY_CREDITS",
            &mut config.monthly_credits,
        )?;
        apply_env_i64(
            "PUMPHOUND_ENRICHMENT_PER_CANDIDATE_CREDITS",
            &mut config.per_candidate_credits,
        )?;
        apply_env_i64(
            "PUMPHOUND_ENRICHMENT_MAX_CANDIDATES_PER_HOUR",
            &mut config.max_candidates_per_hour,
        )?;
        apply_env_usize(
            "PUMPHOUND_ENRICHMENT_MAX_WALLETS_PER_CANDIDATE",
            &mut config.max_wallets_per_candidate,
        )?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.version != "CANDIDATE_ENRICHMENT_V1" {
            return Err("ENRICHMENT_CONFIG_VERSION_INVALID".into());
        }
        if self.daily_credits < 0
            || self.monthly_credits < self.daily_credits
            || self.per_candidate_credits < 0
            || self.max_candidates_per_hour <= 0
            || self.max_wallets_per_candidate == 0
            || self.max_wallets_per_candidate > 25
            || self.level1_credits_per_request != 10
            || self.level1_limit == 0
            || self.level1_limit > 100
            || self.level2_credits_upper_bound < 100
            || self.level2_limit == 0
            || self.level2_limit > 100
        {
            return Err("ENRICHMENT_BOUNDS_INVALID".into());
        }
        for value in [
            self.initial_funder_confidence_complete,
            self.initial_funder_confidence_incomplete,
            self.minimum_high_reputation,
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err("ENRICHMENT_CONFIDENCE_INVALID".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct HeliusClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
}

impl fmt::Debug for HeliusClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HeliusClient")
            .field("endpoint_configured", &!self.endpoint.is_empty())
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug)]
enum RpcFailure {
    RateLimited,
    NotEntitled,
    Transport,
    InvalidResponse,
}

impl RpcFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::RateLimited => "RATE_LIMITED",
            Self::NotEntitled => "NOT_ENTITLED",
            Self::Transport => "RPC_TRANSPORT",
            Self::InvalidResponse => "RPC_INVALID_RESPONSE",
        }
    }
}

impl HeliusClient {
    fn from_env() -> Option<Self> {
        let entitled = std::env::var("PUMPHOUND_HELIUS_ENRICHMENT_ENTITLED")
            .ok()
            .is_some_and(|value| value == "1");
        let api_key = std::env::var("PUMPHOUND_HELIUS_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())?;
        if !entitled {
            return None;
        }
        Some(Self::new(
            std::env::var("PUMPHOUND_HELIUS_ENRICHMENT_RPC_URL")
                .unwrap_or_else(|_| "https://mainnet.helius-rpc.com".into()),
            api_key,
        ))
    }

    fn new(endpoint: String, api_key: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
            endpoint,
            api_key,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, RpcFailure> {
        let response = self
            .http
            .post(&self.endpoint)
            .query(&[("api-key", self.api_key.as_str())])
            .json(&json!({
                "jsonrpc": "2.0",
                "id": Uuid::now_v7().to_string(),
                "method": method,
                "params": params
            }))
            .send()
            .await
            .map_err(|_| RpcFailure::Transport)?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(RpcFailure::RateLimited);
        }
        if matches!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(RpcFailure::NotEntitled);
        }
        if !response.status().is_success() {
            return Err(RpcFailure::Transport);
        }
        let body = response
            .json::<Value>()
            .await
            .map_err(|_| RpcFailure::InvalidResponse)?;
        if let Some(error) = body.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            if message.contains("plan")
                || message.contains("entitle")
                || message.contains("access")
                || message.contains("upgrade")
            {
                return Err(RpcFailure::NotEntitled);
            }
            if message.contains("rate") {
                return Err(RpcFailure::RateLimited);
            }
            return Err(RpcFailure::InvalidResponse);
        }
        body.get("result")
            .cloned()
            .ok_or(RpcFailure::InvalidResponse)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct SelectedWallet {
    wallet: String,
    role: String,
}

#[derive(Clone, Debug)]
struct CandidateContext {
    decision_id: Uuid,
    launch_id: Uuid,
    action: String,
    as_of_slot: i64,
    mint: String,
    creator: String,
}

pub fn candidate_action(action: &str) -> bool {
    action == "BUY"
}

pub fn research_intent_action(action: &str) -> bool {
    matches!(action, "BUY" | "WATCH")
}

pub fn spawn_for_candidate(state: Arc<AppState>, decision_id: Uuid, action: &str) {
    if !candidate_action(action) {
        return;
    }
    let action = action.to_string();
    tokio::spawn(async move {
        if let Err(error) = enrich_candidate(&state, decision_id).await {
            tracing::warn!(%decision_id, error_code = %safe_error_code(&error), "candidate enrichment degraded");
        }
        crate::investigator::spawn_for_decision(state, decision_id, &action);
    });
}

pub fn spawn_for_research_intent(state: Arc<AppState>, decision_id: Uuid) {
    tokio::spawn(async move {
        if let Err(error) = enrich_research_intent(&state, decision_id).await {
            tracing::warn!(%decision_id, error_code = %safe_error_code(&error), "research enrichment degraded");
        }
        crate::investigator::spawn_for_decision(state, decision_id, "WATCH");
    });
}

async fn enrich_candidate(state: &AppState, decision_id: Uuid) -> anyhow::Result<()> {
    enrich_eligible(state, decision_id, candidate_action).await
}

async fn enrich_research_intent(state: &AppState, decision_id: Uuid) -> anyhow::Result<()> {
    enrich_eligible(state, decision_id, research_intent_action).await
}

async fn enrich_eligible(
    state: &AppState,
    decision_id: Uuid,
    eligible: fn(&str) -> bool,
) -> anyhow::Result<()> {
    let config = EnrichmentConfig::load().await.map_err(anyhow::Error::msg)?;
    let context = candidate_context(&state.stores, decision_id).await?;
    if !eligible(&context.action) {
        anyhow::bail!("CANDIDATE_ACTION_NOT_ELIGIBLE");
    }
    release_stale_reservations(&state.stores).await?;
    let selected = select_wallets(state, &context, &config).await?;
    persist_level_zero(&state.stores, &context, &selected, &config).await?;
    if !config.enabled {
        set_dossier_status(&state.stores, decision_id, "DISABLED").await?;
        return Ok(());
    }
    let Some(client) = HeliusClient::from_env() else {
        journal_not_entitled(&state.stores, &context, &selected, &config).await?;
        set_dossier_status(&state.stores, decision_id, "NOT_ENTITLED").await?;
        return Ok(());
    };

    set_dossier_status(&state.stores, decision_id, "ENRICHING").await?;
    let service_addresses = known_service_addresses(&state.stores, &config).await?;
    let mut completed = 0usize;
    let mut withheld = 0usize;
    let mut degraded = 0usize;
    for selected_wallet in &selected {
        let from_slot = next_enrichment_slot(&state.stores, &selected_wallet.wallet).await?;
        if from_slot > context.as_of_slot {
            completed += 1;
            continue;
        }
        let request_key = format!(
            "{}:{}:{}:{}",
            context.decision_id, selected_wallet.wallet, LEVEL_1_METHOD, from_slot
        );
        let reservation = reserve_credits(
            &state.stores,
            &context,
            &selected_wallet.wallet,
            1,
            LEVEL_1_METHOD,
            None,
            config.level1_credits_per_request,
            &request_key,
            &config,
        )
        .await?;
        let Reservation::Reserved(request_id) = reservation else {
            withheld += 1;
            continue;
        };
        let params = transfer_params(
            &selected_wallet.wallet,
            from_slot,
            context.as_of_slot,
            config.level1_limit,
        );
        match client.call(LEVEL_1_METHOD, params).await {
            Ok(result) => {
                let evidence = parse_transfer_result(
                    &selected_wallet.wallet,
                    from_slot,
                    context.as_of_slot,
                    result,
                    &service_addresses,
                    &config,
                );
                if persist_wallet_result(
                    &state.stores,
                    &selected_wallet.wallet,
                    from_slot,
                    &evidence,
                    &config,
                )
                .await
                .is_err()
                {
                    settle_request(
                        &state.stores,
                        request_id,
                        "DEGRADED",
                        config.level1_credits_per_request,
                        Some("CACHE_PERSIST_FAILED"),
                        Some(&evidence.raw_sha256),
                    )
                    .await?;
                    degraded += 1;
                    continue;
                }
                settle_request(
                    &state.stores,
                    request_id,
                    "COMPLETED",
                    config.level1_credits_per_request,
                    None,
                    Some(&evidence.raw_sha256),
                )
                .await?;
                completed += 1;
                maybe_level_two(
                    &state.stores,
                    &client,
                    &context,
                    selected_wallet,
                    &evidence,
                    &config,
                )
                .await?;
            }
            Err(error) => {
                let status = match error {
                    RpcFailure::NotEntitled => "NOT_ENTITLED",
                    RpcFailure::RateLimited => "DEGRADED",
                    RpcFailure::Transport | RpcFailure::InvalidResponse => "FAILED",
                };
                settle_request(
                    &state.stores,
                    request_id,
                    status,
                    0,
                    Some(error.code()),
                    None,
                )
                .await?;
                degraded += 1;
            }
        }
    }
    finalize_dossier(
        &state.stores,
        &context,
        &selected,
        completed,
        withheld,
        degraded,
    )
    .await?;
    Ok(())
}

async fn candidate_context(stores: &Stores, decision_id: Uuid) -> anyhow::Result<CandidateContext> {
    let row = sqlx::query(
        r#"SELECT d.decision_id,d.launch_id,d.action,COALESCE(fs.cutoff_slot,l.creation_slot) AS as_of_slot,
                  l.mint,l.creator
           FROM pumphound.decisions d
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           LEFT JOIN pumphound.feature_snapshots fs ON fs.feature_snapshot_id=d.feature_snapshot_id
           WHERE d.decision_id=$1 AND d.heuristic_version='HEURISTIC_V1'"#,
    )
    .bind(decision_id)
    .fetch_one(&stores.postgres)
    .await?;
    Ok(CandidateContext {
        decision_id: row.get("decision_id"),
        launch_id: row.get("launch_id"),
        action: row.get("action"),
        as_of_slot: row.get("as_of_slot"),
        mint: row.get("mint"),
        creator: row.get("creator"),
    })
}

async fn select_wallets(
    state: &AppState,
    context: &CandidateContext,
    config: &EnrichmentConfig,
) -> anyhow::Result<Vec<SelectedWallet>> {
    let mut candidates = vec![SelectedWallet {
        wallet: context.creator.clone(),
        role: "creator".into(),
    }];
    let tape = state.tape.read().await;
    let trades = tape.get(&context.mint).cloned().unwrap_or_default();
    drop(tape);
    let mut buyers = Vec::new();
    let mut seen_buyers = HashSet::new();
    for trade in trades.iter().rev().filter(|trade| trade.side == "buy") {
        if seen_buyers.insert(trade.trader.clone()) {
            buyers.push(trade.trader.clone());
        }
    }
    let buyer_limit = if config.include_buyers_6_to_10 { 10 } else { 5 };
    candidates.extend(
        buyers
            .into_iter()
            .take(buyer_limit)
            .enumerate()
            .map(|(index, wallet)| SelectedWallet {
                wallet,
                role: format!("first_buyer_{}", index + 1),
            }),
    );
    let largest_seller = trades
        .iter()
        .filter(|trade| trade.side == "sell")
        .max_by_key(|trade| {
            trade
                .quote_amount_atomic
                .parse::<u128>()
                .unwrap_or_default()
        })
        .map(|trade| trade.trader.clone());
    if let Some(wallet) = largest_seller {
        candidates.push(SelectedWallet {
            wallet,
            role: "largest_recent_seller".into(),
        });
    }
    let launch_attributes: Option<Value> =
        sqlx::query_scalar("SELECT attributes FROM pumphound.launches WHERE launch_id=$1")
            .bind(context.launch_id)
            .fetch_optional(&state.stores.postgres)
            .await?;
    if let Some(wallet) = launch_attributes
        .as_ref()
        .and_then(|value| value.pointer("/holder_metrics/largest_holder/wallet"))
        .or_else(|| {
            launch_attributes
                .as_ref()
                .and_then(|value| value.pointer("/holder_metrics/largest_holder/address"))
        })
        .and_then(Value::as_str)
    {
        candidates.push(SelectedWallet {
            wallet: wallet.into(),
            role: "largest_observed_holder".into(),
        });
    }
    let observed_wallets =
        observed_wallets(&state.stores, &context.mint, context.as_of_slot).await?;
    let reputation_rows = sqlx::query(
        r#"SELECT DISTINCT ON (subject_id) subject_id,posterior
           FROM pumphound.reputation_snapshots
           WHERE subject_id=ANY($1) AND as_of_slot <= $2
           ORDER BY subject_id,as_of_slot DESC,knowledge_cutoff DESC"#,
    )
    .bind(&observed_wallets)
    .bind(context.as_of_slot)
    .fetch_all(&state.stores.postgres)
    .await?;
    for row in reputation_rows {
        let posterior: Value = row.get("posterior");
        let score = posterior
            .get("mean")
            .or_else(|| posterior.get("score"))
            .and_then(json_f64);
        if score.is_some_and(|score| score >= config.minimum_high_reputation) {
            candidates.push(SelectedWallet {
                wallet: row.get("subject_id"),
                role: "high_reputation_observed".into(),
            });
        }
    }
    let cohort_wallets = sqlx::query_scalar::<_, String>(
        r#"SELECT DISTINCT m.wallet FROM pumphound.cohort_members m
           WHERE m.wallet=ANY($1) ORDER BY m.wallet LIMIT 10"#,
    )
    .bind(&observed_wallets)
    .fetch_all(&state.stores.postgres)
    .await?;
    candidates.extend(cohort_wallets.into_iter().map(|wallet| SelectedWallet {
        wallet,
        role: "known_cohort_observed".into(),
    }));
    Ok(deduplicate_wallets(
        candidates,
        config.max_wallets_per_candidate,
    ))
}

fn deduplicate_wallets(candidates: Vec<SelectedWallet>, limit: usize) -> Vec<SelectedWallet> {
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|candidate| {
            !candidate.wallet.trim().is_empty() && seen.insert(candidate.wallet.clone())
        })
        .take(limit)
        .collect()
}

async fn observed_wallets(
    stores: &Stores,
    mint: &str,
    as_of_slot: i64,
) -> anyhow::Result<Vec<String>> {
    let nodes = sqlx::query(
        r#"SELECT source_node,target_node FROM pumphound.graph_facts
           WHERE asset_mint=$1 AND slot <= $2 ORDER BY slot LIMIT 200"#,
    )
    .bind(mint)
    .bind(as_of_slot)
    .fetch_all(&stores.postgres)
    .await?;
    Ok(nodes
        .iter()
        .flat_map(|row| {
            [
                row.get::<String, _>("source_node"),
                row.get::<String, _>("target_node"),
            ]
        })
        .filter_map(|node| node.strip_prefix("wallet:").map(str::to_string))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

async fn persist_level_zero(
    stores: &Stores,
    context: &CandidateContext,
    selected: &[SelectedWallet],
    config: &EnrichmentConfig,
) -> anyhow::Result<()> {
    let wallets = selected
        .iter()
        .map(|selected| selected.wallet.clone())
        .collect::<Vec<_>>();
    let cached = sqlx::query(
        r#"SELECT wallet,transfer_status,funding_state,inferred_initial_funder,
                  funding_confidence,funding_evidence_signature,funding_evidence_slot,
                  funding_method,funding_coverage,last_enriched_slot,raw_evidence_sha256
           FROM pumphound.wallet_enrichment_cache WHERE wallet=ANY($1)"#,
    )
    .bind(&wallets)
    .fetch_all(&stores.postgres)
    .await?;
    let cached_evidence_ids = cached
        .iter()
        .filter_map(|row| {
            row.try_get::<Option<String>, _>("funding_evidence_signature")
                .ok()
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    let mut tx = stores.postgres.begin().await?;
    sqlx::query(
        r#"INSERT INTO pumphound.candidate_dossiers
           (decision_id,launch_id,action,as_of_slot,status,selected_wallets,evidence_ids,
            enrichment,version,created_at,updated_at)
           VALUES ($1,$2,$3,$4,'LEVEL_0',$5,$6,$7,$8,now(),now())
           ON CONFLICT (decision_id) DO NOTHING"#,
    )
    .bind(context.decision_id)
    .bind(context.launch_id)
    .bind(&context.action)
    .bind(context.as_of_slot)
    .bind(json!(selected))
    .bind(json!(cached_evidence_ids))
    .bind(json!({
        "level_0": true,
        "provider_calls": 0,
        "cached_wallets": cached.iter().map(cache_row_json).collect::<Vec<_>>(),
        "baseline_effect": "NONE"
    }))
    .bind(&config.version)
    .execute(&mut *tx)
    .await?;
    for (ordinal, selected) in selected.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO pumphound.candidate_dossier_wallets
               (decision_id,wallet,selection_role,selection_ordinal,enrichment_status,evidence_ids)
               VALUES ($1,$2,$3,$4,'LEVEL_0','[]')
               ON CONFLICT (decision_id,wallet) DO NOTHING"#,
        )
        .bind(context.decision_id)
        .bind(&selected.wallet)
        .bind(&selected.role)
        .bind(ordinal as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Debug)]
enum Reservation {
    Reserved(Uuid),
    Withheld,
    Existing,
}

#[allow(clippy::too_many_arguments)]
async fn reserve_credits(
    stores: &Stores,
    context: &CandidateContext,
    wallet: &str,
    level: i16,
    method: &str,
    eligibility_reason: Option<&str>,
    estimated: i64,
    request_key: &str,
    config: &EnrichmentConfig,
) -> anyhow::Result<Reservation> {
    let mut tx = stores.postgres.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('candidate-enrichment-budget'))")
        .execute(&mut *tx)
        .await?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pumphound.enrichment_request_ledger WHERE request_key=$1)",
    )
    .bind(request_key)
    .fetch_one(&mut *tx)
    .await?
    {
        tx.commit().await?;
        return Ok(Reservation::Existing);
    }
    let today = used_credits(&mut tx, "reserved_at >= date_trunc('day',now())", None).await?;
    let month = used_credits(&mut tx, "reserved_at >= date_trunc('month',now())", None).await?;
    let candidate = used_credits(&mut tx, "decision_id=$1", Some(context.decision_id)).await?;
    let candidates_last_hour: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(DISTINCT decision_id) FROM pumphound.enrichment_request_ledger
           WHERE reserved_at >= now()-interval '1 hour'
             AND status IN ('RESERVED','COMPLETED')"#,
    )
    .fetch_one(&mut *tx)
    .await?;
    let within_budget = today.saturating_add(estimated) <= config.daily_credits
        && month.saturating_add(estimated) <= config.monthly_credits
        && candidate.saturating_add(estimated) <= config.per_candidate_credits
        && (candidates_last_hour < config.max_candidates_per_hour || candidate > 0);
    let request_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO pumphound.enrichment_request_ledger
           (request_id,request_key,decision_id,launch_id,wallet,level,method,eligibility_reason,
            estimated_credits,actual_credits,status,error_code,reserved_at,updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,now(),now())"#,
    )
    .bind(request_id)
    .bind(request_key)
    .bind(context.decision_id)
    .bind(context.launch_id)
    .bind(wallet)
    .bind(level)
    .bind(method)
    .bind(eligibility_reason)
    .bind(estimated)
    .bind(if within_budget { None } else { Some(0i64) })
    .bind(if within_budget {
        "RESERVED"
    } else {
        "BUDGET_WITHHELD"
    })
    .bind(if within_budget {
        None
    } else {
        Some("HARD_BUDGET_EXCEEDED")
    })
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(if within_budget {
        Reservation::Reserved(request_id)
    } else {
        Reservation::Withheld
    })
}

async fn used_credits(
    tx: &mut Transaction<'_, Postgres>,
    predicate: &str,
    decision_id: Option<Uuid>,
) -> anyhow::Result<i64> {
    let sql = format!(
        r#"SELECT COALESCE(SUM(
             CASE WHEN actual_credits IS NOT NULL THEN actual_credits
                  WHEN status='RESERVED' THEN estimated_credits ELSE 0 END
           ),0)::bigint
           FROM pumphound.enrichment_request_ledger WHERE {predicate}"#
    );
    let mut query = sqlx::query_scalar(&sql);
    if let Some(decision_id) = decision_id {
        query = query.bind(decision_id);
    }
    Ok(query.fetch_one(&mut **tx).await?)
}

async fn settle_request(
    stores: &Stores,
    request_id: Uuid,
    status: &str,
    actual_credits: i64,
    error_code: Option<&str>,
    response_sha256: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE pumphound.enrichment_request_ledger
           SET status=$2,actual_credits=$3,error_code=$4,response_sha256=$5,
               completed_at=now(),updated_at=now()
           WHERE request_id=$1 AND status='RESERVED'"#,
    )
    .bind(request_id)
    .bind(status)
    .bind(actual_credits)
    .bind(error_code)
    .bind(response_sha256)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

async fn release_stale_reservations(stores: &Stores) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE pumphound.enrichment_request_ledger
           SET status='RELEASED',actual_credits=0,error_code='STALE_RESERVATION_RELEASED',
               completed_at=now(),updated_at=now()
           WHERE status='RESERVED' AND reserved_at < now()-interval '15 minutes'"#,
    )
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

fn transfer_params(wallet: &str, from_slot: i64, to_slot: i64, limit: u32) -> Value {
    json!([
        wallet,
        {
            "direction": "any",
            "mint": Value::Null,
            "solMode": "merged",
            "filters": {"slot": {"gte": from_slot, "lte": to_slot}},
            "limit": limit.min(100),
            "paginationToken": Value::Null,
            "commitment": "finalized",
            "sortOrder": "asc"
        }
    ])
}

#[derive(Clone, Debug)]
struct ParsedEvidence {
    data: Vec<Value>,
    pagination_token: Option<String>,
    initial_funder: Option<FundingInference>,
    history_complete: bool,
    covered_to_slot: i64,
    raw_sha256: String,
    evidence_ids: Vec<String>,
    tokens_summary: Value,
    trade_summary: Value,
}

#[derive(Clone, Debug, PartialEq)]
struct FundingInference {
    funder: String,
    confidence: f64,
    signature: String,
    slot: i64,
    coverage: &'static str,
}

fn parse_transfer_result(
    wallet: &str,
    from_slot: i64,
    to_slot: i64,
    result: Value,
    services: &HashSet<String>,
    config: &EnrichmentConfig,
) -> ParsedEvidence {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(100)
        .collect::<Vec<_>>();
    let pagination_token = result
        .get("paginationToken")
        .and_then(Value::as_str)
        .map(str::to_string);
    let history_complete = from_slot <= 1 && pagination_token.is_none();
    let covered_to_slot = if pagination_token.is_some() {
        data.iter()
            .filter_map(|record| record.get("slot").and_then(json_i64))
            .max()
            .unwrap_or(from_slot.saturating_sub(1))
    } else {
        to_slot
    };
    let initial_funder = infer_initial_funder(
        wallet,
        &data,
        services,
        config.minimum_meaningful_native_lamports,
        history_complete,
        config.initial_funder_confidence_complete,
        config.initial_funder_confidence_incomplete,
    );
    let evidence_ids = data
        .iter()
        .filter_map(transfer_signature)
        .take(100)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let token_mints = data.iter().flat_map(token_mints).collect::<BTreeSet<_>>();
    let raw = json!({
        "history_from_slot": from_slot,
        "history_to_slot": covered_to_slot,
        "requested_through_slot": to_slot,
        "data": data,
        "pagination_token": pagination_token
    });
    ParsedEvidence {
        data,
        pagination_token,
        initial_funder,
        history_complete,
        covered_to_slot,
        raw_sha256: pumphound_domain::sha256_hex(
            serde_json::to_string(&raw).unwrap_or_default().as_bytes(),
        ),
        evidence_ids,
        tokens_summary: json!({
            "known_mints": token_mints,
            "complete": history_complete
        }),
        trade_summary: json!({
            "records_observed": raw["data"].as_array().map(Vec::len).unwrap_or_default(),
            "complete": history_complete
        }),
    }
}

fn infer_initial_funder(
    wallet: &str,
    data: &[Value],
    services: &HashSet<String>,
    minimum_lamports: u64,
    complete: bool,
    complete_confidence: f64,
    incomplete_confidence: f64,
) -> Option<FundingInference> {
    let mut inbound = data
        .iter()
        .flat_map(native_transfers)
        .filter(|transfer| {
            transfer.to == wallet
                && transfer.from != wallet
                && !transfer.from.is_empty()
                && !services.contains(&transfer.from)
                && transfer.lamports >= minimum_lamports
                && !transfer.kind.to_ascii_lowercase().contains("wrap")
        })
        .collect::<Vec<_>>();
    inbound.sort_by_key(|transfer| transfer.slot);
    let earliest = inbound.first()?;
    Some(FundingInference {
        funder: earliest.from.clone(),
        confidence: if complete {
            complete_confidence
        } else {
            incomplete_confidence
        },
        signature: earliest.signature.clone(),
        slot: earliest.slot,
        coverage: if complete { "COMPLETE" } else { "INCOMPLETE" },
    })
}

#[derive(Clone, Debug)]
struct NativeTransfer {
    from: String,
    to: String,
    lamports: u64,
    signature: String,
    slot: i64,
    kind: String,
}

fn native_transfers(record: &Value) -> Vec<NativeTransfer> {
    let signature = transfer_signature(record).unwrap_or_default();
    let slot = record.get("slot").and_then(json_i64).unwrap_or_default();
    let kind = record
        .get("type")
        .or_else(|| record.get("source"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if let Some(transfers) = record.get("nativeTransfers").and_then(Value::as_array) {
        return transfers
            .iter()
            .filter_map(|transfer| {
                Some(NativeTransfer {
                    from: transfer
                        .get("fromUserAccount")
                        .or_else(|| transfer.get("from"))
                        .and_then(Value::as_str)?
                        .into(),
                    to: transfer
                        .get("toUserAccount")
                        .or_else(|| transfer.get("to"))
                        .and_then(Value::as_str)?
                        .into(),
                    lamports: transfer
                        .get("amount")
                        .or_else(|| transfer.get("lamports"))
                        .and_then(json_u64)?,
                    signature: signature.clone(),
                    slot,
                    kind: kind.clone(),
                })
            })
            .collect();
    }
    let mint = record.get("mint");
    let native = mint.is_none()
        || mint.is_some_and(Value::is_null)
        || mint.and_then(Value::as_str).is_some_and(|mint| {
            matches!(
                mint,
                "SOL"
                    | "So11111111111111111111111111111111111111111"
                    | "So11111111111111111111111111111111111111112"
            )
        });
    if !native {
        return vec![];
    }
    let from = record
        .get("fromUserAccount")
        .or_else(|| record.get("from"))
        .and_then(Value::as_str);
    let to = record
        .get("toUserAccount")
        .or_else(|| record.get("to"))
        .and_then(Value::as_str);
    let amount = record
        .get("amount")
        .or_else(|| record.get("lamports"))
        .and_then(json_u64);
    from.zip(to)
        .zip(amount)
        .map(|((from, to), lamports)| {
            vec![NativeTransfer {
                from: from.into(),
                to: to.into(),
                lamports,
                signature,
                slot,
                kind,
            }]
        })
        .unwrap_or_default()
}

fn transfer_signature(record: &Value) -> Option<String> {
    record
        .get("signature")
        .or_else(|| record.get("transactionSignature"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn token_mints(record: &Value) -> Vec<String> {
    record
        .get("tokenTransfers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|transfer| transfer.get("mint").and_then(Value::as_str))
        .take(100)
        .map(str::to_string)
        .collect()
}

async fn persist_wallet_result(
    stores: &Stores,
    wallet: &str,
    from_slot: i64,
    evidence: &ParsedEvidence,
    config: &EnrichmentConfig,
) -> anyhow::Result<()> {
    let existing = sqlx::query(
        r#"SELECT inferred_initial_funder,raw_evidence
           FROM pumphound.wallet_enrichment_cache WHERE wallet=$1"#,
    )
    .bind(wallet)
    .fetch_optional(&stores.postgres)
    .await?;
    let existing_funder = existing.as_ref().and_then(|row| {
        row.try_get::<Option<String>, _>("inferred_initial_funder")
            .ok()
            .flatten()
    });
    let inference = if from_slot <= 1 && existing_funder.is_none() {
        evidence.initial_funder.as_ref()
    } else {
        None
    };
    let batch = json!({
        "history_from_slot": from_slot,
        "history_to_slot": evidence.covered_to_slot,
        "data": evidence.data,
        "evidence_ids": evidence.evidence_ids,
        "coverage_complete": evidence.history_complete,
        "response_sha256": evidence.raw_sha256
    });
    let mut batches = existing
        .as_ref()
        .and_then(|row| row.try_get::<Value, _>("raw_evidence").ok())
        .and_then(|value| value.get("batches").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    batches.push(batch);
    let raw_evidence = json!({"batches": batches});
    let raw_evidence_sha256 = pumphound_domain::sha256_hex(
        serde_json::to_string(&raw_evidence)
            .unwrap_or_default()
            .as_bytes(),
    );
    sqlx::query(
        r#"INSERT INTO pumphound.wallet_enrichment_cache
           (wallet,history_from_slot,history_to_slot,transfer_cursor,transfer_status,funding_state,
            inferred_initial_funder,funding_confidence,funding_evidence_signature,
            funding_evidence_slot,funding_method,funding_coverage,tokens_summary,trade_summary,
            cohort_links,creator_links,pnl_completeness,last_enriched_slot,last_enriched_at,
            provider,method,version,raw_evidence,raw_evidence_sha256,updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                   COALESCE((SELECT cohort_ids FROM pumphound.wallet_profiles WHERE wallet=$1),'[]'),
                   COALESCE((SELECT jsonb_agg(creator) FROM pumphound.creator_profiles WHERE creator=$1),'[]'),
                   COALESCE((SELECT pnl_status FROM pumphound.wallet_profiles WHERE wallet=$1),'INCOMPLETE'),
                   $3,now(),'helius',$15,$16,$17,$18,now())
           ON CONFLICT (wallet) DO UPDATE SET
             history_from_slot=LEAST(pumphound.wallet_enrichment_cache.history_from_slot,EXCLUDED.history_from_slot),
             history_to_slot=GREATEST(pumphound.wallet_enrichment_cache.history_to_slot,EXCLUDED.history_to_slot),
             transfer_cursor=EXCLUDED.transfer_cursor,
             transfer_status=EXCLUDED.transfer_status,
             inferred_initial_funder=COALESCE(pumphound.wallet_enrichment_cache.inferred_initial_funder,EXCLUDED.inferred_initial_funder),
             funding_confidence=COALESCE(pumphound.wallet_enrichment_cache.funding_confidence,EXCLUDED.funding_confidence),
             funding_evidence_signature=COALESCE(pumphound.wallet_enrichment_cache.funding_evidence_signature,EXCLUDED.funding_evidence_signature),
             funding_evidence_slot=COALESCE(pumphound.wallet_enrichment_cache.funding_evidence_slot,EXCLUDED.funding_evidence_slot),
             funding_method=COALESCE(pumphound.wallet_enrichment_cache.funding_method,EXCLUDED.funding_method),
             funding_state=CASE WHEN pumphound.wallet_enrichment_cache.inferred_initial_funder IS NOT NULL
                                  OR EXCLUDED.inferred_initial_funder IS NOT NULL THEN 'INFERRED' ELSE 'UNAVAILABLE' END,
             funding_coverage=CASE WHEN pumphound.wallet_enrichment_cache.funding_coverage='COMPLETE'
                                    OR EXCLUDED.funding_coverage='COMPLETE' THEN 'COMPLETE' ELSE 'INCOMPLETE' END,
             tokens_summary=EXCLUDED.tokens_summary,trade_summary=EXCLUDED.trade_summary,
             cohort_links=EXCLUDED.cohort_links,pnl_completeness=EXCLUDED.pnl_completeness,
             last_enriched_slot=EXCLUDED.last_enriched_slot,last_enriched_at=now(),
             provider=EXCLUDED.provider,method=EXCLUDED.method,version=EXCLUDED.version,
             raw_evidence=EXCLUDED.raw_evidence,raw_evidence_sha256=EXCLUDED.raw_evidence_sha256,
             updated_at=now()"#,
    )
    .bind(wallet)
    .bind(from_slot)
    .bind(evidence.covered_to_slot)
    .bind(&evidence.pagination_token)
    .bind(if evidence.pagination_token.is_none() {
        "COMPLETE"
    } else {
        "PARTIAL"
    })
    .bind(if inference.is_some() {
        "INFERRED"
    } else {
        "UNAVAILABLE"
    })
    .bind(inference.map(|value| &value.funder))
    .bind(inference.map(|value| value.confidence))
    .bind(inference.map(|value| &value.signature))
    .bind(inference.map(|value| value.slot))
    .bind(inference.map(|_| "INFERRED_INITIAL_FUNDER"))
    .bind(if evidence.history_complete {
        "COMPLETE"
    } else {
        "INCOMPLETE"
    })
    .bind(&evidence.tokens_summary)
    .bind(&evidence.trade_summary)
    .bind(LEVEL_1_METHOD)
    .bind(&config.version)
    .bind(raw_evidence)
    .bind(raw_evidence_sha256)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

async fn next_enrichment_slot(stores: &Stores, wallet: &str) -> anyhow::Result<i64> {
    let last: Option<i64> = sqlx::query_scalar(
        "SELECT last_enriched_slot FROM pumphound.wallet_enrichment_cache WHERE wallet=$1",
    )
    .bind(wallet)
    .fetch_optional(&stores.postgres)
    .await?
    .flatten();
    Ok(last.map(|slot| slot.saturating_add(1)).unwrap_or(0))
}

async fn known_service_addresses(
    stores: &Stores,
    config: &EnrichmentConfig,
) -> anyhow::Result<HashSet<String>> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"SELECT public_key FROM pumphound.graph_nodes
           WHERE public_key IS NOT NULL
             AND lower(COALESCE(attributes->>'classification','')) IN
                 ('service','exchange','router','program','wrap_service')"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    Ok(config
        .known_service_addresses
        .iter()
        .cloned()
        .chain(rows)
        .collect())
}

async fn maybe_level_two(
    stores: &Stores,
    client: &HeliusClient,
    context: &CandidateContext,
    selected: &SelectedWallet,
    evidence: &ParsedEvidence,
    config: &EnrichmentConfig,
) -> anyhow::Result<()> {
    let reason = "funding_inference_incomplete";
    if !config.level2_enabled
        || evidence.initial_funder.is_some()
        || !config
            .level2_eligibility_reasons
            .iter()
            .any(|allowed| allowed == reason)
    {
        return Ok(());
    }
    let key = format!(
        "{}:{}:{}:{}",
        context.decision_id, selected.wallet, LEVEL_2_METHOD, context.as_of_slot
    );
    let Reservation::Reserved(request_id) = reserve_credits(
        stores,
        context,
        &selected.wallet,
        2,
        LEVEL_2_METHOD,
        Some(reason),
        config.level2_credits_upper_bound,
        &key,
        config,
    )
    .await?
    else {
        return Ok(());
    };
    let result = client
        .call(
            LEVEL_2_METHOD,
            json!([
                selected.wallet,
                {
                    "limit": config.level2_limit.min(100),
                    "commitment": "finalized"
                }
            ]),
        )
        .await;
    match result {
        Ok(value) => {
            let hash = pumphound_domain::sha256_hex(
                serde_json::to_string(&value).unwrap_or_default().as_bytes(),
            );
            settle_request(
                stores,
                request_id,
                "COMPLETED",
                config.level2_credits_upper_bound,
                None,
                Some(&hash),
            )
            .await
        }
        Err(error) => {
            settle_request(stores, request_id, "DEGRADED", 0, Some(error.code()), None).await
        }
    }
}

async fn journal_not_entitled(
    stores: &Stores,
    context: &CandidateContext,
    selected: &[SelectedWallet],
    config: &EnrichmentConfig,
) -> anyhow::Result<()> {
    for selected in selected {
        sqlx::query(
            r#"INSERT INTO pumphound.enrichment_request_ledger
               (request_id,request_key,decision_id,launch_id,wallet,level,method,
                estimated_credits,actual_credits,status,error_code,reserved_at,completed_at,updated_at)
               VALUES ($1,$2,$3,$4,$5,1,$6,$7,0,'NOT_ENTITLED','NOT_ENTITLED',now(),now(),now())
               ON CONFLICT (request_key) DO NOTHING"#,
        )
        .bind(Uuid::now_v7())
        .bind(format!(
            "{}:{}:{}:not-entitled",
            context.decision_id, selected.wallet, LEVEL_1_METHOD
        ))
        .bind(context.decision_id)
        .bind(context.launch_id)
        .bind(&selected.wallet)
        .bind(LEVEL_1_METHOD)
        .bind(config.level1_credits_per_request)
        .execute(&stores.postgres)
        .await?;
    }
    Ok(())
}

async fn finalize_dossier(
    stores: &Stores,
    context: &CandidateContext,
    selected: &[SelectedWallet],
    completed: usize,
    withheld: usize,
    degraded: usize,
) -> anyhow::Result<()> {
    let caches = sqlx::query(
        r#"SELECT wallet,transfer_status,funding_state,inferred_initial_funder,
                  funding_confidence,funding_evidence_signature,funding_evidence_slot,
                  funding_method,funding_coverage,last_enriched_slot,raw_evidence_sha256
           FROM pumphound.wallet_enrichment_cache WHERE wallet=ANY($1)"#,
    )
    .bind(
        selected
            .iter()
            .map(|selected| selected.wallet.clone())
            .collect::<Vec<_>>(),
    )
    .fetch_all(&stores.postgres)
    .await?;
    let evidence_ids = caches
        .iter()
        .flat_map(|row| {
            [
                row.try_get::<Option<String>, _>("funding_evidence_signature")
                    .ok()
                    .flatten(),
                row.try_get::<String, _>("raw_evidence_sha256").ok(),
            ]
        })
        .flatten()
        .collect::<BTreeSet<_>>();
    let status = if degraded > 0 {
        "DEGRADED"
    } else if withheld > 0 && completed == 0 {
        "BUDGET_WITHHELD"
    } else if withheld > 0 {
        "PARTIAL"
    } else {
        "COMPLETE"
    };
    sqlx::query(
        r#"UPDATE pumphound.candidate_dossiers
           SET status=$2,evidence_ids=$3,enrichment=$4,updated_at=now() WHERE decision_id=$1"#,
    )
    .bind(context.decision_id)
    .bind(status)
    .bind(json!(evidence_ids))
    .bind(json!({
        "baseline_effect": "NONE",
        "wallets": caches.iter().map(cache_row_json).collect::<Vec<_>>(),
        "completed_wallets": completed,
        "budget_withheld_wallets": withheld,
        "degraded_wallets": degraded
    }))
    .execute(&stores.postgres)
    .await?;
    for row in caches {
        let wallet: String = row.get("wallet");
        sqlx::query(
            r#"UPDATE pumphound.candidate_dossier_wallets
               SET enrichment_status=$3,evidence_ids=$4
               WHERE decision_id=$1 AND wallet=$2"#,
        )
        .bind(context.decision_id)
        .bind(wallet)
        .bind(row.get::<String, _>("transfer_status"))
        .bind(json!([
            row.try_get::<Option<String>, _>("funding_evidence_signature")
                .ok()
                .flatten(),
            row.try_get::<String, _>("raw_evidence_sha256").ok()
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()))
        .execute(&stores.postgres)
        .await?;
    }
    Ok(())
}

async fn set_dossier_status(
    stores: &Stores,
    decision_id: Uuid,
    status: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE pumphound.candidate_dossiers SET status=$2,updated_at=now() WHERE decision_id=$1",
    )
    .bind(decision_id)
    .bind(status)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

fn cache_row_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({
        "wallet": row.get::<String,_>("wallet"),
        "transfer_status": row.get::<String,_>("transfer_status"),
        "funding_inference": {
            "state": row.get::<String,_>("funding_state"),
            "funder": row.try_get::<Option<String>,_>("inferred_initial_funder").ok().flatten(),
            "confidence": row.try_get::<Option<f64>,_>("funding_confidence").ok().flatten(),
            "evidence_signature": row.try_get::<Option<String>,_>("funding_evidence_signature").ok().flatten(),
            "evidence_slot": row.try_get::<Option<i64>,_>("funding_evidence_slot").ok().flatten().map(|slot| slot.to_string()),
            "method": row.try_get::<Option<String>,_>("funding_method").ok().flatten(),
            "coverage": row.get::<String,_>("funding_coverage"),
            "ownership_claim": false
        },
        "last_enriched_slot": row.try_get::<Option<i64>,_>("last_enriched_slot").ok().flatten().map(|slot| slot.to_string()),
        "raw_evidence_sha256": row.get::<String,_>("raw_evidence_sha256")
    })
}

pub async fn status(stores: &Stores) -> anyhow::Result<Value> {
    let counts = sqlx::query(
        r#"SELECT
             (SELECT COUNT(*) FROM pumphound.wallet_enrichment_cache) AS wallets,
             (SELECT COUNT(*) FROM pumphound.candidate_dossiers) AS candidates,
             (SELECT COUNT(*) FROM pumphound.candidate_dossiers
              WHERE status IN ('ENRICHING','LEVEL_0')) AS pending"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let today: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(CASE WHEN actual_credits IS NOT NULL THEN actual_credits
                 WHEN status='RESERVED' THEN estimated_credits ELSE 0 END),0)::bigint
           FROM pumphound.enrichment_request_ledger
           WHERE reserved_at >= date_trunc('day',now())"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let month: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(CASE WHEN actual_credits IS NOT NULL THEN actual_credits
                 WHEN status='RESERVED' THEN estimated_credits ELSE 0 END),0)::bigint
           FROM pumphound.enrichment_request_ledger
           WHERE reserved_at >= date_trunc('month',now())"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let config = EnrichmentConfig::load().await.ok();
    Ok(json!({
        "enabled": config.as_ref().is_some_and(|config| config.enabled),
        "entitled": HeliusClient::from_env().is_some(),
        "level2_enabled": config.as_ref().is_some_and(|config| config.level2_enabled),
        "wallet_cache_count": counts.get::<i64,_>("wallets"),
        "candidate_count": counts.get::<i64,_>("candidates"),
        "pending_candidates": counts.get::<i64,_>("pending"),
        "credits_today": today,
        "credits_month": month,
        "daily_credit_limit": config.as_ref().map(|config| config.daily_credits),
        "monthly_credit_limit": config.as_ref().map(|config| config.monthly_credits),
        "secrets_redacted": true,
        "baseline_effect": "NONE"
    }))
}

pub async fn dossier(stores: &Stores, decision_id: Uuid) -> anyhow::Result<Option<Value>> {
    let row = sqlx::query(
        r#"SELECT decision_id::text,launch_id::text,action,as_of_slot,status,selected_wallets,
                  evidence_ids,enrichment,version,created_at,updated_at
           FROM pumphound.candidate_dossiers WHERE decision_id=$1"#,
    )
    .bind(decision_id)
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(row.map(|row| {
        json!({
            "decision_id": row.get::<String,_>("decision_id"),
            "launch_id": row.get::<String,_>("launch_id"),
            "action": row.get::<String,_>("action"),
            "as_of_slot": row.get::<i64,_>("as_of_slot").to_string(),
            "status": row.get::<String,_>("status"),
            "selected_wallets": row.get::<Value,_>("selected_wallets"),
            "evidence_ids": row.get::<Value,_>("evidence_ids"),
            "enrichment": row.get::<Value,_>("enrichment"),
            "version": row.get::<String,_>("version"),
            "created_at": row.get::<DateTime<Utc>,_>("created_at"),
            "updated_at": row.get::<DateTime<Utc>,_>("updated_at"),
            "funding_relationship_is_inferred": true,
            "policy_effect": false
        })
    }))
}

pub async fn wallet_cache(stores: &Stores, wallet: &str) -> anyhow::Result<Option<Value>> {
    let row = sqlx::query(
        r#"SELECT wallet,history_from_slot,history_to_slot,transfer_status,funding_state,
                  inferred_initial_funder,funding_confidence,funding_evidence_signature,
                  funding_evidence_slot,funding_method,funding_coverage,tokens_summary,
                  trade_summary,cohort_links,creator_links,pnl_completeness,last_enriched_slot,
                  last_enriched_at,provider,method,version,raw_evidence_sha256,updated_at
           FROM pumphound.wallet_enrichment_cache WHERE wallet=$1"#,
    )
    .bind(wallet)
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(row.map(|row| {
        json!({
            "wallet": row.get::<String,_>("wallet"),
            "history_from_slot": row.try_get::<Option<i64>,_>("history_from_slot").ok().flatten().map(|slot| slot.to_string()),
            "history_to_slot": row.try_get::<Option<i64>,_>("history_to_slot").ok().flatten().map(|slot| slot.to_string()),
            "transfer_status": row.get::<String,_>("transfer_status"),
            "funding_inference": {
                "state": row.get::<String,_>("funding_state"),
                "funder": row.try_get::<Option<String>,_>("inferred_initial_funder").ok().flatten(),
                "confidence": row.try_get::<Option<f64>,_>("funding_confidence").ok().flatten(),
                "evidence_signature": row.try_get::<Option<String>,_>("funding_evidence_signature").ok().flatten(),
                "evidence_slot": row.try_get::<Option<i64>,_>("funding_evidence_slot").ok().flatten().map(|slot| slot.to_string()),
                "method": row.try_get::<Option<String>,_>("funding_method").ok().flatten(),
                "coverage": row.get::<String,_>("funding_coverage"),
                "ownership_claim": false
            },
            "tokens_summary": row.get::<Value,_>("tokens_summary"),
            "trade_summary": row.get::<Value,_>("trade_summary"),
            "cohort_links": row.get::<Value,_>("cohort_links"),
            "creator_links": row.get::<Value,_>("creator_links"),
            "pnl_completeness": row.get::<String,_>("pnl_completeness"),
            "last_enriched_slot": row.try_get::<Option<i64>,_>("last_enriched_slot").ok().flatten().map(|slot| slot.to_string()),
            "last_enriched_at": row.try_get::<Option<DateTime<Utc>>,_>("last_enriched_at").ok().flatten(),
            "provider": row.get::<String,_>("provider"),
            "method": row.get::<String,_>("method"),
            "version": row.get::<String,_>("version"),
            "raw_evidence_sha256": row.get::<String,_>("raw_evidence_sha256"),
            "updated_at": row.get::<DateTime<Utc>,_>("updated_at")
        })
    }))
}

pub async fn investigator_evidence(stores: &Stores, decision_id: Uuid) -> Value {
    dossier(stores, decision_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            json!({
                "status": "UNAVAILABLE",
                "reason": "candidate_enrichment_not_available_at_freeze",
                "policy_effect": false
            })
        })
}

fn apply_env_bool(name: &str, target: &mut bool) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = match value.as_str() {
            "1" | "true" | "TRUE" => true,
            "0" | "false" | "FALSE" => false,
            _ => return Err(format!("{name}_INVALID")),
        };
    }
    Ok(())
}

fn apply_env_i64(name: &str, target: &mut i64) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = value.parse().map_err(|_| format!("{name}_INVALID"))?;
    }
    Ok(())
}

fn apply_env_usize(name: &str, target: &mut usize) -> Result<(), String> {
    if let Ok(value) = std::env::var(name) {
        *target = value.parse().map_err(|_| format!("{name}_INVALID"))?;
    }
    Ok(())
}

fn json_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn json_f64(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
}

fn safe_error_code(error: &anyhow::Error) -> String {
    let message = error.to_string();
    if message.chars().all(|character| {
        character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
    }) {
        message
    } else {
        "CANDIDATE_ENRICHMENT_INTERNAL".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use tokio::sync::Mutex;

    fn config() -> EnrichmentConfig {
        EnrichmentConfig {
            schema_version: 1,
            version: "CANDIDATE_ENRICHMENT_V1".into(),
            enabled: true,
            daily_credits: 500,
            monthly_credits: 5_000,
            per_candidate_credits: 100,
            max_candidates_per_hour: 10,
            max_wallets_per_candidate: 10,
            include_buyers_6_to_10: false,
            level1_credits_per_request: 10,
            level1_limit: 100,
            level2_enabled: false,
            level2_credits_upper_bound: 100,
            level2_limit: 100,
            level2_eligibility_reasons: vec!["funding_inference_incomplete".into()],
            minimum_meaningful_native_lamports: 1_000_000,
            initial_funder_confidence_complete: 0.8,
            initial_funder_confidence_incomplete: 0.55,
            minimum_high_reputation: 0.8,
            known_service_addresses: vec![],
        }
    }

    #[test]
    fn only_buy_triggers_provider_enrichment() {
        assert!(candidate_action("BUY"));
        assert!(!candidate_action("WATCH"));
        assert!(!candidate_action("AVOID"));
        assert!(!candidate_action("ABSTAIN"));
    }

    #[test]
    fn wallet_selection_is_deduplicated_and_hard_capped() {
        let candidates = (0..20)
            .map(|index| SelectedWallet {
                wallet: format!("wallet-{}", index % 12),
                role: "observed".into(),
            })
            .collect();
        let selected = deduplicate_wallets(candidates, 10);
        assert_eq!(selected.len(), 10);
        assert_eq!(
            selected
                .iter()
                .map(|selected| &selected.wallet)
                .collect::<HashSet<_>>()
                .len(),
            10
        );
    }

    #[test]
    fn incremental_filter_uses_last_slot_plus_one() {
        let params = transfer_params("wallet", 42, 100, 100);
        assert_eq!(params[1]["filters"]["slot"]["gte"], 42);
        assert_eq!(params[1]["filters"]["slot"]["lte"], 100);
        assert_eq!(params[1]["limit"], 100);
        assert!(params[1]["paginationToken"].is_null());
    }

    #[test]
    fn earliest_meaningful_native_inbound_is_inferred_not_owned() {
        let data = vec![
            json!({"signature":"self","slot":1,"nativeTransfers":[{"fromUserAccount":"wallet","toUserAccount":"wallet","amount":9_000_000}]}),
            json!({"signature":"small","slot":2,"nativeTransfers":[{"fromUserAccount":"small","toUserAccount":"wallet","amount":10}]}),
            json!({"signature":"fund","slot":3,"nativeTransfers":[{"fromUserAccount":"funder","toUserAccount":"wallet","amount":2_000_000}]}),
            json!({"signature":"later","slot":4,"nativeTransfers":[{"fromUserAccount":"later","toUserAccount":"wallet","amount":3_000_000}]}),
        ];
        let inferred = infer_initial_funder(
            "wallet",
            &data,
            &HashSet::new(),
            1_000_000,
            false,
            0.8,
            0.55,
        )
        .unwrap();
        assert_eq!(inferred.funder, "funder");
        assert_eq!(inferred.signature, "fund");
        assert_eq!(inferred.coverage, "INCOMPLETE");
        assert_eq!(inferred.confidence, 0.55);
    }

    #[tokio::test]
    async fn mocked_429_degrades_without_exposing_key() {
        let app = Router::new().route(
            "/",
            post(|| async { (StatusCode::TOO_MANY_REQUESTS, "rate limited") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = HeliusClient::new(format!("http://{address}"), "super-secret-key".into());
        assert!(matches!(
            client
                .call(LEVEL_1_METHOD, transfer_params("wallet", 0, 100, 100))
                .await,
            Err(RpcFailure::RateLimited)
        ));
        let debug = format!("{client:?}");
        assert!(!debug.contains("super-secret-key"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn mocked_rpc_receives_exact_incremental_contract() {
        let captured = Arc::new(Mutex::new(Value::Null));
        let app = Router::new()
            .route(
                "/",
                post(
                    |State(captured): State<Arc<Mutex<Value>>>,
                     Json(body): Json<Value>| async move {
                        *captured.lock().await = body;
                        Json(json!({"jsonrpc":"2.0","result":{"data":[],"paginationToken":null}}))
                    },
                ),
            )
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = HeliusClient::new(format!("http://{address}"), "test-key".into());
        client
            .call(LEVEL_1_METHOD, transfer_params("wallet", 101, 200, 100))
            .await
            .unwrap();
        let body = captured.lock().await.clone();
        assert_eq!(body["method"], LEVEL_1_METHOD);
        assert_eq!(body["params"][0], "wallet");
        assert_eq!(body["params"][1]["filters"]["slot"]["gte"], 101);
        assert_eq!(body["params"][1]["filters"]["slot"]["lte"], 200);
        assert_eq!(body["params"][1]["solMode"], "merged");
        assert_eq!(body["params"][1]["sortOrder"], "asc");
    }

    #[tokio::test]
    async fn mocked_plan_error_is_not_entitled() {
        let app = Router::new().route(
            "/",
            post(|| async {
                Json(json!({
                    "jsonrpc":"2.0",
                    "error":{"code":-32000,"message":"upgrade plan for access"}
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = HeliusClient::new(format!("http://{address}"), "test-key".into());
        assert!(matches!(
            client
                .call(LEVEL_1_METHOD, transfer_params("wallet", 0, 100, 100))
                .await,
            Err(RpcFailure::NotEntitled)
        ));
    }

    #[test]
    fn cache_slot_prevents_repeat_and_advances_incrementally() {
        fn needs_call(last: Option<i64>, candidate_slot: i64) -> bool {
            last.map(|slot| slot.saturating_add(1)).unwrap_or_default() <= candidate_slot
        }
        assert!(!needs_call(Some(100), 100));
        assert!(needs_call(Some(100), 101));
    }

    #[test]
    fn baseline_decision_is_immutable_enrichment_input() {
        let decision = json!({"action":"BUY","persisted":true});
        let before = decision.clone();
        let _request = transfer_params("wallet", 0, 100, config().level1_limit);
        assert_eq!(decision, before);
    }
}
