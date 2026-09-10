use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/venues/health", get(health))
        .route("/api/v1/venues/storage", get(storage))
}

async fn health(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT venue, provider, state, last_success_at, last_error, error_count_1h, queue_depth,
                  observations, market_count, stale_after_ms, extra, updated_at
           FROM pumphound.venue_provider_health
           ORDER BY venue, provider"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "venue": r.get::<String, _>("venue"),
                "provider": r.get::<String, _>("provider"),
                "state": r.get::<String, _>("state"),
                "last_success_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_success_at"),
                "last_error": r.get::<Option<String>, _>("last_error"),
                "error_count_1h": r.get::<i32, _>("error_count_1h"),
                "queue_depth": r.get::<Option<i32>, _>("queue_depth"),
                "observations": r.get::<i64, _>("observations"),
                "market_count": r.get::<i32, _>("market_count"),
                "stale_after_ms": r.get::<i64, _>("stale_after_ms"),
                "extra": r.get::<Value, _>("extra"),
                "updated_at": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
            })).collect::<Vec<_>>(),
            "note": "Per-venue/provider. A stale Ostium feed does not mark Pump down."
        })),
        Err(error) => Json(json!({
            "items": [],
            "state": "UNKNOWN",
            "error": error.to_string()
        })),
    }
}

async fn storage(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT venue, measured_bytes, measured_at, first_measured_at, first_measured_bytes,
                  bytes_per_minute::float8 AS bytes_per_minute, estimated_30d_bytes::float8 AS estimated_30d_bytes,
                  estimated_90d_bytes::float8 AS estimated_90d_bytes, retention_policy, extra
           FROM pumphound.venue_storage_stats
           ORDER BY venue"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "venue": r.get::<String, _>("venue"),
                "measured_bytes": r.get::<Option<i64>, _>("measured_bytes"),
                "measured_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("measured_at"),
                "bytes_per_minute": r.get::<Option<f64>, _>("bytes_per_minute"),
                "estimated_30d_bytes": r.get::<Option<f64>, _>("estimated_30d_bytes"),
                "estimated_90d_bytes": r.get::<Option<f64>, _>("estimated_90d_bytes"),
                "retention_policy": r.get::<Option<String>, _>("retention_policy"),
                "growth_basis": if r.get::<Option<f64>, _>("estimated_30d_bytes").is_some() { "ESTIMATED" } else { "MEASURED_ONLY" },
                "extra": r.get::<Value, _>("extra"),
            })).collect::<Vec<_>>()
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}
