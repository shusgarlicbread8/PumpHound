use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/prediction/markets", get(markets))
        .route("/api/v1/prediction/events", get(events))
        .route("/api/v1/prediction/relations", get(relations))
        .route("/api/v1/prediction/books", get(books))
}

async fn markets(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT market_id, event_id, question, yes_price::float8 AS yes_price, no_price::float8 AS no_price,
                  best_bid::float8 AS best_bid, best_ask::float8 AS best_ask, spread::float8 AS spread,
                  volume::float8 AS volume, liquidity::float8 AS liquidity, end_date, resolution_source, active, closed, updated_at
           FROM pumphound.prediction_markets
           ORDER BY volume DESC NULLS LAST
           LIMIT 200"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "market_id": r.get::<String, _>("market_id"),
                "event_id": r.get::<Option<String>, _>("event_id"),
                "question": r.get::<Option<String>, _>("question"),
                "yes": r.get::<Option<f64>, _>("yes_price"),
                "no": r.get::<Option<f64>, _>("no_price"),
                "best_bid": r.get::<Option<f64>, _>("best_bid"),
                "best_ask": r.get::<Option<f64>, _>("best_ask"),
                "spread": r.get::<Option<f64>, _>("spread"),
                "volume": r.get::<Option<f64>, _>("volume"),
                "liquidity": r.get::<Option<f64>, _>("liquidity"),
                "end_date": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("end_date"),
                "resolution_source": r.get::<Option<String>, _>("resolution_source"),
                "active": r.get::<Option<bool>, _>("active"),
                "closed": r.get::<Option<bool>, _>("closed"),
            })).collect::<Vec<_>>(),
            "trading": false
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}

async fn events(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query("SELECT event_id, title, slug, category, updated_at FROM pumphound.prediction_events ORDER BY updated_at DESC LIMIT 200")
        .fetch_all(&s.stores.postgres)
        .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "event_id": r.get::<String, _>("event_id"),
                "title": r.get::<Option<String>, _>("title"),
                "slug": r.get::<Option<String>, _>("slug"),
                "category": r.get::<Option<String>, _>("category"),
            })).collect::<Vec<_>>()
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}

async fn relations(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query("SELECT left_id, right_id, relation_type, provenance, evidence FROM pumphound.prediction_relations")
        .fetch_all(&s.stores.postgres)
        .await;
    match rows {
        Ok(rows) => {
            let items: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "left_id": r.get::<String, _>("left_id"),
                        "right_id": r.get::<String, _>("right_id"),
                        "relation_type": r.get::<String, _>("relation_type"),
                        "provenance": r.get::<String, _>("provenance"),
                        "evidence": r.get::<Value, _>("evidence"),
                    })
                })
                .collect();
            let mut proven = 0;
            let mut likely = 0;
            let mut unverified = 0;
            for item in &items {
                match item.get("provenance").and_then(Value::as_str) {
                    Some("PROVEN_RELATION") => proven += 1,
                    Some("LIKELY_RELATION") => likely += 1,
                    _ => unverified += 1,
                }
            }
            Json(json!({"items": items, "counts": {"PROVEN_RELATION": proven, "LIKELY_RELATION": likely, "UNVERIFIED_RELATION": unverified}}))
        }
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}

async fn books(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT token_id, observed_at, best_bid::float8 AS best_bid, best_ask::float8 AS best_ask,
                  mid::float8 AS mid, spread::float8 AS spread
           FROM pumphound.prediction_books
           ORDER BY observed_at DESC LIMIT 80"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "token_id": r.get::<String, _>("token_id"),
                "observed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("observed_at"),
                "best_bid": r.get::<Option<f64>, _>("best_bid"),
                "best_ask": r.get::<Option<f64>, _>("best_ask"),
                "mid": r.get::<Option<f64>, _>("mid"),
                "spread": r.get::<Option<f64>, _>("spread"),
            })).collect::<Vec<_>>()
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}
