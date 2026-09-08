//! M11 intelligence reads + funding-queue consumer.
//!
//! Funding adapter is intentionally conservative: we never invent a funder.
//! Helius Enhanced Transactions is not treated as a funded-by parser here.
//! If `PUMPHOUND_HELIUS_API_KEY` is missing or the entitled funded-by adapter
//! is not implemented, queue rows are cached as `status=unknown`.

use crate::state::AppState;
use pumphound_domain::now_utc;
use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;
use tracing::{info, warn};

const FUNDING_LOOKUP_PRIORITY: i32 = 70;
const FUNDING_BATCH: i64 = 3;
const GRAPH_EDGE_CAP: usize = 400;

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Err(e) = drain_funding_once(&state.stores).await {
                warn!(error = %e, "funding queue");
            }
        }
    });
}

/// Presence only. Never returns or logs the key value.
pub fn helius_key_present() -> bool {
    std::env::var("PUMPHOUND_HELIUS_API_KEY")
        .ok()
        .is_some_and(|s| !s.is_empty())
}

async fn drain_funding_once(stores: &Stores) -> anyhow::Result<()> {
    let wallets = stores.claim_funding_batch(FUNDING_BATCH).await?;
    if wallets.is_empty() {
        return Ok(());
    }
    // Do not call Enhanced Transactions and guess inbound transfers.
    // Cache the unknown result so queued wallets are not retried every tick.
    let last_error = if helius_key_present() {
        "funding adapter not entitled"
    } else {
        "no helius key"
    };
    for wallet in wallets {
        stores
            .mark_funding_status(&wallet, "unknown", Some(last_error))
            .await?;
        info!(%wallet, status = "unknown", last_error, "funding crawl cached");
    }
    Ok(())
}

pub async fn wallet_profile_json(stores: &Stores, address: &str) -> Option<Value> {
    let row = sqlx::query(
        r#"SELECT wallet, first_seen_slot, last_seen_slot, launches_entered,
                  first10_appearances, buys, sells, creator_linked_launches,
                  cohort_ids, behaviour, updated_at, attributes
           FROM pumphound.wallet_profiles WHERE wallet=$1"#,
    )
    .bind(address)
    .fetch_optional(&stores.postgres)
    .await
    .ok()
    .flatten()?;
    Some(json!({
        "wallet": row.get::<String,_>("wallet"),
        "first_seen_slot": row.get::<i64,_>("first_seen_slot").to_string(),
        "last_seen_slot": row.get::<i64,_>("last_seen_slot").to_string(),
        "launches_entered": row.get::<i32,_>("launches_entered"),
        "first10_appearances": row.get::<i32,_>("first10_appearances"),
        "buys": row.get::<i32,_>("buys"),
        "sells": row.get::<i32,_>("sells"),
        "creator_linked_launches": row.get::<i32,_>("creator_linked_launches"),
        "cohort_ids": row.try_get::<Value,_>("cohort_ids").unwrap_or(json!([])),
        "behaviour": row.try_get::<Option<String>,_>("behaviour").ok().flatten(),
        "pnl_status": "INCOMPLETE",
        "realized_return": null,
        "updated_at": row.try_get::<chrono::DateTime<chrono::Utc>,_>("updated_at").ok(),
        "attributes": row.try_get::<Value,_>("attributes").unwrap_or(json!({})),
    }))
}

pub async fn wallet_cohort_ids(stores: &Stores, address: &str) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT cohort_id FROM pumphound.cohort_members WHERE wallet=$1 ORDER BY cohort_id",
    )
    .bind(address)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default()
}

pub async fn enqueue_funding_lookup(stores: &Stores, address: &str) {
    if let Err(e) = stores
        .enqueue_funding_if_absent(address, FUNDING_LOOKUP_PRIORITY, "api_lookup")
        .await
    {
        warn!(error = %e, wallet = %address, "funding enqueue");
    }
}

pub async fn funding_response(stores: &Stores, address: &str) -> Value {
    let edges = sqlx::query(
        r#"SELECT edge_id, wallet, funder, source, score, evidence, algorithm_version, known_at
           FROM pumphound.funding_edges
           WHERE wallet=$1
           ORDER BY known_at DESC
           LIMIT 50"#,
    )
    .bind(address)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();

    let queue = sqlx::query(
        "SELECT status, priority, reason, last_error, attempts, updated_at
         FROM pumphound.funding_queue WHERE wallet=$1",
    )
    .bind(address)
    .fetch_optional(&stores.postgres)
    .await
    .ok()
    .flatten();

    if edges.is_empty() {
        enqueue_funding_lookup(stores, address).await;
        let queue = sqlx::query(
            "SELECT status, priority, reason, last_error, attempts, updated_at
             FROM pumphound.funding_queue WHERE wallet=$1",
        )
        .bind(address)
        .fetch_optional(&stores.postgres)
        .await
        .ok()
        .flatten()
        .or(queue);
        let status = queue
            .as_ref()
            .map(|r| r.get::<String, _>("status"))
            .unwrap_or_else(|| "queued".into());
        let reason = if status == "unknown" || status == "failed" {
            "unknown"
        } else {
            "funding crawl not started"
        };
        return json!({
            "wallet": address,
            "paths": [],
            "truncated": true,
            "reason": reason,
            "queue": queue.as_ref().map(|r| json!({
                "status": r.get::<String,_>("status"),
                "priority": r.get::<i32,_>("priority"),
                "reason": r.get::<String,_>("reason"),
                "last_error": r.try_get::<Option<String>,_>("last_error").ok().flatten(),
                "attempts": r.get::<i32,_>("attempts"),
                "updated_at": r.try_get::<chrono::DateTime<chrono::Utc>,_>("updated_at").ok(),
            })),
        });
    }

    let paths: Vec<Value> = edges
        .into_iter()
        .map(|r| {
            json!({
                "edge_id": r.get::<String,_>("edge_id"),
                "wallet": r.get::<String,_>("wallet"),
                "funder": r.try_get::<Option<String>,_>("funder").ok().flatten(),
                "source": r.get::<String,_>("source"),
                "score": r.try_get::<Option<f64>,_>("score").ok().flatten(),
                "evidence": r.try_get::<Value,_>("evidence").unwrap_or(json!([])),
                "algorithm_version": r.get::<String,_>("algorithm_version"),
                "known_at": r.try_get::<chrono::DateTime<chrono::Utc>,_>("known_at").ok(),
                "inferred": false,
                "note": "observed funding edge; not ownership"
            })
        })
        .collect();
    json!({
        "wallet": address,
        "paths": paths,
        "truncated": false,
        "reason": null,
        "queue": queue.as_ref().map(|r| json!({
            "status": r.get::<String,_>("status"),
            "priority": r.get::<i32,_>("priority"),
            "last_error": r.try_get::<Option<String>,_>("last_error").ok().flatten(),
        })),
    })
}

pub async fn creator_profile_json(stores: &Stores, address: &str) -> Option<Value> {
    let row = sqlx::query(
        r#"SELECT creator, launches, first_seen_slot, last_seen_slot, connected_buyers,
                  updated_at, attributes
           FROM pumphound.creator_profiles WHERE creator=$1"#,
    )
    .bind(address)
    .fetch_optional(&stores.postgres)
    .await
    .ok()
    .flatten()?;
    Some(json!({
        "creator": row.get::<String,_>("creator"),
        "launches": row.get::<i32,_>("launches"),
        "first_seen_slot": row.try_get::<Option<i64>,_>("first_seen_slot").ok().flatten().map(|s| s.to_string()),
        "last_seen_slot": row.try_get::<Option<i64>,_>("last_seen_slot").ok().flatten().map(|s| s.to_string()),
        "connected_buyers": row.get::<i32,_>("connected_buyers"),
        "pnl_status": "INCOMPLETE",
        "realized_return": null,
        "graduation_rate": null,
        "collapse_rate": null,
        "updated_at": row.try_get::<chrono::DateTime<chrono::Utc>,_>("updated_at").ok(),
        "attributes": row.try_get::<Value,_>("attributes").unwrap_or(json!({})),
    }))
}

pub async fn connected_buyers(stores: &Stores, creator: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(DISTINCT f.source_node)
           FROM pumphound.launches l
           JOIN pumphound.graph_facts f ON f.asset_mint = l.mint AND f.edge_type='BOUGHT'
           WHERE l.creator=$1"#,
    )
    .bind(creator)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0)
}

/// Observed migrate/complete share only. Zero outcome rows → null (not 0%).
pub async fn honest_graduation_rate(stores: &Stores, creator: &str) -> Option<f64> {
    let row = sqlx::query(
        r#"SELECT
             (SELECT COUNT(*) FROM pumphound.launches WHERE creator=$1)::bigint AS launches,
             (
               SELECT COUNT(DISTINCT l.launch_id)::bigint
               FROM pumphound.launches l
               WHERE l.creator=$1 AND (
                 EXISTS (
                   SELECT 1 FROM pumphound.markets m
                   WHERE m.launch_id = l.launch_id AND m.migrated_from IS NOT NULL
                 )
                 OR EXISTS (
                   SELECT 1 FROM pumphound.graph_facts f
                   WHERE f.asset_mint = l.mint
                     AND f.edge_type IN ('MIGRATED_TO','COMPLETED')
                 )
               )
             ) AS graduated"#,
    )
    .bind(creator)
    .fetch_optional(&stores.postgres)
    .await
    .ok()
    .flatten()?;
    let launches: i64 = row.try_get("launches").unwrap_or(0);
    let graduated: i64 = row.try_get("graduated").unwrap_or(0);
    if graduated <= 0 || launches <= 0 {
        return None;
    }
    Some(graduated as f64 / launches as f64)
}

pub async fn derived_edges_touching(
    stores: &Stores,
    nodes: &[String],
    remaining: usize,
) -> (Vec<Value>, Vec<String>, u64) {
    if nodes.is_empty() {
        return (Vec::new(), Vec::new(), 0);
    }
    if remaining == 0 {
        let hidden = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*) FROM pumphound.derived_edges
               WHERE edge_type = 'CO_BOUGHT_WITH'
                 AND (source_node = ANY($1) OR target_node = ANY($1))"#,
        )
        .bind(nodes)
        .fetch_one(&stores.postgres)
        .await
        .unwrap_or(0);
        return (Vec::new(), Vec::new(), hidden.max(0) as u64);
    }
    let rows = sqlx::query(
        r#"SELECT source_node, target_node, edge_type, score, evidence_count,
                  algorithm_version, supporting_signatures
           FROM pumphound.derived_edges
           WHERE edge_type = 'CO_BOUGHT_WITH'
             AND (source_node = ANY($1) OR target_node = ANY($1))
           ORDER BY evidence_count DESC, score DESC
           LIMIT 400"#,
    )
    .bind(nodes)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    let hidden = rows.len().saturating_sub(remaining) as u64;
    let mut extra_nodes = Vec::new();
    let mut edges = Vec::new();
    for r in rows.into_iter().take(remaining) {
        let src: String = r.get("source_node");
        let tgt: String = r.get("target_node");
        if !nodes.iter().any(|n| n == &src) {
            extra_nodes.push(src.clone());
        }
        if !nodes.iter().any(|n| n == &tgt) {
            extra_nodes.push(tgt.clone());
        }
        edges.push(json!({
            "source": src,
            "target": tgt,
            "kind": r.get::<String,_>("edge_type"),
            "weight": r.try_get::<f64,_>("score").unwrap_or(0.0),
            "inferred": true,
            "algorithm_version": r.get::<String,_>("algorithm_version"),
            "evidence_count": r.try_get::<i32,_>("evidence_count").unwrap_or(0),
            "supporting_signatures": r.try_get::<Value,_>("supporting_signatures").unwrap_or(json!([])),
            "note": "derived CO_BOUGHT_WITH; not ownership"
        }));
    }
    (edges, extra_nodes, hidden)
}

pub fn graph_edge_cap() -> usize {
    GRAPH_EDGE_CAP
}

pub async fn graph_fact_count(stores: &Stores, mint: &str) -> i64 {
    let token = format!("token:{mint}");
    sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM pumphound.graph_facts
           WHERE asset_mint=$1 OR source_node=$2 OR target_node=$2"#,
    )
    .bind(mint)
    .bind(&token)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0)
}

pub async fn derived_edge_count(stores: &Stores, mint: &str) -> i64 {
    let token = format!("token:{mint}");
    sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM pumphound.derived_edges
           WHERE source_node=$1 OR target_node=$1
              OR source_node IN (
                   SELECT source_node FROM pumphound.graph_facts WHERE asset_mint=$2
                 )
              OR target_node IN (
                   SELECT source_node FROM pumphound.graph_facts WHERE asset_mint=$2
                 )"#,
    )
    .bind(&token)
    .bind(mint)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0)
}

pub async fn cluster_response(stores: &Stores, id: &str) -> Value {
    let row = sqlx::query(
        r#"SELECT cohort_id, algorithm_version, member_count, score,
                  first_slot, last_slot, evidence_signatures, attributes
           FROM pumphound.wallet_cohorts WHERE cohort_id=$1"#,
    )
    .bind(id)
    .fetch_optional(&stores.postgres)
    .await
    .ok()
    .flatten();
    let members: Vec<String> = sqlx::query_scalar(
        "SELECT wallet FROM pumphound.cohort_members WHERE cohort_id=$1 ORDER BY wallet",
    )
    .bind(id)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    json!({
        "cluster_id": id,
        "algorithm_version": row.as_ref().and_then(|r| r.try_get::<String,_>("algorithm_version").ok()),
        "member_count": row.as_ref().and_then(|r| r.try_get::<i32,_>("member_count").ok()),
        "score": row.as_ref().and_then(|r| r.try_get::<f64,_>("score").ok()),
        "first_slot": row.as_ref().and_then(|r| r.try_get::<i64,_>("first_slot").ok()).map(|s| s.to_string()),
        "last_slot": row.as_ref().and_then(|r| r.try_get::<i64,_>("last_slot").ok()).map(|s| s.to_string()),
        "evidence_signatures": row.as_ref().and_then(|r| r.try_get::<Value,_>("evidence_signatures").ok()).unwrap_or(json!([])),
        "attributes": row.as_ref().and_then(|r| r.try_get::<Value,_>("attributes").ok()).unwrap_or(json!({})),
        "members": members,
        "lineage": [],
        "note": "behavioural cohort; not ownership",
        "as_of": now_utc()
    })
}
