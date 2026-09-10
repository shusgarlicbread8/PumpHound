//! M11 intelligence reads + funding-queue consumer.
//!
//! Funding adapter is intentionally conservative: we never invent a funder. No paid API
//! entitlement needed — the same public `getSignaturesForAddress`/`getTransaction` RPC pair the
//! rest of this codebase already uses (see `chain_source::backfill`) is enough, because the
//! wallets worth resolving here (early buyers of a brand-new launch) are overwhelmingly
//! single-purpose burner wallets with only a handful of transactions ever. `resolve_funder` only
//! calls a wallet's earliest visible transaction "the funding event" when the page of signatures
//! came back under the RPC page limit — i.e. we can see the *entire* history, not just the most
//! recent slice of a long-lived wallet — and only names a funder when that transaction shows the
//! classic pattern of the fee payer sending lamports directly to this wallet. Anything else
//! (an established wallet with more history than one page holds, a multi-party transaction, no
//! net inflow) is left `unknown` rather than guessed at.
//!
//! Rate: `FUNDING_BATCH` wallets drained per `spawn`'s 30s tick, two RPC calls per wallet
//! (one signature list, one transaction fetch) — a firmly bounded, low-frequency background
//! crawl, not a bulk backfill. Keep it that way; see `chain_source::backfill`'s doc comment for
//! what happens when a "just fetch it" RPC loop isn't kept on a tight leash.

use crate::state::AppState;
use pumphound_domain::now_utc;
use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tracing::{info, warn};

struct FundingClusterCacheEntry {
    computed_at: Instant,
    slot: i64,
    value: Value,
}

fn funding_cluster_cache() -> &'static Mutex<HashMap<String, FundingClusterCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, FundingClusterCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

const FUNDING_LOOKUP_PRIORITY: i32 = 70;
const FUNDING_BATCH: i64 = 3;
const GRAPH_EDGE_CAP: usize = 400;
const FUNDING_ALGORITHM_VERSION: &str = "onchain_earliest_transfer_v1";
/// One `getSignaturesForAddress` page. Not paginated further — a wallet whose history doesn't
/// fit in one page is not the fresh burner this heuristic is built for, and paging back through
/// an established wallet's full history for every early buyer is exactly the unbounded-RPC
/// pattern `chain_source::backfill` was built (and re-tuned, twice) to avoid.
const FUNDING_SIGNATURE_PAGE_LIMIT: usize = 1000;

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Err(e) = drain_funding_once(&state.stores, &client).await {
                warn!(error = %e, "funding queue");
            }
        }
    });
}

fn funding_rpc_url() -> String {
    pumphound_chain_source::SourceConfig::from_env().rpc_url
}

enum FunderResolution {
    Resolved {
        funder: String,
        amount_lamports: i64,
        signature: String,
    },
    Unresolved {
        reason: &'static str,
    },
    TransportError {
        reason: String,
    },
}

/// Pure: does this earliest-visible transaction look like "the fee payer funded this wallet"?
/// Only the single-funder direct-transfer shape is recognized; anything ambiguous is `None`
/// rather than a guess. `wallet_index` is this wallet's position in `account_keys` (its balance
/// is what we're checking rose); index 0 is always the fee payer in a Solana transaction.
fn derive_funder(
    account_keys: &[String],
    pre_balances: &[i64],
    post_balances: &[i64],
    wallet_index: usize,
) -> Option<(String, i64)> {
    if wallet_index == 0 || wallet_index >= account_keys.len() {
        return None;
    }
    if pre_balances.len() != account_keys.len() || post_balances.len() != account_keys.len() {
        return None;
    }
    let wallet_delta = post_balances[wallet_index] - pre_balances[wallet_index];
    if wallet_delta <= 0 {
        return None;
    }
    let payer_delta = post_balances[0] - pre_balances[0];
    // The payer must have paid out at least what the wallet received; the gap is the tx fee
    // (and any other outputs), which is why this is `>=` the inflow rather than an exact match.
    if payer_delta >= 0 || payer_delta.unsigned_abs() < wallet_delta as u64 {
        return None;
    }
    Some((account_keys[0].clone(), wallet_delta))
}

async fn resolve_funder(client: &reqwest::Client, wallet: &str) -> FunderResolution {
    let rpc = funding_rpc_url();
    let sig_body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
        "params": [wallet, {"limit": FUNDING_SIGNATURE_PAGE_LIMIT}]
    });
    let sig_response = match client.post(&rpc).json(&sig_body).send().await {
        Ok(r) => r,
        Err(e) => return FunderResolution::TransportError { reason: format!("signatures_rpc:{e}") },
    };
    let sig_value: Value = match sig_response.json().await {
        Ok(v) => v,
        Err(e) => return FunderResolution::TransportError { reason: format!("signatures_decode:{e}") },
    };
    let Some(results) = sig_value["result"].as_array() else {
        return FunderResolution::Unresolved { reason: "signatures_unavailable" };
    };
    if results.is_empty() {
        return FunderResolution::Unresolved { reason: "no_signature_history" };
    }
    if results.len() >= FUNDING_SIGNATURE_PAGE_LIMIT {
        // Full page: this wallet has more history than we're willing to page through. The
        // oldest entry here is not its genesis, so it is not safe to call it "the funder."
        return FunderResolution::Unresolved { reason: "history_exceeds_page_limit" };
    }
    let Some(earliest_signature) = results.last().and_then(|e| e["signature"].as_str()) else {
        return FunderResolution::Unresolved { reason: "earliest_signature_missing" };
    };
    let tx_body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "getTransaction",
        "params": [earliest_signature, {"encoding": "json", "maxSupportedTransactionVersion": 0}]
    });
    let tx_response = match client.post(&rpc).json(&tx_body).send().await {
        Ok(r) => r,
        Err(e) => return FunderResolution::TransportError { reason: format!("transaction_rpc:{e}") },
    };
    let tx_value: Value = match tx_response.json().await {
        Ok(v) => v,
        Err(e) => return FunderResolution::TransportError { reason: format!("transaction_decode:{e}") },
    };
    let Some(account_keys) = tx_value["result"]["transaction"]["message"]["accountKeys"].as_array()
    else {
        return FunderResolution::Unresolved { reason: "earliest_transaction_unavailable" };
    };
    let account_keys: Vec<String> = account_keys
        .iter()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();
    let Some(wallet_index) = account_keys.iter().position(|k| k == wallet) else {
        return FunderResolution::Unresolved { reason: "wallet_not_in_account_keys" };
    };
    let parse_balances = |field: &str| -> Option<Vec<i64>> {
        tx_value["result"]["meta"][field]
            .as_array()
            .map(|values| values.iter().filter_map(Value::as_i64).collect())
    };
    let (Some(pre_balances), Some(post_balances)) =
        (parse_balances("preBalances"), parse_balances("postBalances"))
    else {
        return FunderResolution::Unresolved { reason: "balances_unavailable" };
    };
    match derive_funder(&account_keys, &pre_balances, &post_balances, wallet_index) {
        Some((funder, amount_lamports)) => FunderResolution::Resolved {
            funder,
            amount_lamports,
            signature: earliest_signature.to_string(),
        },
        None => FunderResolution::Unresolved {
            reason: "earliest_transaction_not_a_direct_funding_transfer",
        },
    }
}

async fn drain_funding_once(stores: &Stores, client: &reqwest::Client) -> anyhow::Result<()> {
    let wallets = stores.claim_funding_batch(FUNDING_BATCH).await?;
    if wallets.is_empty() {
        return Ok(());
    }
    for wallet in wallets {
        match resolve_funder(client, &wallet).await {
            FunderResolution::Resolved {
                funder,
                amount_lamports,
                signature,
            } => {
                let evidence = json!([{"signature": signature, "amount_lamports": amount_lamports}]);
                stores
                    .record_funding_edge(
                        &wallet,
                        &funder,
                        "rpc_earliest_transaction",
                        1.0,
                        &evidence,
                        FUNDING_ALGORITHM_VERSION,
                    )
                    .await?;
                stores
                    .mark_funding_status(&wallet, "succeeded", None)
                    .await?;
                info!(%wallet, funder = %funder, amount_lamports, "funding resolved");
            }
            FunderResolution::Unresolved { reason } => {
                stores
                    .mark_funding_status(&wallet, "unknown", Some(reason))
                    .await?;
                info!(%wallet, status = "unknown", last_error = reason, "funding crawl cached");
            }
            FunderResolution::TransportError { reason } => {
                stores
                    .mark_funding_status(&wallet, "failed", Some(&reason))
                    .await?;
                warn!(%wallet, error = %reason, "funding lookup failed");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod funding_tests {
    use super::*;

    fn keys(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("wallet{i}")).collect()
    }

    #[test]
    fn direct_transfer_from_fee_payer_is_resolved() {
        let account_keys = keys(2);
        let pre = vec![5_000_000_000, 0];
        let post = vec![4_000_000_000 - 5_000, 1_000_000_000];
        let resolved = derive_funder(&account_keys, &pre, &post, 1);
        assert_eq!(resolved, Some(("wallet0".to_string(), 1_000_000_000)));
    }

    #[test]
    fn wallet_paying_its_own_fee_is_never_treated_as_self_funded() {
        let account_keys = keys(2);
        let pre = vec![1_000_000_000, 500_000_000];
        let post = vec![999_995_000, 500_000_000];
        assert_eq!(derive_funder(&account_keys, &pre, &post, 0), None);
    }

    #[test]
    fn no_net_inflow_is_not_a_funding_event() {
        let account_keys = keys(2);
        let pre = vec![5_000_000_000, 1_000_000_000];
        let post = vec![4_000_000_000, 1_000_000_000];
        assert_eq!(derive_funder(&account_keys, &pre, &post, 1), None);
    }

    #[test]
    fn payer_paying_out_less_than_wallet_received_is_not_the_source() {
        // Some other input funded most of the transfer; account 0 alone doesn't cover it, so
        // don't misattribute the whole amount to it.
        let account_keys = keys(2);
        let pre = vec![100_000, 0];
        let post = vec![95_000, 1_000_000_000];
        assert_eq!(derive_funder(&account_keys, &pre, &post, 1), None);
    }

    #[test]
    fn mismatched_balance_array_lengths_never_panics() {
        let account_keys = keys(3);
        let pre = vec![1_000_000_000, 0];
        let post = vec![999_995_000, 1_000_000_000, 0];
        assert_eq!(derive_funder(&account_keys, &pre, &post, 1), None);
    }
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

const COORDINATION_EARLY_BUYER_DEPTH: i64 = 10;
const COORDINATION_MAX_LAUNCHES: i64 = 50;

/// Repeat-early-buyer detection for one creator: wallets that show up among the first
/// COORDINATION_EARLY_BUYER_DEPTH buyers on two or more of this creator's launches. A stranger
/// happening to snipe two unrelated tokens early is unremarkable; the same small wallet set doing
/// it across most of one creator's launches is the textbook insider/self-funded-volume pattern —
/// the creator (or someone coordinating with them) recycling capital across their own launches to
/// fake early demand before exiting on real buyers. This is descriptive, not an accusation: it
/// surfaces the overlap and its evidence (which launches, which wallets); it does not label
/// anything a "rug".
///
/// Cross-references pumphound.derived_edges (CO_BOUGHT_WITH, computed by
/// Stores::refresh_intelligence) so a repeat-buyer wallet found here can also show whether it
/// co-buys with others *outside* this creator's launches too — i.e. whether it looks like a
/// standing wallet ring rather than something specific to this one creator.
pub async fn creator_coordination(stores: &Stores, creator: &str) -> Value {
    let launches = sqlx::query(
        r#"SELECT launch_id::text, mint FROM pumphound.launches
           WHERE creator=$1
             AND COALESCE(attributes->>'integrity_state','') <> 'QUARANTINED'
           ORDER BY creation_slot DESC LIMIT $2"#,
    )
    .bind(creator)
    .bind(COORDINATION_MAX_LAUNCHES)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    let launch_count = launches.len();
    if launches.is_empty() {
        return json!({
            "creator": creator,
            "launches_examined": 0,
            "early_buyer_depth": COORDINATION_EARLY_BUYER_DEPTH,
            "distinct_early_buyers": 0,
            "repeat_buyers": [],
            "concentration_fraction": null,
            "risk_label": "INSUFFICIENT_EVIDENCE",
            "note": "no launches attributed to this creator",
        });
    }
    let mints: Vec<String> = launches
        .iter()
        .map(|r| r.get::<String, _>("mint"))
        .collect();
    let repeat_rows = sqlx::query(
        r#"WITH ranked AS (
          SELECT gf.asset_mint AS mint, gf.source_node AS wallet, gf.slot,
                 row_number() OVER (PARTITION BY gf.asset_mint ORDER BY gf.slot ASC) AS rn
          FROM pumphound.graph_facts gf
          WHERE gf.edge_type='BOUGHT' AND gf.asset_mint = ANY($1)
        ), early AS (
          SELECT mint, wallet FROM ranked WHERE rn <= $2
        )
        SELECT wallet, count(DISTINCT mint) AS launches_touched,
               array_agg(DISTINCT mint) AS mints
        FROM early
        GROUP BY wallet
        HAVING count(DISTINCT mint) >= 2
        ORDER BY launches_touched DESC, wallet
        LIMIT 25"#,
    )
    .bind(&mints)
    .bind(COORDINATION_EARLY_BUYER_DEPTH)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    // Same ranked/early shape as the repeat-buyers query above (not a correlated subquery
    // re-ranking per outer row, which is what made this take ~8s on a 30-launch creator).
    let distinct_early_buyers: i64 = sqlx::query_scalar(
        r#"WITH ranked AS (
          SELECT gf.asset_mint AS mint, gf.source_node AS wallet,
                 row_number() OVER (PARTITION BY gf.asset_mint ORDER BY gf.slot ASC) AS rn
          FROM pumphound.graph_facts gf
          WHERE gf.edge_type='BOUGHT' AND gf.asset_mint = ANY($1)
        )
        SELECT count(DISTINCT wallet) FROM ranked WHERE rn <= $2"#,
    )
    .bind(&mints)
    .bind(COORDINATION_EARLY_BUYER_DEPTH)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0);
    let wallets: Vec<String> = repeat_rows
        .iter()
        .map(|r| r.get::<String, _>("wallet"))
        .collect();
    let external_ties = if wallets.is_empty() {
        Vec::new()
    } else {
        sqlx::query(
            r#"SELECT source_node, target_node, evidence_count, score
               FROM pumphound.derived_edges
               WHERE edge_type='CO_BOUGHT_WITH' AND (source_node = ANY($1) OR target_node = ANY($1))
               ORDER BY evidence_count DESC LIMIT 50"#,
        )
        .bind(&wallets)
        .fetch_all(&stores.postgres)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| {
            json!({
                "source": r.get::<String, _>("source_node"),
                "target": r.get::<String, _>("target_node"),
                "evidence_count": r.try_get::<i32, _>("evidence_count").unwrap_or(0),
                "score": r.try_get::<f64, _>("score").unwrap_or(0.0),
            })
        })
        .collect::<Vec<_>>()
    };
    let repeat_buyers: Vec<Value> = repeat_rows
        .iter()
        .map(|r| {
            let wallet: String = r.get("wallet");
            let touched: i64 = r.get("launches_touched");
            let mints: Vec<String> = r.try_get("mints").unwrap_or_default();
            json!({
                "wallet": wallet,
                "launches_touched": touched,
                "launches_touched_fraction": touched as f64 / launch_count as f64,
                "mints": mints,
            })
        })
        .collect();
    let concentration_fraction = if distinct_early_buyers > 0 {
        Some(repeat_buyers.len() as f64 / distinct_early_buyers as f64)
    } else {
        None
    };
    let risk_label = match (launch_count, concentration_fraction) {
        (n, _) if n < 3 => "INSUFFICIENT_EVIDENCE",
        (_, None) => "INSUFFICIENT_EVIDENCE",
        (_, Some(f)) if f >= 0.35 => "HIGH_REPEAT_BUYER_CONCENTRATION",
        (_, Some(f)) if f >= 0.15 => "ELEVATED_REPEAT_BUYER_CONCENTRATION",
        _ => "NO_ELEVATED_CONCENTRATION_DETECTED",
    };
    json!({
        "creator": creator,
        "launches_examined": launch_count,
        "early_buyer_depth": COORDINATION_EARLY_BUYER_DEPTH,
        "distinct_early_buyers": distinct_early_buyers,
        "repeat_buyers": repeat_buyers,
        "external_co_buy_ties": external_ties,
        "concentration_fraction": concentration_fraction,
        "risk_label": risk_label,
        "note": "Descriptive early-buyer overlap only, not an accusation. A wallet appearing here bought early into >=2 of this creator's launches; external_co_buy_ties shows the same wallets co-buying with others outside this creator's launches too.",
    })
}

pub const FUNDING_CLUSTER_VERSION: &str = "funding-cluster-v1";

/// Observed first-10 buyer structure. Funders stay UNKNOWN unless `funding_edges` has a row.
/// Never encodes same_funder as a rug.
pub async fn funding_cluster_for_mint(
    stores: &Stores,
    mint: &str,
    creator: &str,
    knowledge_cutoff_slot: i64,
) -> Value {
    if let Ok(cache) = funding_cluster_cache().lock() {
        if let Some(entry) = cache.get(mint) {
            if entry.computed_at.elapsed() < Duration::from_secs(20)
                && entry.slot >= knowledge_cutoff_slot.saturating_sub(32)
            {
                return entry.value.clone();
            }
        }
    }
    let value = compute_funding_cluster(stores, mint, creator, knowledge_cutoff_slot).await;
    if let Ok(mut cache) = funding_cluster_cache().lock() {
        cache.insert(
            mint.to_string(),
            FundingClusterCacheEntry {
                computed_at: Instant::now(),
                slot: knowledge_cutoff_slot,
                value: value.clone(),
            },
        );
        if cache.len() > 400 {
            cache.retain(|_, entry| entry.computed_at.elapsed() < Duration::from_secs(60));
        }
    }
    if value.get("first10_count").and_then(Value::as_i64).unwrap_or(0) > 0 {
        let _ = persist_funding_cluster(stores, mint, &value).await;
    }
    value
}

async fn persist_funding_cluster(
    stores: &Stores,
    mint: &str,
    value: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.funding_clusters
           (mint, algorithm_version, knowledge_cutoff, first10_count, unique_slots,
            same_slot_max_group, same_slot_first10_share, creator_in_first10, funders_known,
            unique_funders, same_funder_first10_count, largest_funder_share,
            creator_funder_overlap, evidence_status, payload)
           VALUES ($1,$2,now(),$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
           ON CONFLICT (mint, algorithm_version) DO UPDATE SET
             computed_at=now(),
             knowledge_cutoff=EXCLUDED.knowledge_cutoff,
             first10_count=EXCLUDED.first10_count,
             unique_slots=EXCLUDED.unique_slots,
             same_slot_max_group=EXCLUDED.same_slot_max_group,
             same_slot_first10_share=EXCLUDED.same_slot_first10_share,
             creator_in_first10=EXCLUDED.creator_in_first10,
             funders_known=EXCLUDED.funders_known,
             unique_funders=EXCLUDED.unique_funders,
             same_funder_first10_count=EXCLUDED.same_funder_first10_count,
             largest_funder_share=EXCLUDED.largest_funder_share,
             creator_funder_overlap=EXCLUDED.creator_funder_overlap,
             evidence_status=EXCLUDED.evidence_status,
             payload=EXCLUDED.payload"#,
    )
    .bind(mint)
    .bind(value.get("algorithm_version").and_then(Value::as_str).unwrap_or(FUNDING_CLUSTER_VERSION))
    .bind(value.get("first10_count").and_then(Value::as_i64).unwrap_or(0) as i32)
    .bind(value.get("unique_slots").and_then(Value::as_i64).map(|n| n as i32))
    .bind(value.get("same_slot_max_group").and_then(Value::as_i64).map(|n| n as i32))
    .bind(value.get("same_slot_first10_share").and_then(Value::as_f64))
    .bind(value.get("creator_in_first10").and_then(Value::as_bool).unwrap_or(false))
    .bind(value.get("funders_known").and_then(Value::as_i64).unwrap_or(0) as i32)
    .bind(value.get("unique_funders").and_then(Value::as_i64).map(|n| n as i32))
    .bind(value.get("same_funder_first10_count").and_then(Value::as_i64).map(|n| n as i32))
    .bind(value.get("largest_funder_share").and_then(Value::as_f64))
    .bind(value.get("creator_funder_overlap").and_then(Value::as_bool))
    .bind(value.get("evidence_status").and_then(Value::as_str).unwrap_or("UNKNOWN"))
    .bind(value)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

async fn compute_funding_cluster(
    stores: &Stores,
    mint: &str,
    creator: &str,
    knowledge_cutoff_slot: i64,
) -> Value {
    let buyers = sqlx::query(
        r#"SELECT source_node, MIN(slot) AS first_slot
           FROM pumphound.graph_facts
           WHERE asset_mint=$1 AND edge_type='BOUGHT' AND slot<=$2
           GROUP BY source_node
           ORDER BY MIN(slot) ASC
           LIMIT 10"#,
    )
    .bind(mint)
    .bind(knowledge_cutoff_slot)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    if buyers.is_empty() {
        return json!({
            "algorithm_version": FUNDING_CLUSTER_VERSION,
            "evidence_status": "UNKNOWN",
            "first10_count": 0,
            "same_slot_first10_share": Value::Null,
            "same_funder_first10_count": Value::Null,
            "unique_funders": Value::Null,
            "largest_funder_share": Value::Null,
            "funders_known": 0,
            "creator_in_first10": false
        });
    }
    let mut wallets = Vec::new();
    let mut slots = Vec::new();
    for row in &buyers {
        let node: String = row.get("source_node");
        let wallet = node.strip_prefix("wallet:").unwrap_or(&node).to_string();
        wallets.push(wallet);
        slots.push(row.get::<i64, _>("first_slot"));
    }
    let first10 = wallets.len();
    let mut slot_groups = std::collections::BTreeMap::<i64, usize>::new();
    for slot in &slots {
        *slot_groups.entry(*slot).or_insert(0) += 1;
    }
    let same_slot_max = slot_groups.values().copied().max().unwrap_or(0);
    let unique_slots = slot_groups.len();
    let same_slot_share = same_slot_max as f64 / first10 as f64;
    let creator_in_first10 = wallets.iter().any(|wallet| wallet == creator);

    let funder_rows = sqlx::query(
        r#"SELECT wallet, funder FROM pumphound.funding_edges
           WHERE wallet = ANY($1) AND funder IS NOT NULL"#,
    )
    .bind(&wallets)
    .fetch_all(&stores.postgres)
    .await
    .unwrap_or_default();
    let mut funder_of = std::collections::BTreeMap::<String, String>::new();
    for row in funder_rows {
        let wallet: String = row.get("wallet");
        if let Some(funder) = row.try_get::<Option<String>, _>("funder").ok().flatten() {
            funder_of.entry(wallet).or_insert(funder);
        }
    }
    let funders_known = wallets.iter().filter(|w| funder_of.contains_key(*w)).count();
    let (unique_funders, same_funder_first10, largest_share, creator_overlap) =
        if funders_known == 0 {
            (Value::Null, Value::Null, Value::Null, Value::Null)
        } else {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for wallet in &wallets {
                if let Some(funder) = funder_of.get(wallet) {
                    *counts.entry(funder.clone()).or_insert(0) += 1;
                }
            }
            let unique = counts.len();
            let max = counts.values().copied().max().unwrap_or(0);
            let creator_funder = funder_of.get(creator);
            let overlap = creator_funder.is_some_and(|cf| {
                wallets
                    .iter()
                    .filter(|w| *w != creator)
                    .any(|w| funder_of.get(w) == Some(cf))
            });
            (
                Value::from(unique as u64),
                Value::from(max as u64),
                Value::from(max as f64 / first10 as f64),
                Value::from(overlap),
            )
        };

    for wallet in &wallets {
        enqueue_funding_lookup(stores, wallet).await;
    }
    if !creator.is_empty() {
        enqueue_funding_lookup(stores, creator).await;
    }

    json!({
        "algorithm_version": FUNDING_CLUSTER_VERSION,
        "evidence_status": if funders_known == 0 { "OBSERVED_BUYERS_UNKNOWN_FUNDERS" } else { "PARTIAL" },
        "first10_count": first10,
        "unique_slots": unique_slots,
        "same_slot_max_group": same_slot_max,
        "same_slot_first10_share": same_slot_share,
        "creator_in_first10": creator_in_first10,
        "funders_known": funders_known,
        "unique_funders": unique_funders,
        "same_funder_first10_count": same_funder_first10,
        "largest_funder_share": largest_share,
        "creator_funder_overlap": creator_overlap,
        "wallets": wallets,
        "note": "same_funder_* is null until a funding_edge exists. Counts are not a rug verdict."
    })
}
