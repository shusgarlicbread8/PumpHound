use crate::state::AppState;
use chrono::{DateTime, Utc};
use pumphound_storage::Stores;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::Row;
use std::collections::BTreeSet;
use std::sync::Arc;
use uuid::Uuid;

const HTTP_TIMEOUT_SECS: u64 = 20;
const REMOTE_TIMEOUT_SECS: f64 = 15.0;
const MAX_GRAPH_FACTS: i64 = 100;
const MAX_FUNDING_EDGES: i64 = 25;
const MAX_COHORTS: i64 = 20;
const MAX_DERIVED_EDGES: i64 = 50;
const MAX_SIGNATURES: usize = 50;
const MAX_EXPLANATION_ITEMS: usize = 24;
const MAX_STRING_BYTES: usize = 2_048;
const MAX_EVIDENCE_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct InvestigatorClient {
    http: reqwest::Client,
    url: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct InvestigateRequest {
    pub subject_type: String,
    pub subject_id: String,
    pub as_of_slot: i64,
    pub knowledge_cutoff: DateTime<Utc>,
    pub roles: Vec<String>,
    pub evidence: Value,
    pub timeout_s: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InvestigateResponse {
    pub job_id: String,
    pub evidence_pack_sha256: String,
    pub provider: String,
    pub roles: Value,
    pub policy_effect: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl InvestigateResponse {
    fn validate(&self) -> Result<(), InvestigatorError> {
        if self.policy_effect
            || self
                .roles
                .as_object()
                .is_some_and(|roles| roles.values().any(nested_policy_effect))
        {
            return Err(InvestigatorError::PolicyEffectRejected);
        }
        if self.job_id.trim().is_empty() {
            return Err(InvestigatorError::InvalidResponse("job_id_missing".into()));
        }
        if self.provider.trim().is_empty() {
            return Err(InvestigatorError::InvalidResponse(
                "provider_missing".into(),
            ));
        }
        if self.evidence_pack_sha256.len() != 64
            || !self
                .evidence_pack_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(InvestigatorError::InvalidResponse(
                "evidence_pack_sha256_invalid".into(),
            ));
        }
        if !self.roles.is_object() || self.roles.as_object().is_some_and(Map::is_empty) {
            return Err(InvestigatorError::InvalidResponse(
                "roles_missing_or_not_object".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InvestigatorError {
    #[error("investigator unavailable: {0}")]
    Unavailable(String),
    #[error("invalid investigator response: {0}")]
    InvalidResponse(String),
    #[error("investigator response attempted policy effect")]
    PolicyEffectRejected,
}

impl InvestigatorError {
    fn code(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "INVESTIGATOR_UNAVAILABLE",
            Self::InvalidResponse(_) => "INVALID_INVESTIGATOR_RESPONSE",
            Self::PolicyEffectRejected => "POLICY_EFFECT_REJECTED",
        }
    }

    fn provider_status(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::InvalidResponse(_) | Self::PolicyEffectRejected => "degraded",
        }
    }
}

impl InvestigatorClient {
    pub fn from_env() -> Self {
        let url = std::env::var("PUMPHOUND_INVESTIGATOR_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8092/internal/investigate".into());
        Self::with_url(url)
    }

    fn with_url(url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
                .build()
                .unwrap_or_default(),
            url: url.into(),
        }
    }

    pub async fn investigate(
        &self,
        request: &InvestigateRequest,
    ) -> Result<InvestigateResponse, InvestigatorError> {
        let mut request = request.clone();
        redact_secrets(&mut request.evidence);
        let evidence_bytes = serde_json::to_vec(&request.evidence)
            .map_err(|error| InvestigatorError::InvalidResponse(error.to_string()))?;
        if evidence_bytes.len() > MAX_EVIDENCE_BYTES {
            return Err(InvestigatorError::InvalidResponse(
                "evidence_size_limit_exceeded".into(),
            ));
        }
        let response = self
            .http
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .map_err(|error| InvestigatorError::Unavailable(error.to_string()))?;
        if !response.status().is_success() {
            return Err(InvestigatorError::Unavailable(format!(
                "http_status:{}",
                response.status()
            )));
        }
        let response = response
            .json::<InvestigateResponse>()
            .await
            .map_err(|error| InvestigatorError::InvalidResponse(error.to_string()))?;
        response.validate()?;
        Ok(response)
    }
}

pub fn advisory_action(action: &str) -> bool {
    matches!(action, "BUY" | "WATCH")
}

#[cfg(test)]
pub async fn call_if_advisory(
    action: &str,
    client: &InvestigatorClient,
    request: &InvestigateRequest,
) -> Option<Result<InvestigateResponse, InvestigatorError>> {
    if advisory_action(action) {
        Some(client.investigate(request).await)
    } else {
        None
    }
}

pub fn spawn_for_decision(state: Arc<AppState>, decision_id: Uuid, action: &str) {
    if !advisory_action(action) {
        return;
    }
    let action = action.to_string();
    tokio::spawn(async move {
        let client = InvestigatorClient::from_env();
        let result = async {
            let context = context_for_decision(&state.stores, decision_id).await?;
            if context.action != action {
                anyhow::bail!("decision_action_changed");
            }
            investigate_and_persist(&state.stores, &client, context).await
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(%decision_id, %error, "advisory investigation failed");
        }
    });
}

#[derive(Clone, Debug)]
pub struct InvestigationContext {
    pub investigation_id: Uuid,
    pub action: String,
    pub subject_type: String,
    pub subject_id: String,
    pub as_of_slot: i64,
    pub knowledge_cutoff: DateTime<Utc>,
    pub roles: Vec<String>,
    pub evidence: Value,
}

pub async fn manual_context(
    stores: &Stores,
    subject_type: &str,
    subject_id: &str,
    as_of_slot: i64,
    knowledge_cutoff: DateTime<Utc>,
    roles: Vec<String>,
) -> anyhow::Result<InvestigationContext> {
    let evidence = freeze_subject_evidence(
        stores,
        subject_type,
        subject_id,
        as_of_slot,
        knowledge_cutoff,
        None,
    )
    .await?;
    Ok(InvestigationContext {
        investigation_id: Uuid::now_v7(),
        action: "MANUAL".into(),
        subject_type: subject_type.into(),
        subject_id: subject_id.into(),
        as_of_slot,
        knowledge_cutoff,
        roles: normalized_roles(roles),
        evidence,
    })
}

pub async fn investigate_and_persist(
    stores: &Stores,
    client: &InvestigatorClient,
    context: InvestigationContext,
) -> anyhow::Result<Uuid> {
    let local_hash = pumphound_domain::sha256_hex(&serde_json::to_vec(&context.evidence)?);
    let started_at = Utc::now();
    let request = InvestigateRequest {
        subject_type: context.subject_type.clone(),
        subject_id: context.subject_id.clone(),
        as_of_slot: context.as_of_slot,
        knowledge_cutoff: context.knowledge_cutoff,
        roles: context.roles.clone(),
        evidence: context.evidence.clone(),
        timeout_s: REMOTE_TIMEOUT_SECS,
    };
    match client.investigate(&request).await {
        Ok(response) => {
            let (model_id, prompt_version) = role_metadata(&response.roles);
            let role_validations = role_validations(&response.roles);
            let grounded = role_validations.as_object().is_some_and(|values| {
                !values.is_empty()
                    && values.values().all(|value| {
                        value.get("ok").and_then(Value::as_bool) == Some(true)
                            && value.get("affects_policy").and_then(Value::as_bool) != Some(true)
                    })
            });
            persist_row(
                stores,
                &context,
                &response.evidence_pack_sha256,
                &response.provider,
                &model_id,
                &prompt_version,
                serde_json::to_value(&response)?,
                json!({
                    "grounded": grounded,
                    "policy_effect": false,
                    "remote_job_id": response.job_id,
                    "role_validations": role_validations,
                    "local_evidence_sha256": local_hash,
                    "knowledge_cutoff": context.knowledge_cutoff,
                    "as_of_slot": context.as_of_slot.to_string(),
                    "started_at": started_at,
                    "completed_at": Utc::now(),
                    "latency_ms": (Utc::now() - started_at).num_milliseconds() as f64
                }),
            )
            .await?;
        }
        Err(error) => {
            persist_row(
                stores,
                &context,
                &local_hash,
                error.provider_status(),
                "unavailable",
                "unavailable",
                json!({
                    "error_code": error.code(),
                    "error": error.to_string(),
                    "policy_effect": false,
                    "decision_unchanged": true
                }),
                json!({
                    "grounded": false,
                    "policy_effect": false,
                    "error_code": error.code(),
                    "knowledge_cutoff": context.knowledge_cutoff,
                    "as_of_slot": context.as_of_slot.to_string()
                }),
            )
            .await?;
        }
    }
    Ok(context.investigation_id)
}

pub async fn load_investigation(
    stores: &Stores,
    investigation_id: Uuid,
) -> anyhow::Result<Option<Value>> {
    let row = sqlx::query(
        r#"SELECT investigation_id::text,subject_type,subject_id,as_of_slot,
                  evidence_pack_sha256,provider,model_id,prompt_version,role,response,
                  validation,created_at
           FROM pumphound.investigations WHERE investigation_id=$1"#,
    )
    .bind(investigation_id)
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(row.map(|row| {
        json!({
            "investigation_id": row.get::<String,_>("investigation_id"),
            "subject_type": row.get::<String,_>("subject_type"),
            "subject_id": row.get::<String,_>("subject_id"),
            "as_of_slot": row.get::<i64,_>("as_of_slot").to_string(),
            "evidence_pack_sha256": row.get::<String,_>("evidence_pack_sha256"),
            "provider": row.get::<String,_>("provider"),
            "model_id": row.get::<String,_>("model_id"),
            "prompt_version": row.get::<String,_>("prompt_version"),
            "role": row.get::<String,_>("role"),
            "response": row.get::<Value,_>("response"),
            "validation": row.get::<Value,_>("validation"),
            "created_at": row.get::<DateTime<Utc>,_>("created_at"),
            "policy_effect": false
        })
    }))
}

async fn context_for_decision(
    stores: &Stores,
    decision_id: Uuid,
) -> anyhow::Result<InvestigationContext> {
    let row = sqlx::query(
        r#"SELECT d.decision_id,d.action,d.knowledge_cutoff,d.exact_inputs,d.model_versions,
                  d.reason_codes,d.payload,l.launch_id,l.mint,l.creator,
                  COALESCE(fs.cutoff_slot,l.creation_slot) AS cutoff_slot
           FROM pumphound.decisions d
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           LEFT JOIN pumphound.feature_snapshots fs
             ON fs.feature_snapshot_id=d.feature_snapshot_id
           WHERE d.decision_id=$1"#,
    )
    .bind(decision_id)
    .fetch_one(&stores.postgres)
    .await?;
    let action: String = row.get("action");
    if !advisory_action(&action) {
        anyhow::bail!("decision_not_advisory_eligible");
    }
    let cutoff: DateTime<Utc> = row.get("knowledge_cutoff");
    let slot: i64 = row.get("cutoff_slot");
    let launch_id: Uuid = row.get("launch_id");
    let mint: String = row.get("mint");
    let creator: String = row.get("creator");
    let candidate_enrichment =
        crate::candidate_enrichment::investigator_evidence(stores, decision_id).await;
    let decision = json!({
        "decision_id": decision_id,
        "action": action,
        "knowledge_cutoff": cutoff,
        "as_of_slot": slot.to_string(),
        "model_versions": row.get::<Value,_>("model_versions"),
        "reason_codes": row.get::<Value,_>("reason_codes"),
        "quantitative_payload": row.get::<Value,_>("payload"),
        "aggregate_model": frozen_aggregate(
            &row.get::<Value,_>("exact_inputs")
        )
    });
    let evidence = freeze_subject_evidence(
        stores,
        "launch",
        &launch_id.to_string(),
        slot,
        cutoff,
        Some(json!({
            "decision": decision,
            "launch": {"launch_id": launch_id, "mint": mint, "creator": creator},
            "candidate_enrichment": candidate_enrichment
        })),
    )
    .await?;
    Ok(InvestigationContext {
        investigation_id: Uuid::now_v7(),
        action,
        subject_type: "launch".into(),
        subject_id: launch_id.to_string(),
        as_of_slot: slot,
        knowledge_cutoff: cutoff,
        // One grounded committee dossier keeps live candidate latency and model cost bounded.
        // Specialist roles remain available through the manual investigation endpoint.
        roles: vec!["committee".into()],
        evidence,
    })
}

async fn freeze_subject_evidence(
    stores: &Stores,
    subject_type: &str,
    subject_id: &str,
    slot: i64,
    cutoff: DateTime<Utc>,
    seed: Option<Value>,
) -> anyhow::Result<Value> {
    let launch = resolve_launch(stores, subject_type, subject_id).await?;
    let (mint, creator) = launch
        .as_ref()
        .map(|value| {
            (
                value.get("mint").and_then(Value::as_str).unwrap_or(""),
                value.get("creator").and_then(Value::as_str).unwrap_or(""),
            )
        })
        .unwrap_or(("", ""));
    let creator_profile = if creator.is_empty() {
        unavailable("creator_not_resolved")
    } else {
        optional_json_row(
            stores,
            "SELECT to_jsonb(cp) AS value FROM pumphound.creator_profiles cp WHERE creator=$1",
            creator,
        )
        .await?
    };
    let wallet_profile = if creator.is_empty() {
        unavailable("wallet_not_resolved")
    } else {
        optional_json_row(
            stores,
            "SELECT to_jsonb(wp) AS value FROM pumphound.wallet_profiles wp WHERE wallet=$1",
            creator,
        )
        .await?
    };
    let reputation = if creator.is_empty() {
        unavailable("wallet_not_resolved")
    } else {
        let row = sqlx::query(
            r#"SELECT to_jsonb(r) AS value FROM pumphound.reputation_snapshots r
               WHERE subject_id=$1 AND as_of_slot <= $2 AND knowledge_cutoff <= $3
               ORDER BY as_of_slot DESC,knowledge_cutoff DESC LIMIT 1"#,
        )
        .bind(creator)
        .bind(slot)
        .bind(cutoff)
        .fetch_optional(&stores.postgres)
        .await?;
        row.map(|row| bounded_json(row.get("value"), 4))
            .unwrap_or_else(|| unavailable("no_time_safe_reputation_snapshot"))
    };
    let funding_edges = if creator.is_empty() {
        vec![]
    } else {
        sqlx::query(
            r#"SELECT edge_id,wallet,funder,source,score,evidence,algorithm_version,known_at
               FROM pumphound.funding_edges
               WHERE wallet=$1 AND known_at <= $2
               ORDER BY known_at DESC LIMIT $3"#,
        )
        .bind(creator)
        .bind(cutoff)
        .bind(MAX_FUNDING_EDGES)
        .fetch_all(&stores.postgres)
        .await?
        .iter()
        .map(|row| {
            json!({
                "edge_id": row.get::<String,_>("edge_id"),
                "wallet": row.get::<String,_>("wallet"),
                "funder": row.try_get::<Option<String>,_>("funder").ok().flatten(),
                "source": row.get::<String,_>("source"),
                "score": row.try_get::<Option<f64>,_>("score").ok().flatten(),
                "evidence": bounded_json(row.get::<Value,_>("evidence"), 3),
                "algorithm_version": row.get::<String,_>("algorithm_version"),
                "known_at": row.get::<DateTime<Utc>,_>("known_at")
            })
        })
        .collect::<Vec<_>>()
    };
    let cohorts = if creator.is_empty() {
        vec![]
    } else {
        sqlx::query(
            r#"SELECT c.cohort_id,c.algorithm_version,c.member_count,c.score,c.first_slot,c.last_slot,
                      c.evidence_signatures,c.attributes
               FROM pumphound.cohort_members m
               JOIN pumphound.wallet_cohorts c ON c.cohort_id=m.cohort_id
               WHERE m.wallet=$1 AND c.last_slot <= $2
               ORDER BY c.last_slot DESC LIMIT $3"#,
        )
        .bind(creator)
        .bind(slot)
        .bind(MAX_COHORTS)
        .fetch_all(&stores.postgres)
        .await?
        .iter()
        .map(|row| {
            json!({
                "cohort_id": row.get::<String,_>("cohort_id"),
                "algorithm_version": row.get::<String,_>("algorithm_version"),
                "member_count": row.get::<i32,_>("member_count"),
                "score": row.get::<f64,_>("score"),
                "first_slot": row.get::<i64,_>("first_slot").to_string(),
                "last_slot": row.get::<i64,_>("last_slot").to_string(),
                "evidence_signatures": capped_array(row.get::<Value,_>("evidence_signatures"), MAX_SIGNATURES),
                "attributes": bounded_json(row.get::<Value,_>("attributes"), 3)
            })
        })
        .collect::<Vec<_>>()
    };
    let wallet_node = (!creator.is_empty()).then(|| format!("wallet:{creator}"));
    let derived_edges = if let Some(wallet_node) = wallet_node.as_deref() {
        sqlx::query(
            r#"SELECT edge_id,source_node,target_node,edge_type,score,evidence_count,
                      first_observed_slot,last_observed_slot,algorithm_version,supporting_signatures
               FROM pumphound.derived_edges
               WHERE (source_node=$1 OR target_node=$1) AND last_observed_slot <= $2
               ORDER BY last_observed_slot DESC LIMIT $3"#,
        )
        .bind(wallet_node)
        .bind(slot)
        .bind(MAX_DERIVED_EDGES)
        .fetch_all(&stores.postgres)
        .await?
        .iter()
        .map(|row| {
            json!({
                "edge_id": row.get::<String,_>("edge_id"),
                "source_node": row.get::<String,_>("source_node"),
                "target_node": row.get::<String,_>("target_node"),
                "edge_type": row.get::<String,_>("edge_type"),
                "score": row.get::<f64,_>("score"),
                "evidence_count": row.get::<i32,_>("evidence_count"),
                "first_observed_slot": row.get::<i64,_>("first_observed_slot").to_string(),
                "last_observed_slot": row.get::<i64,_>("last_observed_slot").to_string(),
                "algorithm_version": row.get::<String,_>("algorithm_version"),
                "supporting_signatures": capped_array(row.get::<Value,_>("supporting_signatures"), MAX_SIGNATURES)
            })
        })
        .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let graph_facts = if mint.is_empty() && wallet_node.is_none() {
        vec![]
    } else {
        sqlx::query(
            r#"SELECT fact_id,event_id,source_node,target_node,edge_type,slot,known_at,
                      asset_mint,amount_atomic::text,attributes
               FROM pumphound.graph_facts
               WHERE slot <= $1 AND known_at <= $2
                 AND (asset_mint=$3 OR source_node=$4 OR target_node=$4)
               ORDER BY slot DESC,known_at DESC LIMIT $5"#,
        )
        .bind(slot)
        .bind(cutoff)
        .bind(mint)
        .bind(wallet_node.as_deref().unwrap_or(""))
        .bind(MAX_GRAPH_FACTS)
        .fetch_all(&stores.postgres)
        .await?
        .iter()
        .map(|row| {
            json!({
                "fact_id": row.get::<String,_>("fact_id"),
                "event_id": row.get::<String,_>("event_id"),
                "source_node": row.get::<String,_>("source_node"),
                "target_node": row.get::<String,_>("target_node"),
                "edge_type": row.get::<String,_>("edge_type"),
                "slot": row.get::<i64,_>("slot").to_string(),
                "known_at": row.get::<DateTime<Utc>,_>("known_at"),
                "asset_mint": row.try_get::<Option<String>,_>("asset_mint").ok().flatten(),
                "amount_atomic": row.try_get::<Option<String>,_>("amount_atomic").ok().flatten(),
                "attributes": bounded_json(row.get::<Value,_>("attributes"), 3)
            })
        })
        .collect::<Vec<_>>()
    };
    let transaction_evidence_ids = graph_facts
        .iter()
        .filter_map(|fact| fact.get("event_id").and_then(Value::as_str))
        .take(MAX_GRAPH_FACTS as usize)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let holder_metrics = launch
        .as_ref()
        .and_then(|value| value.get("attributes"))
        .and_then(|attributes| attributes.get("holder_metrics"))
        .cloned()
        .map(|value| bounded_json(value, 4))
        .unwrap_or_else(|| unavailable("holder_metrics_not_stored"));
    let mut evidence = json!({
        "schema_version": "investigator-evidence-v1",
        "policy_effect": false,
        "subject": {"type": subject_type, "id": subject_id},
        "as_of_slot": slot.to_string(),
        "knowledge_cutoff": cutoff,
        "decision_context": seed.unwrap_or_else(|| unavailable("manual_request_no_decision")),
        "launch": launch.unwrap_or_else(|| unavailable("launch_not_resolved")),
        "creator_profile": bounded_json(creator_profile, 4),
        "holder_metrics": holder_metrics,
        "wallet_profile": bounded_json(wallet_profile, 4),
        "wallet_reputation": reputation,
        "funding_edges": funding_edges,
        "cohorts": cohorts,
        "derived_edges": derived_edges,
        "graph_facts": graph_facts,
        "transaction_evidence_ids": transaction_evidence_ids,
        "limits": {
            "graph_facts": MAX_GRAPH_FACTS,
            "funding_edges": MAX_FUNDING_EDGES,
            "cohorts": MAX_COHORTS,
            "derived_edges": MAX_DERIVED_EDGES,
            "signatures_per_field": MAX_SIGNATURES,
            "explanation_items": MAX_EXPLANATION_ITEMS
        }
    });
    enforce_evidence_limit(&mut evidence);
    redact_secrets(&mut evidence);
    Ok(evidence)
}

async fn resolve_launch(
    stores: &Stores,
    subject_type: &str,
    subject_id: &str,
) -> anyhow::Result<Option<Value>> {
    let row = match subject_type {
        "launch" | "token" | "mint" => {
            sqlx::query(
                r#"SELECT launch_id::text,mint,creator,protocol,creation_slot,creation_time,
                          known_at,canonicality,attributes
                   FROM pumphound.launches
                   WHERE launch_id::text=$1 OR mint=$1 LIMIT 1"#,
            )
            .bind(subject_id)
            .fetch_optional(&stores.postgres)
            .await?
        }
        "creator" | "wallet" => {
            sqlx::query(
                r#"SELECT launch_id::text,mint,creator,protocol,creation_slot,creation_time,
                          known_at,canonicality,attributes
                   FROM pumphound.launches WHERE creator=$1
                   ORDER BY creation_slot DESC LIMIT 1"#,
            )
            .bind(subject_id)
            .fetch_optional(&stores.postgres)
            .await?
        }
        _ => None,
    };
    Ok(row.map(|row| {
        json!({
            "launch_id": row.get::<String,_>("launch_id"),
            "mint": row.get::<String,_>("mint"),
            "creator": row.get::<String,_>("creator"),
            "protocol": row.get::<String,_>("protocol"),
            "creation_slot": row.get::<i64,_>("creation_slot").to_string(),
            "creation_time": row.try_get::<Option<DateTime<Utc>>,_>("creation_time").ok().flatten(),
            "known_at": row.get::<DateTime<Utc>,_>("known_at"),
            "canonicality": row.get::<String,_>("canonicality"),
            "attributes": bounded_json(row.get::<Value,_>("attributes"), 4)
        })
    }))
}

async fn optional_json_row(stores: &Stores, query: &str, subject: &str) -> anyhow::Result<Value> {
    let row = sqlx::query(query)
        .bind(subject)
        .fetch_optional(&stores.postgres)
        .await?;
    Ok(row
        .map(|row| row.get::<Value, _>("value"))
        .unwrap_or_else(|| unavailable("not_stored")))
}

#[allow(clippy::too_many_arguments)]
async fn persist_row(
    stores: &Stores,
    context: &InvestigationContext,
    evidence_hash: &str,
    provider: &str,
    model_id: &str,
    prompt_version: &str,
    response: Value,
    validation: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.investigations
           (investigation_id,subject_type,subject_id,as_of_slot,evidence_pack_sha256,
            provider,model_id,prompt_version,role,response,validation,created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'committee_bundle',$9,$10,now())
           ON CONFLICT (investigation_id) DO NOTHING"#,
    )
    .bind(context.investigation_id)
    .bind(&context.subject_type)
    .bind(&context.subject_id)
    .bind(context.as_of_slot)
    .bind(evidence_hash)
    .bind(provider)
    .bind(model_id)
    .bind(prompt_version)
    .bind(response)
    .bind(validation)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

fn frozen_aggregate(exact_inputs: &Value) -> Value {
    let Some(aggregate) = exact_inputs.get("aggregate_prediction") else {
        return unavailable("aggregate_prediction_not_stored");
    };
    json!({
        "status": aggregate.get("status").cloned().unwrap_or(Value::Null),
        "prediction_id": aggregate.get("prediction_id").cloned().unwrap_or(Value::Null),
        "feature_snapshot_id": aggregate.get("feature_snapshot_id").cloned().unwrap_or(Value::Null),
        "probabilities": aggregate.get("probabilities").cloned().unwrap_or_else(|| unavailable("not_stored")),
        "target_model_versions": aggregate.get("target_model_versions").cloned().unwrap_or_else(|| unavailable("not_stored")),
        "familiarity": aggregate.get("familiarity").cloned().unwrap_or_else(|| unavailable("not_stored")),
        "withheld_reasons": aggregate.get("withheld_reasons").cloned().unwrap_or_else(|| json!({})),
        "explanations": aggregate.get("explanations")
            .cloned()
            .map(|value| bounded_json(value, 4))
            .unwrap_or_else(|| unavailable("not_stored"))
    })
}

fn role_metadata(roles: &Value) -> (String, String) {
    let mut models = BTreeSet::new();
    let mut prompts = BTreeSet::new();
    if let Some(roles) = roles.as_object() {
        for value in roles.values() {
            if let Some(model) = value.get("model_id").and_then(Value::as_str) {
                models.insert(model.to_string());
            }
            if let Some(prompt) = value.get("prompt_version").and_then(Value::as_str) {
                prompts.insert(prompt.to_string());
            }
        }
    }
    (
        joined_or_unavailable(models),
        joined_or_unavailable(prompts),
    )
}

fn role_validations(roles: &Value) -> Value {
    Value::Object(
        roles
            .as_object()
            .into_iter()
            .flat_map(|roles| roles.iter())
            .filter_map(|(role, value)| {
                value
                    .get("validation")
                    .cloned()
                    .map(|validation| (role.clone(), validation))
            })
            .collect(),
    )
}

fn joined_or_unavailable(values: BTreeSet<String>) -> String {
    if values.is_empty() {
        "unavailable".into()
    } else {
        values.into_iter().collect::<Vec<_>>().join(",")
    }
}

fn normalized_roles(roles: Vec<String>) -> Vec<String> {
    let allowed = [
        "quant",
        "wallet",
        "creator",
        "risk",
        "contrarian",
        "committee",
    ];
    let roles = roles
        .into_iter()
        .filter(|role| allowed.contains(&role.as_str()))
        .take(6)
        .collect::<Vec<_>>();
    if roles.is_empty() {
        vec!["committee".into()]
    } else {
        roles
    }
}

fn nested_policy_effect(value: &Value) -> bool {
    value
        .get("policy_effect")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || value
            .pointer("/validation/affects_policy")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn unavailable(reason: &str) -> Value {
    json!({"status": "UNAVAILABLE", "reason": reason})
}

fn capped_array(value: Value, limit: usize) -> Value {
    match value {
        Value::Array(mut values) => {
            let original = values.len();
            values.truncate(limit);
            json!({
                "items": values.into_iter().map(|value| bounded_json(value, 2)).collect::<Vec<_>>(),
                "truncated": original > limit,
                "original_count": original
            })
        }
        other => bounded_json(other, 2),
    }
}

fn bounded_json(value: Value, depth: usize) -> Value {
    if depth == 0 {
        return json!({"status": "TRUNCATED", "reason": "depth_limit"});
    }
    match value {
        Value::String(mut string) => {
            if string.len() > MAX_STRING_BYTES {
                string.truncate(MAX_STRING_BYTES);
                json!({"value": string, "truncated": true})
            } else {
                Value::String(string)
            }
        }
        Value::Array(values) => {
            let original = values.len();
            let values = values
                .into_iter()
                .take(MAX_EXPLANATION_ITEMS)
                .map(|value| bounded_json(value, depth - 1))
                .collect::<Vec<_>>();
            if original > MAX_EXPLANATION_ITEMS {
                json!({"items": values, "truncated": true, "original_count": original})
            } else {
                Value::Array(values)
            }
        }
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .take(100)
                .map(|(key, value)| (key, bounded_json(value, depth - 1)))
                .collect(),
        ),
        other => other,
    }
}

fn redact_secrets(value: &mut Value) {
    match value {
        Value::Object(values) => {
            values.retain(|key, _| {
                let key = key.to_ascii_lowercase();
                ![
                    "password",
                    "secret",
                    "api_key",
                    "authorization",
                    "credential",
                    "private_key",
                ]
                .iter()
                .any(|blocked| key.contains(blocked))
            });
            for value in values.values_mut() {
                redact_secrets(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_secrets(value);
            }
        }
        _ => {}
    }
}

fn enforce_evidence_limit(evidence: &mut Value) {
    let original_bytes = serde_json::to_vec(evidence)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    if original_bytes <= MAX_EVIDENCE_BYTES {
        evidence["evidence_size"] = json!({
            "bytes": original_bytes,
            "maximum_bytes": MAX_EVIDENCE_BYTES,
            "truncated": false
        });
        return;
    }
    for (field, limit) in [
        ("graph_facts", 25usize),
        ("derived_edges", 15),
        ("funding_edges", 10),
        ("cohorts", 10),
    ] {
        if let Some(values) = evidence.get_mut(field).and_then(Value::as_array_mut) {
            values.truncate(limit);
        }
    }
    let reduced_bytes = serde_json::to_vec(evidence)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    if reduced_bytes > MAX_EVIDENCE_BYTES {
        evidence["graph_facts"] = unavailable("omitted_by_evidence_byte_cap");
        evidence["derived_edges"] = unavailable("omitted_by_evidence_byte_cap");
    }
    if serde_json::to_vec(evidence)
        .map(|bytes| bytes.len())
        .unwrap_or_default()
        > MAX_EVIDENCE_BYTES
    {
        if let Some(explanations) =
            evidence.pointer_mut("/decision_context/decision/aggregate_model/explanations")
        {
            *explanations = unavailable("omitted_by_evidence_byte_cap");
        }
    }
    let final_bytes = serde_json::to_vec(evidence)
        .map(|bytes| bytes.len())
        .unwrap_or_default();
    evidence["evidence_size"] = json!({
        "bytes": final_bytes,
        "original_bytes": original_bytes,
        "maximum_bytes": MAX_EVIDENCE_BYTES,
        "truncated": true
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn request() -> InvestigateRequest {
        InvestigateRequest {
            subject_type: "launch".into(),
            subject_id: "launch-1".into(),
            as_of_slot: 7,
            knowledge_cutoff: Utc::now(),
            roles: vec!["committee".into()],
            evidence: json!({"decision_id": Uuid::nil(), "policy_effect": false}),
            timeout_s: 1.0,
        }
    }

    async fn mock_client(policy_effect: bool) -> (InvestigatorClient, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/investigate",
                post(
                    move |State(calls): State<Arc<AtomicUsize>>,
                          Json(_request): Json<Value>| async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Json(json!({
                            "job_id": "job-1",
                            "evidence_pack_sha256": "a".repeat(64),
                            "provider": "mock",
                            "roles": {"committee": {"model_id": "mock-model", "prompt_version": "v1", "policy_effect": policy_effect}},
                            "policy_effect": policy_effect
                        }))
                    },
                ),
            )
            .with_state(calls.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            InvestigatorClient::with_url(format!("http://{address}/investigate")),
            calls,
        )
    }

    #[tokio::test]
    async fn calls_only_for_buy_and_watch() {
        let (client, calls) = mock_client(false).await;
        assert!(call_if_advisory("BUY", &client, &request())
            .await
            .unwrap()
            .is_ok());
        assert!(call_if_advisory("WATCH", &client, &request())
            .await
            .unwrap()
            .is_ok());
        assert!(call_if_advisory("AVOID", &client, &request())
            .await
            .is_none());
        assert!(call_if_advisory("ABSTAIN", &client, &request())
            .await
            .is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn policy_effect_true_is_rejected() {
        let (client, _) = mock_client(true).await;
        assert!(matches!(
            client.investigate(&request()).await,
            Err(InvestigatorError::PolicyEffectRejected)
        ));
    }

    #[tokio::test]
    async fn investigator_failure_cannot_mutate_decision() {
        let decision = json!({"decision_id": Uuid::nil(), "action": "BUY", "persisted": true});
        let before = decision.clone();
        let client = InvestigatorClient::with_url("http://127.0.0.1:9/investigate");
        assert!(client.investigate(&request()).await.is_err());
        assert_eq!(decision, before);
    }
}
