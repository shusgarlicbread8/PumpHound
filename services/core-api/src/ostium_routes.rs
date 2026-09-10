use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use sqlx::Row;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/v1/ostium/markets", get(markets))
}

async fn markets(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT m.pair_id, m.symbol, m.from_asset, m.to_asset, m.first_observed_at,
                  o.observed_at, o.price::float8 AS price, o.funding_long::float8 AS funding_long,
                  o.funding_short::float8 AS funding_short, o.last_funding_rate::float8 AS last_funding_rate,
                  o.rollover::float8 AS rollover, o.rollover_long_acc::float8 AS rollover_long_acc,
                  o.rollover_short_acc::float8 AS rollover_short_acc, o.broker_premium::float8 AS broker_premium,
                  o.is_negative_rollover_allowed, o.oi_long::float8 AS oi_long, o.oi_short::float8 AS oi_short,
                  o.max_oi_usd::float8 AS max_oi_usd, o.utilization::float8 AS utilization,
                  o.price_impact_k::float8 AS price_impact_k, o.sim_slippage_status
           FROM pumphound.ostium_markets m
           LEFT JOIN LATERAL (
             SELECT * FROM pumphound.ostium_observations o WHERE o.pair_id = m.pair_id
             ORDER BY observed_at DESC LIMIT 1
           ) o ON true
           ORDER BY m.symbol"#,
    )
    .fetch_all(&s.stores.postgres)
    .await;
    match rows {
        Ok(rows) => Json(json!({
            "items": rows.iter().map(|r| json!({
                "pair_id": r.get::<String, _>("pair_id"),
                "symbol": r.get::<Option<String>, _>("symbol"),
                "from": r.get::<Option<String>, _>("from_asset"),
                "to": r.get::<Option<String>, _>("to_asset"),
                "price": r.get::<Option<f64>, _>("price"),
                "funding_long": r.get::<Option<f64>, _>("funding_long"),
                "funding_short": r.get::<Option<f64>, _>("funding_short"),
                "last_funding_rate": r.get::<Option<f64>, _>("last_funding_rate"),
                "rollover": r.get::<Option<f64>, _>("rollover"),
                "rollover_long_acc": r.get::<Option<f64>, _>("rollover_long_acc"),
                "rollover_short_acc": r.get::<Option<f64>, _>("rollover_short_acc"),
                "broker_premium": r.get::<Option<f64>, _>("broker_premium"),
                "is_negative_rollover_allowed": r.get::<Option<bool>, _>("is_negative_rollover_allowed"),
                "oi_long": r.get::<Option<f64>, _>("oi_long"),
                "oi_short": r.get::<Option<f64>, _>("oi_short"),
                "max_oi_usd": r.get::<Option<f64>, _>("max_oi_usd"),
                "utilization": r.get::<Option<f64>, _>("utilization"),
                "price_impact_k": r.get::<Option<f64>, _>("price_impact_k"),
                "sim_slippage_status": r.get::<Option<String>, _>("sim_slippage_status"),
                "observed_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("observed_at"),
            })).collect::<Vec<_>>(),
            "strategy": "NONE",
            "note": "Funding and rollover are separate columns. Observed 0 is stored as 0, not unknown."
        })),
        Err(error) => Json(json!({"items": [], "error": error.to_string()})),
    }
}
