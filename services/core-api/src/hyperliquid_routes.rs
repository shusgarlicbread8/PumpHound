use crate::state::AppState;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/hyperliquid/markets", get(markets))
        .route("/api/v1/hyperliquid/markets/{id}", get(market))
        .route("/api/v1/hyperliquid/experiments", get(experiments))
        .route("/api/v1/hyperliquid/paper", get(paper))
}

async fn markets(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT i.native_id, i.display_name, i.class, i.dex, i.deployer, i.first_observed_at, i.delisted, i.meta,
                  q.mark::float8 AS mark, q.mid::float8 AS mid, q.oracle::float8 AS oracle, q.funding::float8 AS funding,
                  q.open_interest::float8 AS open_interest, q.day_notional_volume::float8 AS day_notional_volume,
                  q.spread::float8 AS spread, q.lifecycle, q.observed_at
           FROM pumphound.venue_instruments i
           LEFT JOIN LATERAL (
             SELECT * FROM pumphound.hl_quotes q WHERE q.native_id = i.native_id ORDER BY observed_at DESC LIMIT 1
           ) q ON true
           WHERE i.venue = 'hyperliquid'
           ORDER BY i.class DESC, q.day_notional_volume DESC NULLS LAST, i.native_id
           LIMIT 400"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "market": r.get::<String, _>("native_id"),
                "name": r.get::<Option<String>, _>("display_name"),
                "type": r.get::<Option<String>, _>("class"),
                "dex": r.get::<Option<String>, _>("dex"),
                "builder": r.get::<Option<String>, _>("deployer"),
                "first_observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("first_observed_at"),
                "age_seconds": (chrono::Utc::now() - r.get::<chrono::DateTime<chrono::Utc>, _>("first_observed_at")).num_seconds(),
                "age_basis": "first_observed_by_this_collector_not_listing_time",
                "delisted": r.get::<bool, _>("delisted"),
                "mark": r.get::<Option<f64>, _>("mark"),
                "mid": r.get::<Option<f64>, _>("mid"),
                "oracle": r.get::<Option<f64>, _>("oracle"),
                "funding": r.get::<Option<f64>, _>("funding"),
                "oi": r.get::<Option<f64>, _>("open_interest"),
                "volume": r.get::<Option<f64>, _>("day_notional_volume"),
                "spread": r.get::<Option<f64>, _>("spread"),
                "maturity": r.get::<Option<String>, _>("lifecycle"),
                "quote_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("observed_at"),
                "model_rank": null,
                "paper_status": "PAPER_ONLY"
            })).collect::<Vec<_>>(),
            "sample": "SAMPLE_ONLY",
            "edge": "UNPROVEN",
            "authority": "NONE"
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}

async fn market(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let quote = sqlx::query(
        r#"SELECT observed_at, mark::float8 AS mark, mid::float8 AS mid, oracle::float8 AS oracle,
                  funding::float8 AS funding, open_interest::float8 AS open_interest,
                  day_notional_volume::float8 AS day_notional_volume, spread::float8 AS spread, lifecycle
           FROM pumphound.hl_quotes WHERE native_id=$1 ORDER BY observed_at DESC LIMIT 12"#,
    )
    .bind(&id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let books = sqlx::query(
        r#"SELECT observed_at, source_ts, best_bid::float8 AS best_bid, best_ask::float8 AS best_ask,
                  mid::float8 AS mid, top_levels, depth
           FROM pumphound.hl_l2_snapshots WHERE native_id=$1 ORDER BY observed_at DESC LIMIT 8"#,
    )
    .bind(&id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let ages = sqlx::query(
        r#"SELECT target_age_seconds, actual_observed_at, observation_delay_seconds, payload
           FROM pumphound.hl_age_snapshots WHERE native_id=$1 ORDER BY target_age_seconds"#,
    )
    .bind(&id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let life = sqlx::query(
        r#"SELECT observed_at, state, rule_version, evidence FROM pumphound.hl_lifecycle
           WHERE native_id=$1 ORDER BY observed_at DESC LIMIT 20"#,
    )
    .bind(&id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let inst = sqlx::query(
        r#"SELECT * FROM pumphound.venue_instruments WHERE venue='hyperliquid' AND native_id=$1"#,
    )
    .bind(&id)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten();
    Json(json!({
        "id": id,
        "instrument": inst.as_ref().map(|r| json!({
            "class": r.get::<Option<String>, _>("class"),
            "dex": r.get::<Option<String>, _>("dex"),
            "deployer": r.get::<Option<String>, _>("deployer"),
            "first_observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("first_observed_at"),
            "meta": r.get::<Value, _>("meta"),
        })),
        "quotes": quote.iter().map(|r| json!({
            "observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("observed_at"),
            "mark": r.get::<Option<f64>, _>("mark"),
            "mid": r.get::<Option<f64>, _>("mid"),
            "oracle": r.get::<Option<f64>, _>("oracle"),
            "funding": r.get::<Option<f64>, _>("funding"),
            "oi": r.get::<Option<f64>, _>("open_interest"),
            "volume": r.get::<Option<f64>, _>("day_notional_volume"),
            "spread": r.get::<Option<f64>, _>("spread"),
            "lifecycle": r.get::<Option<String>, _>("lifecycle"),
        })).collect::<Vec<_>>(),
        "books": books.iter().map(|r| json!({
            "observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("observed_at"),
            "best_bid": r.get::<Option<f64>, _>("best_bid"),
            "best_ask": r.get::<Option<f64>, _>("best_ask"),
            "mid": r.get::<Option<f64>, _>("mid"),
            "top_levels": r.get::<Value, _>("top_levels"),
            "depth": r.get::<Value, _>("depth"),
        })).collect::<Vec<_>>(),
        "age_snapshots": ages.iter().map(|r| json!({
            "target_age_seconds": r.get::<i32, _>("target_age_seconds"),
            "actual_observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("actual_observed_at"),
            "observation_delay_seconds": r.get::<i32, _>("observation_delay_seconds"),
            "payload": r.get::<Value, _>("payload"),
        })).collect::<Vec<_>>(),
        "lifecycle": life.iter().map(|r| json!({
            "observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("observed_at"),
            "state": r.get::<String, _>("state"),
            "rule_version": r.get::<String, _>("rule_version"),
            "evidence": r.get::<Value, _>("evidence"),
        })).collect::<Vec<_>>(),
        "liquidations": {"status": "UNSUPPORTED", "reason": "public tape requires a user address"},
        "basis": {"status": "UNSUPPORTED", "reason": "no defensible external spot reference wired"},
        "model_rank": null
    }))
}

async fn experiments(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query("SELECT * FROM pumphound.hl_experiments ORDER BY book")
        .fetch_all(&s.stores.postgres)
        .await;
    match rows {
        Ok(rows) => Json(json!({
            "family": "HIP-NEW-MARKET-V1",
            "items": rows.iter().map(|r| json!({
                "experiment_id": r.get::<String, _>("experiment_id"),
                "book": r.get::<String, _>("book"),
                "status": r.get::<String, _>("status"),
                "activation_reason": r.get::<Option<String>, _>("activation_reason"),
            })).collect::<Vec<_>>(),
            "sample": "SAMPLE_ONLY",
            "edge": "UNPROVEN",
            "authority": "NONE"
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}

async fn paper(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        "SELECT order_id, book, native_id, fill, created_at FROM pumphound.hl_paper_orders ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "order_id": r.get::<uuid::Uuid, _>("order_id"),
                "book": r.get::<Option<String>, _>("book"),
                "market": r.get::<String, _>("native_id"),
                "fill": r.get::<Value, _>("fill"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
            })).collect::<Vec<_>>(),
            "mode": "PAPER_ONLY"
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}
