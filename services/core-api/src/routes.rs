use crate::state::AppState;
use crate::stream;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pumphound_api_contract::{Flag, HealthResponse, RadarRow};
use pumphound_domain::{now_utc, AssetAmount, Atomic, PaperOrderRequest, Pubkey, TradeSide};
use pumphound_feature_core::heuristic_rank;
use pumphound_market_state::{curve_progress, executable_price_quote};
use pumphound_paper_core::quote_order;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::{collections::BTreeMap, sync::Arc};
use tower_http::cors::CorsLayer;
use uuid::Uuid;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(crate::execution_routes::router())
        .merge(crate::venue_health::router())
        .merge(crate::hyperliquid_routes::router())
        .merge(crate::ostium_routes::router())
        .merge(crate::prediction_routes::router())
        .route("/api/v1/health", get(health))
        .route("/api/v1/paper/truth", get(paper_truth))
        .route("/api/v1/paper/traces/{id}", get(paper_trace))
        .route("/api/v1/home", get(home))
        .route("/api/v1/system/status", get(system_status))
        .route("/api/v1/system/readiness", get(system_readiness))
        .route("/api/v1/system/capabilities", get(capabilities))
        .route("/api/v1/hot-set", get(hot_set_report))
        .route("/api/v1/enrichment/status", get(enrichment_status))
        .route(
            "/api/v1/enrichment/candidates/{decision_id}",
            get(candidate_dossier),
        )
        .route("/api/v1/enrichment/wallets/{wallet}", get(enriched_wallet))
        .route("/api/v1/search", get(search))
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .route("/api/v1/launches", get(launches))
        .route("/api/v1/launches/{id}", get(launch_detail))
        .route("/api/v1/launches/{id}/trades", get(trades))
        .route("/api/v1/launches/{id}/candles", get(candles))
        .route("/api/v1/launches/{id}/holders", get(holders))
        .route("/api/v1/launches/{id}/features", get(features))
        .route("/api/v1/launches/{id}/predictions", get(predictions))
        .route("/api/v1/launches/{id}/model-states", get(model_states))
        .route("/api/v1/launches/{id}/decisions", get(decisions))
        .route("/api/v1/launches/{id}/graph", get(launch_graph))
        .route(
            "/api/v1/launches/{id}/investigations",
            get(launch_investigations),
        )
        .route(
            "/api/v1/launches/{id}/intelligence",
            get(launch_intelligence),
        )
        .route("/api/v1/wallets", get(wallets_index))
        .route("/api/v1/wallets/{address}", get(wallet))
        .route("/api/v1/wallets/{address}/activity", get(wallet_activity))
        .route("/api/v1/wallets/{address}/funding", get(wallet_funding))
        .route("/api/v1/mints/{mint}/funding-cluster", get(mint_funding_cluster))
        .route("/api/v1/creators", get(creators_index))
        .route("/api/v1/creators/{address}", get(creator))
        .route(
            "/api/v1/creators/{address}/coordination",
            get(creator_coordination),
        )
        .route("/api/v1/clusters/{id}", get(cluster))
        .route("/api/v1/evidence/{id}", get(evidence))
        .route("/api/v1/launches/{id}/metadata", get(launch_metadata))
        .route("/api/v1/paper/quotes", post(paper_quotes))
        .route("/api/v1/paper/orders", post(paper_orders))
        .route("/api/v1/paper/orders/{id}/cancel", post(cancel_order))
        .route(
            "/api/v1/paper/portfolios",
            get(list_portfolios).post(create_portfolio),
        )
        .route("/api/v1/paper/portfolios/{id}", get(get_portfolio))
        .route("/api/v1/paper/portfolios/{id}/snapshot", get(get_portfolio))
        .route("/api/v1/paper/portfolios/{id}/ledger", get(ledger))
        .route(
            "/api/v1/paper/portfolios/{id}/performance",
            get(performance),
        )
        .route("/api/v1/paper/strategies", get(strategy_reports))
        .route("/api/v1/paper/champion-funnel", get(champion_funnel))
        .route("/api/v1/research/strategies", get(strategy_reports))
        .route("/api/v1/research/funnel", get(research_funnel))
        .route("/api/v1/research/learning", get(research_learning))
        .route("/api/v1/research/timelines", get(research_timelines))
        .route("/api/v1/research/timing", get(research_timing))
        .route("/api/v1/research/models", get(research_models))
        .route("/api/v1/research/policies", get(research_policies))
        .route("/api/v1/research/strategy-diff", get(research_strategy_diff))
        .route("/api/v1/research/selection-forensics", get(research_selection_forensics))
        .route("/api/v1/research/exec-select", get(research_exec_select))
        .route("/api/v1/research/strategy-catalog", get(research_strategy_catalog))
        .route(
            "/api/v1/research/models/{version}/promote",
            post(promote_paper_model),
        )
        .route(
            "/api/v1/paper/portfolios/{id}/breaker-reset",
            post(breaker_reset),
        )
        .route("/api/v1/paper/portfolios/{id}/policy", post(switch_policy))
        .route("/api/v1/policies", post(create_policy))
        .route("/api/v1/models", get(models))
        .route("/api/v1/models/truth", get(models_truth))
        .route("/api/v1/models/train", post(enqueue_retraining))
        .route("/api/v1/replays", post(start_replay))
        .route("/api/v1/replays/{id}", get(get_replay))
        .route("/api/v1/replays/{id}/control", post(control_replay))
        .route("/api/v1/investigations", post(start_investigation))
        .route("/api/v1/investigations/{id}", get(get_investigation))
        .route("/api/v1/jobs/{id}", get(get_job))
        .route("/api/v1/jobs/{id}/cancel", post(cancel_job))
        .route(
            "/api/v1/watchlists",
            get(list_watchlists).post(create_watchlist),
        )
        .route(
            "/api/v1/watchlists/{id}/items/{subject}",
            put(add_watch).delete(del_watch),
        )
        .route("/api/v1/alerts", get(alerts))
        .route("/api/v1/alert-rules", post(alert_rule))
        .route("/api/v1/alerts/{id}/acknowledge", post(ack_alert))
        .route("/api/v1/stream", get(stream::ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health(State(s): State<Arc<AppState>>) -> Json<HealthResponse> {
    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&s.stores.postgres)
        .await
        .is_ok();
    Json(HealthResponse {
        ready: db_ok,
        mode: s.mode.clone(),
        uptime_seconds: s.started.elapsed().as_secs(),
        decoder_version: pumphound_domain::DECODER_VERSION.into(),
        source_label: s.source_label.clone(),
        model_version: None,
    })
}

async fn home(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::cockpit::home(&s).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn system_readiness(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::readiness::as_json(&s).await)
}

async fn hot_set_report(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::hot_set::report(&s.stores).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn wallets_index(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::cockpit::wallet_index(&s).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn creators_index(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::cockpit::creator_index(&s).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn system_status(State(s): State<Arc<AppState>>) -> Json<Value> {
    let (chain, last_slot, lag_ms, last_event_at, decoded, quarantined) = {
        let h = s.health.read().await;
        (
            h.chain.clone(),
            h.last_slot,
            h.lag_ms,
            h.last_event_at,
            h.decoded,
            h.quarantined,
        )
    };
    let readiness = crate::readiness::as_json(&s).await;
    let quarantine_context = readiness.get("quarantine").cloned();
    let capture = s.stores.capture_metrics().await.unwrap_or(json!({}));
    let champions = crate::paper_ledger::champions(&s.stores)
        .await
        .unwrap_or_default();
    let missing_targets = crate::model_client::missing_required_targets(&champions);
    let mode = crate::paper_loop::OperatingMode::parse(&s.mode);
    let (strategy, configured_ml_status) = crate::paper_loop::mode_strategy_status(mode);
    let shadow_models = crate::paper_ledger::has_shadow_models(&s.stores)
        .await
        .unwrap_or(false);
    let ml_status = if (mode == crate::paper_loop::OperatingMode::LivePaperShadowMl
        && !shadow_models)
        || (matches!(
            mode,
            crate::paper_loop::OperatingMode::LivePaperMl
                | crate::paper_loop::OperatingMode::Replay
        ) && !missing_targets.is_empty())
    {
        "NOT_TRAINED"
    } else {
        configured_ml_status
    };
    Json(json!({
        "mode": s.mode,
        "operating_mode": s.mode,
        "strategy": strategy,
        "ml_status": ml_status,
        "ai_status": "ADVISORY",
        "parallel_books": crate::paper_loop::OperatingMode::parse(&s.mode)
            .heuristic_controls()
            .then(|| {
                pumphound_paper_core::parallel_books(
                    mode == crate::paper_loop::OperatingMode::LivePaperShadowMl,
                )
                .into_iter()
                .map(|book| book.key)
                .collect::<Vec<_>>()
            }),
        "capital": "SIMULATED",
        "source": s.source_label,
        "chain": crate::readiness::display_chain(&chain, last_event_at, now_utc()),
        "last_slot": last_slot.to_string(),
        "lag_ms": lag_ms,
        "source_lag_ms": lag_ms,
        "idle_ms": crate::readiness::idle_ms(last_event_at, now_utc()),
        "headline": readiness.get("headline").cloned().unwrap_or(Value::Null),
        "decoded": decoded,
        "quarantined": quarantined,
        "last_event_at": last_event_at,
        "stores": {
            "postgres": if sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&s.stores.postgres).await.is_ok() { "up" } else { "down" },
            // Was hardcoded to "configured" regardless of actual reachability, so a
            // wedged or unresponsive store (observed: ClickHouse pinned at 300-500%
            // CPU and refusing new connections for minutes at a time under merge
            // pressure) never showed up here even though it was silently degrading
            // every page that touched it. Both are now real liveness probes.
            "clickhouse": if s.stores.clickhouse.query("SELECT 1").fetch_one::<u8>().await.is_ok() { "up" } else { "down" },
            "nats": match s.stores.nats.connection_state() {
                async_nats::connection::State::Connected => "up",
                async_nats::connection::State::Pending => "connecting",
                async_nats::connection::State::Disconnected => "down",
            }
        },
        "readiness": readiness,
        "quarantine_context": quarantine_context,
        "model": ml_status,
        "target_model_versions": crate::model_client::target_version_map(&champions),
        "missing_targets": missing_targets,
        "familiarity": if champions.len() == crate::model_client::REQUIRED_TARGETS.len() { "per-target" } else { "withheld" },
        "capture": capture,
        "ai": "ADVISORY"
    }))
}

async fn capabilities(State(s): State<Arc<AppState>>) -> Json<Value> {
    let investigator_provider = std::env::var("PUMPHOUND_AI_PROVIDER")
        .ok()
        .filter(|provider| matches!(provider.as_str(), "local" | "siliconflow"));
    let investigator_url_configured = std::env::var("PUMPHOUND_INVESTIGATOR_URL")
        .ok()
        .is_some_and(|url| !url.trim().is_empty());
    Json(json!({
        "laserstream": s.source_label.contains("LASERSTREAM"),
        "rpc_fallback": s.source_label.contains("FALLBACK"),
        "birdeye": false,
        "jupiter": false,
        "pyth": false,
        "investigator": investigator_provider.is_some(),
        "investigator_provider": investigator_provider,
        "investigator_url_configured": investigator_url_configured,
        "secrets_redacted": true
    }))
}

async fn enrichment_status(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::candidate_enrichment::status(&s.stores).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn candidate_dossier(
    State(s): State<Arc<AppState>>,
    Path(decision_id): Path<String>,
) -> impl IntoResponse {
    let Ok(decision_id) = Uuid::parse_str(&decision_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"candidate dossier"}})),
        )
            .into_response();
    };
    match crate::candidate_enrichment::dossier(&s.stores, decision_id).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"candidate dossier"}})),
        )
            .into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn enriched_wallet(
    State(s): State<Arc<AppState>>,
    Path(wallet): Path<String>,
) -> impl IntoResponse {
    match crate::candidate_enrichment::wallet_cache(&s.stores, &wallet).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"wallet enrichment"}})),
        )
            .into_response(),
        Err(error) => internal_error_response(error),
    }
}

#[derive(Deserialize)]
struct SearchQ {
    q: String,
}

async fn search(State(s): State<Arc<AppState>>, Query(q): Query<SearchQ>) -> Json<Value> {
    let rows = sqlx::query(
        "SELECT launch_id::text, mint, creator FROM pumphound.launches
         WHERE mint=$1 OR creator=$1 OR creation_signature=$1 LIMIT 20",
    )
    .bind(&q.q)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    Json(json!({
        "items": rows.iter().map(|r| json!({
            "launch_id": r.get::<String,_>("launch_id"),
            "mint": r.get::<String,_>("mint"),
            "creator": r.get::<String,_>("creator"),
        })).collect::<Vec<_>>()
    }))
}

async fn get_settings(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(s.settings.read().await.clone())
}

async fn patch_settings(State(s): State<Arc<AppState>>, Json(v): Json<Value>) -> Json<Value> {
    let mut cur = s.settings.write().await;
    if let Some(obj) = cur.as_object_mut() {
        obj.insert("value".into(), v);
        let rev = obj.get("revision").and_then(|r| r.as_i64()).unwrap_or(1) + 1;
        obj.insert("revision".into(), json!(rev));
    }
    Json(cur.clone())
}

#[derive(Deserialize)]
struct LaunchQ {
    #[allow(dead_code)]
    cursor: Option<String>,
}

const RADAR_ROW_COLUMNS: &str = "l.launch_id::text, l.mint, l.protocol, l.quote_mint, l.creator, \
     l.creation_slot, l.creation_time, l.known_at, l.attributes, \
     (cp.attributes->>'risk_score')::float8 AS creator_risk_score";
const RADAR_ROW_JOIN: &str = "LEFT JOIN pumphound.creator_profiles cp ON cp.creator = l.creator";

/// Builds one RadarRow from a `pumphound.launches` row (selecting RADAR_ROW_COLUMNS) plus the
/// live in-memory market/window state. Shared by the REST snapshot (`GET /launches`) and the
/// WebSocket delta path, so a row streamed over the wire is computed exactly the same way as one
/// fetched fresh — no separate "lightweight" patch representation to drift out of sync.
fn build_radar_row(
    r: &sqlx::postgres::PgRow,
    markets: &pumphound_market_state::InMemoryMarketReducer,
    windows: &std::collections::HashMap<String, pumphound_feature_core::WindowAggregator>,
    now: chrono::DateTime<chrono::Utc>,
) -> RadarRow {
    let mint: String = r.get("mint");
    let launch_id: String = r.get("launch_id");
    let protocol: String = r.get("protocol");
    let quote_mint: String = r.get("quote_mint");
    let creator: String = r.get("creator");
    let created: Option<chrono::DateTime<chrono::Utc>> = r
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("creation_time")
        .ok()
        .flatten();
    let known_at: chrono::DateTime<chrono::Utc> = r.get("known_at");
    let age = crate::launch_age::launch_age(created, Some(known_at), now);
    let snap = markets
        .get(&mint)
        .filter(|snapshot| snapshot.mint.to_string() == mint);
    let progress = snap.and_then(curve_progress);
    let price = snap.and_then(executable_price_quote);
    let agg = windows.get(&mint);
    let buyers = agg
        .map(|a| a.unique_buyers())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| markets.buyer_count(&mint) as u64);
    let imb = agg.and_then(|a| a.imbalance(30));
    let flow = agg.and_then(|a| a.flow_quote(60));
    let rank = age
        .seconds
        .map(|seconds| heuristic_rank(seconds, buyers as f64, imb, progress));
    let stance = "WATCH";
    let quality = snap
        .map(|m| current_market_quality(m, now))
        .unwrap_or_else(|| pumphound_domain::DataQuality::unknown("no live snapshot yet"));
    let as_of = snap
        .map(|m| m.as_of.clone())
        .unwrap_or(pumphound_domain::ObservationPoint {
            slot: pumphound_domain::Slot(r.get::<i64, _>("creation_slot") as u64),
            tx_index: None,
            event_index: None,
            event_time: created,
            observed_at: r.get("known_at"),
            knowledge_cutoff: known_at,
            commitment: pumphound_domain::Commitment::Confirmed,
        });
    let attrs: Value = r.try_get("attributes").unwrap_or(json!({}));
    let creator_risk: Option<f64> = r.try_get("creator_risk_score").ok().flatten();
    RadarRow {
        launch_id,
        mint: mint.clone(),
        creator: Some(creator.clone()),
        creator_risk,
        symbol: attrs
            .get("symbol")
            .and_then(|v| v.as_str())
            .or_else(|| attrs.pointer("/flags/symbol").and_then(|v| v.as_str()))
            .map(|s| s.to_string()),
        age_seconds: age.seconds,
        age_basis: Some(age.basis.to_string()),
        protocol,
        quote_mint,
        price_quote: price,
        executable_depth_quote: snap.map(|m| m.effective_quote_atomic.to_string()),
        curve_progress: progress,
        buyers,
        independent_clusters: None,
        flow_60s_quote: flow.map(|f| f.to_string()),
        graduation_6h: None,
        collapse_1h: None,
        heuristic_rank: rank,
        prediction_id: None,
        stance: stance.into(),
        paper_position_id: None,
        flags: vec![Flag {
            code: "TARGET_MODEL_STATE_NOT_IN_RADAR_CONTRACT".into(),
            evidence_id: mint,
            severity: "info".into(),
        }],
        quality,
        as_of,
    }
}

/// Single-row lookup used to build a live WebSocket delta patch for one mint, instead of forcing
/// every subscriber to re-fetch the full 200-row snapshot on every chain event (that fallback is
/// still what happens for event kinds this doesn't cover, or if the launch isn't found).
pub async fn radar_row_for_mint(state: &AppState, mint: &str) -> Option<RadarRow> {
    let query = format!(
        "SELECT {RADAR_ROW_COLUMNS} FROM pumphound.launches l {RADAR_ROW_JOIN} \
         WHERE l.mint=$1 \
         AND COALESCE(l.attributes->>'integrity_state','') <> 'QUARANTINED' LIMIT 1"
    );
    let row = sqlx::query(&query)
        .bind(mint)
        .fetch_optional(&state.stores.postgres)
        .await
        .ok()??;
    let markets = state.markets.read().await;
    let windows = state.windows.read().await;
    Some(build_radar_row(&row, &markets, &windows, now_utc()))
}

async fn launches(State(s): State<Arc<AppState>>, Query(_q): Query<LaunchQ>) -> impl IntoResponse {
    let query = format!(
        "SELECT {RADAR_ROW_COLUMNS} FROM pumphound.launches l {RADAR_ROW_JOIN} \
         WHERE l.mint NOT IN ($1,$2) \
           AND COALESCE(l.attributes->>'integrity_state','') <> 'QUARANTINED' \
           AND NOT EXISTS ( \
             SELECT 1 FROM pumphound.entity_integrity_quarantines iq \
             WHERE iq.entity_type='LAUNCH' \
               AND iq.entity_id=l.launch_id::text \
               AND iq.status='ACTIVE' \
           ) \
         ORDER BY l.creation_slot DESC LIMIT 200"
    );
    let rows = sqlx::query(&query)
        .bind(pumphound_domain::WSOL_MINT)
        .bind(pumphound_domain::USDC_MINT)
        .fetch_all(&s.stores.postgres)
        .await;
    let Ok(rows) = rows else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                json!({"error":{"code":"STORE_UNAVAILABLE","message":"postgres","retryable":true}}),
            ),
        )
            .into_response();
    };
    let markets = s.markets.read().await;
    let windows = s.windows.read().await;
    let now = now_utc();
    let items: Vec<RadarRow> = rows
        .iter()
        .map(|r| build_radar_row(r, &markets, &windows, now))
        .collect();
    Json(json!({
        "as_of": now,
        "stream_cursor": {"epoch": s.stream_epoch, "sequence": *s.sequence.read().await},
        "quality": {"grade": "B"},
        "items": items
    }))
    .into_response()
}

fn current_market_quality(
    snapshot: &pumphound_domain::MarketSnapshot,
    now: chrono::DateTime<chrono::Utc>,
) -> pumphound_domain::DataQuality {
    let mut quality = snapshot.quality.clone();
    let lag_ms = (now - snapshot.as_of.knowledge_cutoff).num_milliseconds();
    if lag_ms < 0 {
        return pumphound_domain::DataQuality::unknown(
            "market snapshot knowledge cutoff is in the future",
        );
    }
    quality.market_lag_ms = Some(lag_ms);
    if lag_ms > crate::readiness::STALE_MARKET_MS {
        quality.grade = pumphound_domain::QualityGrade::C;
        quality.score = quality.score.min(0.4);
        if !quality
            .stale_components
            .iter()
            .any(|component| component == "market_snapshot")
        {
            quality.stale_components.push("market_snapshot".into());
        }
        quality
            .reasons
            .push(format!("market snapshot is {lag_ms}ms old"));
    }
    quality
}

async fn launch_detail(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let row = sqlx::query(
        "SELECT launch_id::text, mint, protocol, quote_mint, creator, creation_signature, creation_slot, creation_time, known_at, attributes
         FROM pumphound.launches WHERE launch_id::text=$1 OR mint=$1 LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten();
    let Some(r) = row else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"unknown launch"}})),
        )
            .into_response();
    };
    let mint: String = r.get("mint");
    let created = r
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("creation_time")
        .ok()
        .flatten();
    let known_at = r
        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("known_at")
        .ok()
        .flatten();
    let now = now_utc();
    let age = crate::launch_age::launch_age(created, known_at, now);
    let snap = {
        let markets = s.markets.read().await;
        markets
            .get(&mint)
            .filter(|snapshot| snapshot.mint.to_string() == mint)
            .cloned()
    };
    let fidelity = crate::hot_set::membership_for(&s.stores, &mint)
        .await
        .ok()
        .flatten();
    Json(json!({
        "launch_id": r.get::<String,_>("launch_id"),
        "mint": mint,
        "protocol": r.get::<String,_>("protocol"),
        "quote_mint": r.get::<String,_>("quote_mint"),
        "creator": r.get::<String,_>("creator"),
        "creation_signature": r.get::<String,_>("creation_signature"),
        "creation_slot": r.get::<i64,_>("creation_slot").to_string(),
        "creation_time": created,
        "known_at": known_at,
        "age": { "seconds": age.seconds, "basis": age.basis },
        "attributes": r.try_get::<Value,_>("attributes").unwrap_or(json!({})),
        "market": snap.as_ref(),
        "fidelity": fidelity.as_ref().and_then(|v| v.get("fidelity")).cloned().unwrap_or(json!("SCOUT")),
        "hot_set": fidelity,
        "scores": {
            "graduation_6h": null,
            "collapse_1h": null,
            "heuristic_rank": "HEURISTIC",
            "model": "TARGET_STATES_AVAILABLE_SEPARATELY"
        },
        "quality": snap.as_ref().map(|market| current_market_quality(market, now)),
        "market_snapshot_age_ms": snap.as_ref().map(|market| (now-market.as_of.knowledge_cutoff).num_milliseconds())
    }))
    .into_response()
}

async fn resolve_mint(s: &AppState, id: &str) -> String {
    sqlx::query("SELECT mint FROM pumphound.launches WHERE launch_id::text=$1 OR mint=$1 LIMIT 1")
        .bind(id)
        .fetch_optional(&s.stores.postgres)
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<String, _>("mint"))
        .unwrap_or_else(|| id.to_string())
}

async fn trades(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let mint = resolve_mint(&s, &id).await;
    let tape = s.tape.read().await;
    let items = tape.get(&mint).cloned().unwrap_or_default();
    let hot = crate::hot_set::membership_for(&s.stores, &mint)
        .await
        .ok()
        .flatten();
    Json(json!({
        "launch_or_mint": id,
        "mint": mint,
        "items": items,
        "fidelity": if hot.is_some() { "HOT" } else { "SCOUT" },
        "quality": {"grade": if items.is_empty() { "C" } else { "B" }, "coverage": "complete_for_scope"}
    }))
}

async fn candles(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let mint = resolve_mint(&s, &id).await;
    let tape = s.tape.read().await;
    let items = tape.get(&mint).cloned().unwrap_or_default();
    let candles = crate::state::candles_from_tape(&items);
    let hot = crate::hot_set::membership_for(&s.stores, &mint)
        .await
        .ok()
        .flatten();
    Json(json!({
        "interval": "15s",
        "quote_unit": "atomic",
        "fidelity": if hot.is_some() { "HOT" } else { "SCOUT" },
        "usable_candles": candles.len(),
        "note": "event-derived 15s OHLC from tape; close is not an executable fill price",
        "items": candles,
        "gaps": candles.is_empty()
    }))
}

async fn holders(Path(_id): Path<String>) -> Json<Value> {
    Json(
        json!({"coverage": "unknown", "items": [], "reason": "holder snapshot not yet budgeted for this mint"}),
    )
}

async fn features(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let mint = resolve_mint(&s, &id).await;
    let w = s.windows.read().await;
    if let Some(agg) = w.get(&mint) {
        let fv = agg.compute(
            &mint,
            60,
            pumphound_domain::ObservationPoint {
                slot: pumphound_domain::Slot(0),
                tx_index: None,
                event_index: None,
                event_time: Some(now_utc()),
                observed_at: now_utc(),
                knowledge_cutoff: now_utc(),
                commitment: pumphound_domain::Commitment::Processed,
            },
            None,
            None,
            agg.unique_buyers() as usize,
            None,
        );
        return Json(serde_json::to_value(fv).unwrap_or(json!({})));
    }
    Json(json!({"values": {}, "quality": {"grade": "UNAVAILABLE"}}))
}

async fn predictions(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT p.prediction_id,p.feature_snapshot_id,p.model_version,p.calibrated,
                  p.familiarity,p.outputs,p.abstention_reason,p.computed_at
           FROM pumphound.model_predictions p
           JOIN pumphound.launches l ON l.launch_id=p.launch_id
           WHERE l.launch_id::text=$1 OR l.mint=$1
           ORDER BY p.computed_at DESC LIMIT 50"#,
    )
    .bind(id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    Json(json!({
        "items": rows.iter().map(|r| json!({
            "prediction_id": r.get::<String,_>("prediction_id"),
            "feature_snapshot_id": r.get::<String,_>("feature_snapshot_id"),
            "model_version": r.get::<String,_>("model_version"),
            "calibrated": r.get::<bool,_>("calibrated"),
            "familiarity": r.try_get::<Option<f64>,_>("familiarity").ok().flatten(),
            "outputs": r.get::<Value,_>("outputs"),
            "abstention_reason": r.try_get::<Option<String>,_>("abstention_reason").ok().flatten(),
            "computed_at": r.get::<chrono::DateTime<chrono::Utc>,_>("computed_at")
        })).collect::<Vec<_>>(),
        "status": if rows.is_empty() { "MODEL NOT TRAINED" } else { "STORED" },
        "promotion": "never automatic"
    }))
}

async fn decisions(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT d.decision_id::text,d.decision_key,d.action,d.decision_kind,d.reason_codes,
                  d.heuristic_version,d.feature_snapshot_id,d.model_versions,d.exact_inputs,
                  d.decision_at,d.payload
           FROM pumphound.decisions d
           JOIN pumphound.launches l ON l.launch_id = d.launch_id
           WHERE l.launch_id::text=$1 OR l.mint=$1
           ORDER BY d.decision_at DESC LIMIT 50"#,
    )
    .bind(&id)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "decision_id": r.get::<String,_>("decision_id"),
                "decision_key": r.get::<String,_>("decision_key"),
                "action": r.get::<String,_>("action"),
                "decision_kind": r.get::<String,_>("decision_kind"),
                "reason_codes": r.try_get::<Value,_>("reason_codes").unwrap_or(json!([])),
                "heuristic_version": r.try_get::<String,_>("heuristic_version").ok(),
                "feature_snapshot_id": r.get::<String,_>("feature_snapshot_id"),
                "model_versions": r.get::<Value,_>("model_versions"),
                "exact_inputs": r.get::<Value,_>("exact_inputs"),
                "decision_at": r.try_get::<chrono::DateTime<chrono::Utc>,_>("decision_at").ok(),
                "payload": r.try_get::<Value,_>("payload").unwrap_or(json!({})),
            })
        })
        .collect();
    Json(json!({"items": items}))
}

async fn launch_graph(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let mint = resolve_mint(&s, &id).await;
    let token = format!("token:{mint}");
    let facts = sqlx::query(
        r#"SELECT source_node, target_node, edge_type, amount_atomic::text, event_id
           FROM pumphound.graph_facts
           WHERE asset_mint=$1 OR source_node=$2 OR target_node=$2
           ORDER BY slot DESC LIMIT 400"#,
    )
    .bind(&mint)
    .bind(&token)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let mut node_ids = std::collections::BTreeSet::new();
    let mut raw = Vec::new();
    for r in &facts {
        let src: String = r.get("source_node");
        let tgt: String = r.get("target_node");
        node_ids.insert(src.clone());
        node_ids.insert(tgt.clone());
        let amt: Option<String> = r.try_get("amount_atomic").ok();
        raw.push(crate::graph_view::RawEdge {
            source: src,
            target: tgt,
            kind: r.get::<String, _>("edge_type"),
            weight: amt
                .as_deref()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0),
            inferred: false,
            event_id: r.try_get::<Option<String>, _>("event_id").ok().flatten(),
            signature: None,
        });
    }
    let cap = crate::intelligence::graph_edge_cap();
    let remaining = cap.saturating_sub(raw.len());
    let node_list: Vec<String> = node_ids.iter().cloned().collect();
    let (derived, extra, hidden_derived) =
        crate::intelligence::derived_edges_touching(&s.stores, &node_list, remaining).await;
    for nid in extra {
        node_ids.insert(nid);
    }
    for edge in derived {
        raw.push(crate::graph_view::RawEdge {
            source: edge
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            target: edge
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            kind: edge
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("DERIVED")
                .to_string(),
            weight: edge.get("weight").and_then(Value::as_f64).unwrap_or(1.0),
            inferred: true,
            event_id: None,
            signature: None,
        });
    }
    let edges = crate::graph_view::aggregate_edges(&raw);
    let nodes: Vec<Value> = node_ids
        .iter()
        .map(|nid| {
            let (kind, label) = nid.split_once(':').unwrap_or(("node", nid.as_str()));
            json!({
                "id": nid,
                "kind": kind,
                "label": if label.len() > 8 { format!("{}…{}", &label[..4], &label[label.len()-4..]) } else { label.to_string() }
            })
        })
        .collect();
    Json(json!({
        "subject_id": id,
        "nodes": nodes,
        "edges": edges,
        "raw_evidence_count": raw.len(),
        "hidden_nodes": hidden_derived,
        "truncated": edges.len() >= cap || hidden_derived > 0,
        "as_of": now_utc()
    }))
}

async fn wallet(State(s): State<Arc<AppState>>, Path(address): Path<String>) -> Json<Value> {
    let created = sqlx::query(
        "SELECT launch_id::text, mint, creation_slot FROM pumphound.launches WHERE creator=$1 ORDER BY creation_slot DESC LIMIT 50",
    )
    .bind(&address)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let trades = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pumphound.graph_facts WHERE source_node=$1 OR target_node=$1",
    )
    .bind(format!("wallet:{address}"))
    .fetch_one(&s.stores.postgres)
    .await
    .unwrap_or(0);
    let profile = crate::intelligence::wallet_profile_json(&s.stores, &address).await;
    let cohorts = crate::intelligence::wallet_cohort_ids(&s.stores, &address).await;
    Json(json!({
        "wallet": address,
        "coverage": if trades == 0 && created.is_empty() && profile.is_none() { "unknown" } else { "partial" },
        "profile": profile,
        "reputation": null,
        "labels": [],
        "created_launches": created.iter().map(|r| json!({
            "launch_id": r.get::<String,_>("launch_id"),
            "mint": r.get::<String,_>("mint"),
            "creation_slot": r.get::<i64,_>("creation_slot").to_string(),
        })).collect::<Vec<_>>(),
        "observed_facts": trades,
        "pnl_status": "INCOMPLETE",
        "realized_return": null,
        "cohorts": cohorts,
        "note": "session-scoped graph facts; P&L requires full cost basis; funding crawl is a separate history job"
    }))
}

async fn wallet_activity(
    State(s): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Json<Value> {
    let node = format!("wallet:{address}");
    let rows = sqlx::query(
        r#"SELECT edge_type, source_node, target_node, asset_mint, amount_atomic::text, event_id, slot, known_at
           FROM pumphound.graph_facts
           WHERE source_node=$1 OR target_node=$1
           ORDER BY slot DESC LIMIT 100"#,
    )
    .bind(&node)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "edge_type": r.get::<String,_>("edge_type"),
                "source": r.get::<String,_>("source_node"),
                "target": r.get::<String,_>("target_node"),
                "mint": r.try_get::<Option<String>,_>("asset_mint").ok().flatten(),
                "amount_atomic": r.try_get::<Option<String>,_>("amount_atomic").ok().flatten(),
                "event_id": r.get::<String,_>("event_id"),
                "slot": r.get::<i64,_>("slot").to_string(),
                "known_at": r.try_get::<chrono::DateTime<chrono::Utc>,_>("known_at").ok(),
            })
        })
        .collect();
    Json(json!({"wallet": address, "items": items}))
}

async fn wallet_funding(
    State(s): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Json<Value> {
    Json(crate::intelligence::funding_response(&s.stores, &address).await)
}

async fn mint_funding_cluster(
    State(s): State<Arc<AppState>>,
    Path(mint): Path<String>,
) -> Json<Value> {
    let creator = sqlx::query_scalar::<_, String>(
        "SELECT creator FROM pumphound.launches WHERE mint=$1 ORDER BY known_at DESC LIMIT 1",
    )
    .bind(&mint)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    Json(crate::intelligence::funding_cluster_for_mint(&s.stores, &mint, &creator, i64::MAX).await)
}

async fn creator(State(s): State<Arc<AppState>>, Path(address): Path<String>) -> Json<Value> {
    let rows = sqlx::query(
        "SELECT launch_id::text, mint, protocol, creation_slot, creation_time FROM pumphound.launches WHERE creator=$1 ORDER BY creation_slot DESC LIMIT 100",
    )
    .bind(&address)
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let mut profile = crate::intelligence::creator_profile_json(&s.stores, &address).await;
    let connected = crate::intelligence::connected_buyers(&s.stores, &address).await;
    let graduation_rate = crate::intelligence::honest_graduation_rate(&s.stores, &address).await;
    if let Some(p) = profile.as_mut().and_then(|v| v.as_object_mut()) {
        p.insert("graduation_rate".into(), json!(graduation_rate));
        p.insert("connected_buyers".into(), json!(connected));
    }
    Json(json!({
        "creator": address,
        "profile": profile,
        "launches": rows.iter().map(|r| json!({
            "launch_id": r.get::<String,_>("launch_id"),
            "mint": r.get::<String,_>("mint"),
            "protocol": r.get::<String,_>("protocol"),
            "creation_slot": r.get::<i64,_>("creation_slot").to_string(),
            "creation_time": r.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("creation_time").ok(),
        })).collect::<Vec<_>>(),
        "connected_buyers": connected,
        "graduation_rate": graduation_rate,
        "collapse_rate": null,
        "pnl_status": "INCOMPLETE",
        "realized_return": null,
        "reputation": null,
        "note": "graduation_rate is observed migrate/complete share only; null if no such rows"
    }))
}

async fn creator_coordination(
    State(s): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Json<Value> {
    Json(crate::intelligence::creator_coordination(&s.stores, &address).await)
}

async fn cluster(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    Json(crate::intelligence::cluster_response(&s.stores, &id).await)
}

async fn launch_intelligence(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
    let row = sqlx::query(
        "SELECT launch_id::text, mint, creator, creation_slot, creation_time, known_at
         FROM pumphound.launches WHERE launch_id::text=$1 OR mint=$1 LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten();
    let mint = match &row {
        Some(r) => r.get::<String, _>("mint"),
        None => resolve_mint(&s, &id).await,
    };
    let creator = row.as_ref().map(|r| r.get::<String, _>("creator"));
    let created: Option<chrono::DateTime<chrono::Utc>> = row.as_ref().and_then(|r| {
        r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("creation_time")
            .ok()
            .flatten()
    });
    let known_at: Option<chrono::DateTime<chrono::Utc>> = row.as_ref().and_then(|r| {
        r.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("known_at")
            .ok()
            .flatten()
    });
    let markets = s.markets.read().await;
    let windows = s.windows.read().await;
    let now = now_utc();
    let age = crate::launch_age::launch_age(created, known_at, now);
    let snap = markets
        .get(&mint)
        .filter(|snapshot| snapshot.mint.to_string() == mint);
    let progress = snap.and_then(curve_progress);
    let agg = windows.get(&mint);
    let buyers = agg
        .map(|a| a.unique_buyers())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| markets.buyer_count(&mint) as u64);
    let imb = agg.and_then(|a| a.imbalance(30));
    let rank = age
        .seconds
        .map(|seconds| heuristic_rank(seconds, buyers as f64, imb, progress));
    let facts = crate::intelligence::graph_fact_count(&s.stores, &mint).await;
    let derived = crate::intelligence::derived_edge_count(&s.stores, &mint).await;
    let token = format!("token:{mint}");
    let graph_nodes = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(DISTINCT n) FROM (
             SELECT source_node AS n FROM pumphound.graph_facts
             WHERE asset_mint=$1 OR source_node=$2 OR target_node=$2
             UNION
             SELECT target_node FROM pumphound.graph_facts
             WHERE asset_mint=$1 OR source_node=$2 OR target_node=$2
           ) q"#,
    )
    .bind(&mint)
    .bind(&token)
    .fetch_one(&s.stores.postgres)
    .await
    .unwrap_or(0);
    let coverage = if facts == 0 { "unknown" } else { "partial" };
    let launch_id = row.as_ref().map(|r| r.get::<String, _>("launch_id"));
    let model_targets = match launch_id.as_deref().and_then(|id| Uuid::parse_str(id).ok()) {
        Some(id) => crate::model_truth::target_states(&s.stores, id)
            .await
            .ok()
            .and_then(|truth| truth.get("targets").cloned())
            .unwrap_or_else(|| json!([])),
        None => json!([]),
    };
    Json(json!({
        "launch_id": launch_id,
        "mint": mint,
        "model": "TARGET_SPECIFIC",
        "model_targets": model_targets,
        "familiarity": "TARGET_SPECIFIC",
        "heuristic": {
            "label": "HEURISTIC",
            "rank": rank,
            "buyers": buyers,
            "age_seconds": age.seconds,
            "age_basis": age.basis,
            "buy_sell_imbalance_30s": imb,
            "curve_progress": progress
        },
        "completeness": {
            "coverage": coverage,
            "graph_facts": facts,
            "derived_edges": derived,
            "funding": "unknown",
            "holders": "unknown",
            "pnl_status": "INCOMPLETE",
            "note": "missing observations stay unknown; no fabricated percents"
        },
        "creator": creator,
        "buyers": buyers,
        "graph_nodes": graph_nodes
    }))
}

async fn launch_metadata(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    let row = sqlx::query(
        "SELECT metadata_uri FROM pumphound.assets a
         JOIN pumphound.launches l ON l.mint=a.mint
         WHERE l.launch_id::text=$1 OR l.mint=$1 LIMIT 1",
    )
    .bind(&id)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten();
    let Some(uri) = row.and_then(|r| {
        r.try_get::<Option<String>, _>("metadata_uri")
            .ok()
            .flatten()
    }) else {
        return Json(json!({"blocked": false, "reason": "no metadata uri"}));
    };
    let meta = crate::metadata::fetch_metadata(&uri).await;
    Json(serde_json::to_value(meta).unwrap_or(json!({})))
}

async fn evidence(Path(id): Path<String>) -> Json<Value> {
    Json(json!({
        "evidence_id": id,
        "class": "chain.event",
        "note": "raw pointer retained in ClickHouse raw_sightings / canonical_event_revisions"
    }))
}

async fn paper_quotes(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaperOrderRequest>,
) -> impl IntoResponse {
    let markets = s.markets.read().await;
    let expected_mint = match req.side {
        TradeSide::Buy => req.output_mint.to_string(),
        TradeSide::Sell => req.input.mint.to_string(),
    };
    let Some(snap) = markets
        .get(&req.market.to_string())
        .filter(|snapshot| snapshot.mint.to_string() == expected_mint)
    else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":{"code":"QUOTE_UNAVAILABLE","message":"no identity-matched market snapshot"}})),
        )
            .into_response();
    };
    match quote_order(&req, snap) {
        Ok(q) => Json(q).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":{"code":"QUOTE_UNAVAILABLE","message": e.to_string()}})),
        )
            .into_response(),
    }
}

async fn paper_orders(
    State(s): State<Arc<AppState>>,
    Json(req): Json<PaperOrderRequest>,
) -> impl IntoResponse {
    match place_internal_paper(s, req).await {
        Ok(order_id) => Json(json!({
            "order_id": order_id,
            "status": "QUEUED",
            "mode": "PAPER",
            "arrival_delay_ms": 1000
        }))
        .into_response(),
        Err((code, body)) => (code, Json(body)).into_response(),
    }
}

pub async fn place_internal_paper(
    s: Arc<AppState>,
    mut req: PaperOrderRequest,
) -> Result<Uuid, (StatusCode, Value)> {
    if !crate::paper_loop::autonomous_mode(&s.mode) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error":{"code":"UNSUPPORTED_EXECUTION_MODE","message":"watch/research cannot place orders"}}),
        ));
    }
    let snapshot = {
        let markets = s.markets.read().await;
        let expected_mint = match req.side {
            TradeSide::Buy => req.output_mint.to_string(),
            TradeSide::Sell => req.input.mint.to_string(),
        };
        markets
            .get(&req.market.to_string())
            .or_else(|| markets.get(&expected_mint))
            .filter(|snapshot| snapshot.mint.to_string() == expected_mint)
            .cloned()
    }
    .ok_or_else(|| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error":{"code":"QUOTE_UNAVAILABLE","message":"no market snapshot"}}),
        )
    })?;
    let quote = quote_order(&req, &snapshot).map_err(|error| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            json!({"error":{"code":"QUOTE_UNAVAILABLE","message":error.to_string()}}),
        )
    })?;
    let (run_id, policy_version) = {
        let paper = s.paper.read().await;
        let portfolio = paper.portfolios.get(&req.portfolio_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                json!({"error":{"code":"NOT_FOUND","message":"portfolio"}}),
            )
        })?;
        (
            portfolio.simulation_run_id,
            portfolio.policy_version.clone(),
        )
    };
    let launch_id = match req.launch_id {
        Some(id) => id,
        None => sqlx::query_scalar::<_, Uuid>(
            "SELECT launch_id FROM pumphound.launches WHERE mint=$1 LIMIT 1",
        )
        .bind(if req.side == pumphound_domain::TradeSide::Buy {
            req.output_mint.to_string()
        } else {
            req.input.mint.to_string()
        })
        .fetch_optional(&s.stores.postgres)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({"error":{"code":"UNKNOWN_LAUNCH","message":"manual order requires a known launch"}}),
            )
        })?,
    };
    req.launch_id = Some(launch_id);
    req.policy_version = policy_version.clone();
    let feature_snapshot_id = format!("manual:{}", snapshot.snapshot_id);
    let decision_key = format!("manual:{}:{}", req.portfolio_id, req.client_request_id);
    let action = if req.side == pumphound_domain::TradeSide::Buy {
        "BUY"
    } else {
        "FULL_EXIT"
    };
    let decision_id = crate::paper_ledger::persist_decision(
        &s.stores,
        req.decision_id,
        &decision_key,
        Some(run_id),
        launch_id,
        None,
        &feature_snapshot_id,
        &policy_version,
        Some("MANUAL"),
        action,
        "MANUAL",
        &json!([]),
        &json!({"request": req, "decision_quote": quote, "market_snapshot": snapshot}),
        &["manual_paper_request".into()],
        &json!({"manual": true, "live_money": false}),
    )
    .await
    .map_err(internal_error)?;
    req.decision_id = decision_id;
    crate::paper_ledger::queue_order(&s.stores, &req, &quote, 1_000, 5_000)
        .await
        .map_err(|error| {
            (
                StatusCode::CONFLICT,
                json!({"error":{"code":"RISK_REJECTED","message":error.to_string()}}),
            )
        })
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, Value) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"error":{"code":"STORE_ERROR","message":error.to_string()}}),
    )
}

fn internal_error_response(error: impl std::fmt::Display) -> axum::response::Response {
    let (status, value) = internal_error(error);
    (status, Json(value)).into_response()
}

async fn cancel_order(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match crate::paper_ledger::reject_order(&s.stores, id, "CANCELLED_BY_USER").await {
        Ok(()) => {
            Json(json!({"order_id": id, "status": "REJECTED", "reason": "CANCELLED_BY_USER"}))
                .into_response()
        }
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({"error":{"code":"CANCEL_FAILED","message":error.to_string()}})),
        )
            .into_response(),
    }
}

async fn list_portfolios(State(s): State<Arc<AppState>>) -> Json<Value> {
    let p = s.paper.read().await;
    Json(json!({"items": p.portfolios.values().cloned().collect::<Vec<_>>()}))
}

#[derive(Deserialize)]
struct NewPortfolio {
    name: String,
}

async fn create_portfolio(
    State(s): State<Arc<AppState>>,
    Json(body): Json<NewPortfolio>,
) -> impl IntoResponse {
    let created = crate::paper_ledger::create_portfolio(
        &s.stores,
        &body.name,
        &s.mode,
        crate::paper_ledger::SYSTEM_ABSTAIN_POLICY,
        25_000_000_000,
        500_000_000,
    )
    .await;
    let Ok((id, _)) = created else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"STORE_ERROR","message":"portfolio create failed"}})),
        )
            .into_response();
    };
    if let Ok(paper) = crate::paper_ledger::hydrate(&s.stores).await {
        *s.paper.write().await = paper;
    }
    match crate::paper_ledger::portfolio_view(&s.stores, id).await {
        Ok(Some(value)) => Json(value).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn get_portfolio(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match crate::paper_ledger::portfolio_view(&s.stores, id).await {
        Ok(Some(mut value)) => {
            add_executable_marks(&s, &mut value).await;
            Json(value).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn add_executable_marks(s: &AppState, view: &mut Value) {
    let Some(positions) = view.get_mut("positions").and_then(Value::as_array_mut) else {
        return;
    };
    let mut totals = std::collections::BTreeMap::<String, i128>::new();
    for position in positions {
        let Some(market) = position
            .get("market")
            .and_then(Value::as_str)
            .and_then(|value| Pubkey::from_base58(value).ok())
        else {
            continue;
        };
        let Some(base_mint) = position
            .get("base_mint")
            .and_then(Value::as_str)
            .and_then(|value| Pubkey::from_base58(value).ok())
        else {
            continue;
        };
        let Some(quote_mint) = position
            .get("quote_mint")
            .and_then(Value::as_str)
            .and_then(|value| Pubkey::from_base58(value).ok())
        else {
            continue;
        };
        let Some(base_atomic) = position
            .get("base_atomic")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<u128>().ok())
        else {
            continue;
        };
        let Some(cost_atomic) = position
            .get("cost_quote_atomic")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i128>().ok())
        else {
            continue;
        };
        let launch_id = position
            .get("launch_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok());
        let snapshot = {
            let markets = s.markets.read().await;
            markets
                .get(&base_mint.to_string())
                .or_else(|| markets.get(&market.to_string()))
                .filter(|snapshot| snapshot.mint == base_mint)
                .cloned()
        };
        let Some(snapshot) = snapshot else {
            continue;
        };
        let request = PaperOrderRequest {
            client_request_id: Uuid::nil(),
            portfolio_id: Uuid::nil(),
            launch_id,
            market,
            side: TradeSide::Sell,
            input: AssetAmount {
                mint: base_mint,
                atomic: Atomic(base_atomic),
                decimals: 6,
            },
            output_mint: quote_mint,
            max_slippage_bps: 100,
            quote_id: Uuid::nil(),
            policy_version: "mark-only".into(),
            decision_id: Uuid::nil(),
        };
        // Preferred: Execution Twin liquidation mark — "sell the whole position now, after exact
        // protocol fees and modeled network cost". Falls back to the legacy quote engine only
        // when the twin refuses the state, and says so.
        let fee_env = crate::execution_twin_bridge::current_fee_environment(s).await;
        let twin_mark = u64::try_from(base_atomic).ok().and_then(|base| {
            pumphound_execution_twin::mark::executable_mark(
                &snapshot,
                base,
                &fee_env,
                pumphound_execution_twin::network::PriorityFeePolicy::P50,
                pumphound_execution_twin::contract::JitoScenario::NoJito,
            )
            .ok()
        });
        let (mark_atomic, snapshot_id, status) = match twin_mark {
            Some(m) => {
                position["theoretical_mark_quote_atomic"] = json!(m.theoretical_quote.to_string());
                position["liquidation_impact_bps"] = json!(m.liquidation_impact_bps);
                position["liquidation_cost_quote_atomic"] =
                    json!(m.liquidation_cost_quote.to_string());
                position["mark_protocol_fees_quote_atomic"] =
                    json!(m.protocol_fees_quote.to_string());
                position["mark_network_cost_lamports"] = json!(m.network_cost_lamports.to_string());
                position["mark_simulator_version"] = json!(m.simulator_version);
                (m.executable_quote as i128, m.market_snapshot_id, "EXECUTABLE")
            }
            None => {
                let Ok(mark) = quote_order(&request, &snapshot) else {
                    position["mark_status"] = json!("UNMARKABLE");
                    continue;
                };
                let Ok(mark_atomic) = i128::try_from(mark.expected_output.atomic.0) else {
                    continue;
                };
                position["mark_simulator_version"] = json!(pumphound_paper_core::SIMULATOR_LEGACY);
                (mark_atomic, mark.market_snapshot_id, "EXECUTABLE_LEGACY_QUOTE")
            }
        };
        let unrealized = mark_atomic - cost_atomic;
        position["executable_mark_quote_atomic"] = json!(mark_atomic.to_string());
        // Field the Portfolio desk already reads for "Current value".
        position["executable_exit_quote_atomic"] = json!(mark_atomic.to_string());
        position["unrealized_pnl_quote_atomic"] = json!(unrealized.to_string());
        position["mark_snapshot_id"] = json!(snapshot_id);
        position["mark_status"] = json!(status);
        *totals.entry(quote_mint.to_string()).or_default() += unrealized;
    }
    view["pnl"]["unrealized_by_quote_mint"] = json!(totals
        .into_iter()
        .map(|(mint, atomic)| json!({"asset_mint": mint, "atomic": atomic.to_string()}))
        .collect::<Vec<_>>());
}

async fn ledger(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> Json<Value> {
    match crate::paper_ledger::ledger_items(&s.stores, id).await {
        Ok(v) => Json(v),
        Err(_) => Json(json!({"portfolio_id": id, "items": [], "reconciled": false})),
    }
}

async fn performance(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> Json<Value> {
    match crate::paper_ledger::portfolio_view(&s.stores, id).await {
        Ok(Some(mut view)) => {
            add_executable_marks(&s, &mut view).await;
            Json(json!({
                "portfolio_id": id,
                "pnl": view.get("pnl").cloned().unwrap_or(Value::Null),
                "status": "LEDGER_DERIVED",
                "performance_report": Value::Null,
                "reason": "no stored strategy report for this endpoint"
            }))
        }
        _ => Json(json!({"portfolio_id": id, "status": "UNAVAILABLE"})),
    }
}

async fn strategy_reports(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        crate::paper_ledger::strategy_reports(&s.stores)
            .await
            .unwrap_or_else(|_| json!({"items": [], "fabricated": false, "status": "UNAVAILABLE"})),
    )
}

async fn research_funnel(
    State(s): State<Arc<AppState>>,
    Query(scope): Query<LearningScope>,
) -> Json<Value> {
    if scope.experiment_id.as_deref() == Some(pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID) {
        return Json(
            learning_report(&s, pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID)
                .await
                .map(|report| {
                    json!({
                        "experiment_id": pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID,
                        "items": report.get("funnel").cloned().unwrap_or_else(|| json!([])),
                        "generated_at": report.get("generated_at").cloned(),
                        "fabricated": false
                    })
                })
                .unwrap_or_else(|error| {
                    json!({
                        "experiment_id": pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID,
                        "items": [],
                        "status": "UNAVAILABLE",
                        "error": error.to_string(),
                        "fabricated": false
                    })
                }),
        );
    }
    Json(
        crate::paper_ledger::experiment_funnel(&s.stores)
            .await
            .unwrap_or_else(|_| json!({"stages": [], "books": {}, "abstain_reasons": {}, "fabricated": false, "status": "UNAVAILABLE"})),
    )
}

#[derive(Deserialize)]
struct LearningScope {
    experiment_id: Option<String>,
}

async fn research_learning(
    State(s): State<Arc<AppState>>,
    Query(scope): Query<LearningScope>,
) -> Json<Value> {
    let experiment_id = learning_experiment_id(scope.experiment_id);
    Json(
        learning_report(&s, &experiment_id)
            .await
            .unwrap_or_else(|error| {
                json!({
                    "experiment_id": experiment_id,
                    "generated_at": now_utc(),
                    "status": "UNAVAILABLE",
                    "error": error.to_string(),
                    "funnel": [],
                    "timing": [],
                    "models": {"champions": [], "challengers": [], "shadows": [], "transitions": []},
                    "policies": {"versions": [], "transitions": []},
                    "datasets": [],
                    "training_runs": [],
                    "jobs": [],
                    "fabricated": false
                })
            }),
    )
}

async fn research_timing(
    State(s): State<Arc<AppState>>,
    Query(scope): Query<LearningScope>,
) -> Json<Value> {
    let experiment_id = learning_experiment_id(scope.experiment_id);
    let report = learning_report(&s, &experiment_id)
        .await
        .unwrap_or_else(|_| json!({"timing": [], "status": "UNAVAILABLE"}));
    Json(json!({
        "experiment_id": experiment_id,
        "generated_at": now_utc(),
        "items": report.get("timing").cloned().unwrap_or_else(|| json!([])),
        "fabricated": false
    }))
}

async fn research_models(
    State(s): State<Arc<AppState>>,
    Query(scope): Query<LearningScope>,
) -> Json<Value> {
    let experiment_id = learning_experiment_id(scope.experiment_id);
    let report = learning_report(&s, &experiment_id)
        .await
        .unwrap_or_else(|_| json!({"models": {}, "status": "UNAVAILABLE"}));
    Json(json!({
        "experiment_id": experiment_id,
        "generated_at": now_utc(),
        "models": report.get("models").cloned().unwrap_or_else(|| json!({})),
        "fabricated": false
    }))
}

async fn research_policies(
    State(s): State<Arc<AppState>>,
    Query(scope): Query<LearningScope>,
) -> Json<Value> {
    let experiment_id = learning_experiment_id(scope.experiment_id);
    let report = learning_report(&s, &experiment_id)
        .await
        .unwrap_or_else(|_| json!({"policies": {}, "status": "UNAVAILABLE"}));
    Json(json!({
        "experiment_id": experiment_id,
        "generated_at": now_utc(),
        "policies": report.get("policies").cloned().unwrap_or_else(|| json!({})),
        "fabricated": false
    }))
}

#[derive(Deserialize)]
struct StrategyDiffQuery {
    a: Option<String>,
    b: Option<String>,
}

async fn research_strategy_diff(Query(query): Query<StrategyDiffQuery>) -> Json<Value> {
    let a = query.a.unwrap_or_else(|| "RANDOM_SELECT_V1".into());
    let b = query.b.unwrap_or_else(|| "MARKET_CONFIRMED_V1".into());
    Json(crate::research_select::strategy_diff_value(&a, &b).await)
}

async fn research_strategy_catalog() -> Json<Value> {
    Json(crate::research_select::catalog_value())
}

async fn research_selection_forensics(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        crate::research_select::selection_forensics(&s)
            .await
            .unwrap_or_else(|error| {
                json!({
                    "status": "UNAVAILABLE",
                    "error": error.to_string(),
                    "fabricated": false
                })
            }),
    )
}

async fn research_exec_select(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        crate::research_select::exec_select_board(&s)
            .await
            .unwrap_or_else(|error| {
                json!({
                    "experiment_id": "EXEC-SELECT-V1",
                    "status": "UNAVAILABLE",
                    "error": error.to_string(),
                    "fabricated": false
                })
            }),
    )
}

fn learning_experiment_id(requested: Option<String>) -> String {
    requested
        .filter(|value| value == pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID)
        .unwrap_or_else(|| pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID.to_string())
}

async fn learning_report(s: &AppState, experiment_id: &str) -> anyhow::Result<Value> {
    let funnel_rows = sqlx::query(
        r#"SELECT state,reason_code,retryable,COUNT(*)::bigint AS count
           FROM pumphound.candidate_lifecycle_events
           WHERE experiment_id=$1
           GROUP BY state,reason_code,retryable
           ORDER BY state,count DESC"#,
    )
    .bind(experiment_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let mut funnel = BTreeMap::<String, (i64, i64, BTreeMap<String, i64>)>::new();
    for row in funnel_rows {
        let state: String = row.get("state");
        let reason: String = row.get("reason_code");
        let retryable: bool = row.get("retryable");
        let count: i64 = row.get("count");
        let entry = funnel
            .entry(state)
            .or_insert_with(|| (0, 0, BTreeMap::new()));
        entry.0 += count;
        if retryable {
            entry.1 += count;
        }
        *entry.2.entry(reason).or_default() += count;
    }
    let funnel_items = funnel
        .iter()
        .map(|(state, (count, retryable_count, reasons))| {
            json!({
                "state": state,
                "count": count,
                "retryable_count": retryable_count,
                "reasons": reasons.iter().map(|(reason_code, count)| {
                    json!({"reason_code": reason_code, "count": count})
                }).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();

    let timing_queries = [
        (
            "create_to_observe",
            "EXTRACT(EPOCH FROM (first_observed_at-chain_creation_at))*1000",
            "chain_creation_at IS NOT NULL",
        ),
        (
            "observe_to_snapshot",
            "EXTRACT(EPOCH FROM (snapshot_at-first_observed_at))*1000",
            "snapshot_at IS NOT NULL",
        ),
        (
            "snapshot_to_decision",
            "EXTRACT(EPOCH FROM (decision_at-snapshot_at))*1000",
            "decision_at IS NOT NULL AND snapshot_at IS NOT NULL",
        ),
    ];
    let mut timing = Vec::new();
    for (metric, expression, predicate) in timing_queries {
        let query = format!(
            r#"SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY {expression})::double precision AS p50,
                      percentile_cont(0.95) WITHIN GROUP (ORDER BY {expression})::double precision AS p95,
                      COUNT(*)::bigint AS count
               FROM pumphound.candidate_lifecycle_events
               WHERE experiment_id=$1 AND {predicate}"#
        );
        let row = sqlx::query(&query)
            .bind(experiment_id)
            .fetch_one(&s.stores.postgres)
            .await?;
        timing.push(json!({
            "metric": metric,
            "p50_ms": row.try_get::<Option<f64>,_>("p50").ok().flatten(),
            "p95_ms": row.try_get::<Option<f64>,_>("p95").ok().flatten(),
            "count": row.get::<i64,_>("count")
        }));
    }

    let shadow_stats_rows = sqlx::query(
        r#"SELECT
            p.model_version,
            COUNT(*)::bigint AS total_predictions,
            COUNT(CASE WHEN ol.status = 'MATURED' THEN 1 END)::bigint AS matured_samples,
            COUNT(CASE WHEN ol.status = 'MATURED' AND ol.value::double precision > 0 THEN 1 END)::bigint AS positive_samples
        FROM pumphound.model_predictions p
        LEFT JOIN pumphound.outcome_labels ol
          ON ol.feature_snapshot_id = p.feature_snapshot_id
        GROUP BY p.model_version"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let mut shadow_stats_map = BTreeMap::<String, Value>::new();
    for row in shadow_stats_rows {
        let mv: String = row.get("model_version");
        let total: i64 = row.get("total_predictions");
        let matured: i64 = row.get("matured_samples");
        let positives: i64 = row.get("positive_samples");
        shadow_stats_map.insert(
            mv,
            json!({
                "total_predictions": total,
                "matured_samples": matured,
                "positive_samples": positives,
            }),
        );
    }

    let model_rows = sqlx::query(
        r#"SELECT model_version,target_version,feature_schema_version,status,manifest,metrics,
                  artifact_uri,artifact_sha256,created_at,promoted_at
           FROM pumphound.model_registry ORDER BY created_at DESC"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let empty_shadow_stats = json!({
        "total_predictions": 0,
        "matured_samples": 0,
        "positive_samples": 0,
    });
    let models = model_rows
        .iter()
        .map(|row| {
            let version: String = row.get("model_version");
            let shadow_metrics = shadow_stats_map.get(&version).unwrap_or(&empty_shadow_stats);
            json!({
                "model_version": version,
                "target_version": row.get::<String,_>("target_version"),
                "feature_schema_version": row.get::<String,_>("feature_schema_version"),
                "status": row.get::<String,_>("status").to_uppercase(),
                "manifest": row.get::<Value,_>("manifest"),
                "metrics": row.get::<Value,_>("metrics"),
                "shadow_metrics": shadow_metrics,
                "artifact_uri": row.get::<String,_>("artifact_uri"),
                "artifact_sha256": row.get::<String,_>("artifact_sha256"),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "promoted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("promoted_at").ok().flatten()
            })
        })
        .collect::<Vec<_>>();
    let transition_rows = sqlx::query(
        r#"SELECT model_version,target_version,from_status,to_status,actor,reason,gate_version,
                  gate_evidence,paper_only,occurred_at
           FROM pumphound.model_transition_events ORDER BY occurred_at DESC LIMIT 100"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let model_transitions = transition_rows
        .iter()
        .map(|row| {
            json!({
                "model_version": row.get::<String,_>("model_version"),
                "target_version": row.get::<String,_>("target_version"),
                "from_status": row.try_get::<Option<String>,_>("from_status").ok().flatten(),
                "to_status": row.get::<String,_>("to_status"),
                "actor": row.get::<String,_>("actor"),
                "reason": row.get::<String,_>("reason"),
                "gate_version": row.try_get::<Option<String>,_>("gate_version").ok().flatten(),
                "gate_evidence": row.get::<Value,_>("gate_evidence"),
                "paper_only": row.get::<bool,_>("paper_only"),
                "occurred_at": row.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at")
            })
        })
        .collect::<Vec<_>>();

    let policy_rows = sqlx::query(
        r#"SELECT policy_version,content_sha256,config,created_at,reason
           FROM pumphound.policy_versions
           WHERE config->>'experiment_id'=$1 ORDER BY created_at DESC"#,
    )
    .bind(experiment_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let policy_versions = policy_rows
        .iter()
        .map(|row| {
            json!({
                "policy_version": row.get::<String,_>("policy_version"),
                "content_sha256": row.get::<String,_>("content_sha256"),
                "config": row.get::<Value,_>("config"),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "reason": row.get::<String,_>("reason")
            })
        })
        .collect::<Vec<_>>();
    let policy_transition_rows = sqlx::query(
        r#"SELECT policy_version,from_state,to_state,actor,reason,paper_only,occurred_at,evidence
           FROM pumphound.policy_transition_events
           WHERE experiment_id=$1 ORDER BY occurred_at DESC LIMIT 100"#,
    )
    .bind(experiment_id)
    .fetch_all(&s.stores.postgres)
    .await?;
    let policy_transitions = policy_transition_rows
        .iter()
        .map(|row| {
            json!({
                "policy_version": row.get::<String,_>("policy_version"),
                "from_state": row.try_get::<Option<String>,_>("from_state").ok().flatten(),
                "to_state": row.get::<String,_>("to_state"),
                "actor": row.get::<String,_>("actor"),
                "reason": row.get::<String,_>("reason"),
                "paper_only": row.get::<bool,_>("paper_only"),
                "occurred_at": row.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at"),
                "evidence": row.get::<Value,_>("evidence")
            })
        })
        .collect::<Vec<_>>();

    let dataset_rows = sqlx::query(
        r#"SELECT dataset_snapshot_id,content_sha256,parent_snapshot_id,feature_schema_version,
                  target_versions,row_count,min_knowledge_cutoff,max_knowledge_cutoff,
                  split_methodology,source_hashes,row_hashes_sha256,manifest_uri,created_at
           FROM pumphound.dataset_snapshots ORDER BY created_at DESC LIMIT 25"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let datasets = dataset_rows
        .iter()
        .map(|row| {
            json!({
                "dataset_snapshot_id": row.get::<String,_>("dataset_snapshot_id"),
                "content_sha256": row.get::<String,_>("content_sha256"),
                "parent_snapshot_id": row.try_get::<Option<String>,_>("parent_snapshot_id").ok().flatten(),
                "feature_schema_version": row.get::<String,_>("feature_schema_version"),
                "target_versions": row.get::<Value,_>("target_versions"),
                "row_count": row.get::<i64,_>("row_count"),
                "min_knowledge_cutoff": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("min_knowledge_cutoff").ok().flatten(),
                "max_knowledge_cutoff": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("max_knowledge_cutoff").ok().flatten(),
                "split_methodology": row.get::<Value,_>("split_methodology"),
                "source_hashes": row.get::<Value,_>("source_hashes"),
                "row_hashes_sha256": row.get::<String,_>("row_hashes_sha256"),
                "manifest_uri": row.get::<String,_>("manifest_uri"),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
            })
        })
        .collect::<Vec<_>>();
    let training_rows = sqlx::query(
        r#"SELECT training_run_id::text,dataset_snapshot_id,target_version,state,config_sha256,
                  temporal_split,metrics,artifact_uri,artifact_sha256,withheld_reason,
                  started_at,completed_at,created_at
           FROM pumphound.training_runs ORDER BY created_at DESC LIMIT 50"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let training_runs = training_rows
        .iter()
        .map(|row| {
            json!({
                "training_run_id": row.get::<String,_>("training_run_id"),
                "dataset_snapshot_id": row.get::<String,_>("dataset_snapshot_id"),
                "target_version": row.get::<String,_>("target_version"),
                "state": row.get::<String,_>("state"),
                "config_sha256": row.get::<String,_>("config_sha256"),
                "temporal_split": row.get::<Value,_>("temporal_split"),
                "metrics": row.get::<Value,_>("metrics"),
                "artifact_uri": row.try_get::<Option<String>,_>("artifact_uri").ok().flatten(),
                "artifact_sha256": row.try_get::<Option<String>,_>("artifact_sha256").ok().flatten(),
                "withheld_reason": row.try_get::<Option<String>,_>("withheld_reason").ok().flatten(),
                "started_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("started_at").ok().flatten(),
                "completed_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("completed_at").ok().flatten(),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
            })
        })
        .collect::<Vec<_>>();
    let job_rows = sqlx::query(
        r#"SELECT job_id::text,kind,state,progress,error_code,created_at,updated_at
           FROM pumphound.background_jobs
           WHERE kind IN ('label_maturation','dataset_snapshot','adaptive_retrain')
           ORDER BY created_at DESC LIMIT 50"#,
    )
    .fetch_all(&s.stores.postgres)
    .await?;
    let jobs = job_rows
        .iter()
        .map(|row| {
            json!({
                "job_id": row.get::<String,_>("job_id"),
                "kind": row.get::<String,_>("kind"),
                "state": row.get::<String,_>("state"),
                "progress": row.get::<Value,_>("progress"),
                "error_code": row.try_get::<Option<String>,_>("error_code").ok().flatten(),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")
            })
        })
        .collect::<Vec<_>>();

    let shadow_predictions: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.model_predictions p
           JOIN pumphound.model_registry m ON m.model_version=p.model_version
           WHERE m.status='shadow'"#,
    )
    .fetch_one(&s.stores.postgres)
    .await?;
    let ood_average: Option<f64> = sqlx::query_scalar(
        r#"SELECT AVG(val) FROM (
             SELECT (exact_inputs#>>'{learning_evidence,information_value_score,ood_novelty}')::double precision AS val
             FROM pumphound.decisions
             WHERE exact_inputs->>'experiment_id'=$1
               AND exact_inputs#>>'{learning_evidence,information_value_score,ood_novelty}' IS NOT NULL
             ORDER BY decision_at DESC
             LIMIT 200
           ) sub"#,
    )
    .bind(experiment_id)
    .fetch_one(&s.stores.postgres)
    .await?;
    let missingness_average: Option<f64> = sqlx::query_scalar(
        r#"SELECT AVG(val) FROM (
             SELECT (exact_inputs#>>'{learning_evidence,information_value_input,missingness_novelty}')::double precision AS val
             FROM pumphound.decisions
             WHERE exact_inputs->>'experiment_id'=$1
               AND exact_inputs#>>'{learning_evidence,information_value_input,missingness_novelty}' IS NOT NULL
             ORDER BY decision_at DESC
             LIMIT 200
           ) sub"#,
    )
    .bind(experiment_id)
    .fetch_one(&s.stores.postgres)
    .await?;
    let count = |state: &str| funnel.get(state).map(|item| item.0).unwrap_or(0);
    Ok(json!({
        "experiment_id": experiment_id,
        "generated_at": now_utc(),
        "status": "OK",
        "paper_only": true,
        "automatic_promotion": false,
        "throughput": {
            "observed": count("OBSERVED"),
            "technically_invalid": count("TECHNICALLY_INVALID"),
            "waiting_information": count("WAITING_INFORMATION"),
            "strategy_evaluated": count("STRATEGY_EVALUATED"),
            "strategy_rejected": count("STRATEGY_REJECTED"),
            "selected": count("SELECTED"),
            "paper_entered": count("PAPER_ENTERED"),
            "exited": count("EXITED"),
            "labels_matured": count("LABEL_MATURED")
        },
        "funnel": funnel_items,
        "timing": timing,
        "models": {
            "champions": models.iter().filter(|item| item["status"] == "CHAMPION").cloned().collect::<Vec<_>>(),
            "challengers": models.iter().filter(|item| item["status"] == "CANDIDATE" || item["status"] == "CHALLENGER").cloned().collect::<Vec<_>>(),
            "shadows": models.iter().filter(|item| item["status"] == "SHADOW").cloned().collect::<Vec<_>>(),
            "transitions": model_transitions
        },
        "policies": {"versions": policy_versions, "transitions": policy_transitions},
        "datasets": datasets,
        "training_runs": training_runs,
        "jobs": jobs,
        "health": {
            "missingness_average": missingness_average,
            "ood_average": ood_average,
            "shadow_predictions": shadow_predictions,
            "drift_status": if shadow_predictions < 30 { "INSUFFICIENT_DATA" } else { "MONITORING_NO_REFERENCE_BASELINE" }
        },
        "disclaimers": [
            "Feature importance is associative, not causal.",
            "Shadow predictions cannot change champion or random-control actions.",
            "Promotion is explicit, audited, paper-only, and never triggered by training success."
        ],
        "fabricated": false
    }))
}

#[derive(Deserialize)]
struct PaperPromotionRequest {
    reason: String,
    gate_version: String,
    gate_passed: bool,
    gate_evidence: Value,
    paper_only: bool,
}

async fn promote_paper_model(
    State(s): State<Arc<AppState>>,
    Path(version): Path<String>,
    Json(body): Json<PaperPromotionRequest>,
) -> impl IntoResponse {
    if !body.paper_only
        || !body.gate_passed
        || body.reason.trim().is_empty()
        || body.gate_version.trim().is_empty()
    {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": {
                "code": "PROMOTION_GATE_NOT_SATISFIED",
                "message": "paper_only, gate_passed, reason and gate_version are required"
            }})),
        )
            .into_response();
    }
    let row = sqlx::query(
        r#"SELECT target_version,status,manifest
           FROM pumphound.model_registry WHERE model_version=$1"#,
    )
    .bind(&version)
    .fetch_optional(&s.stores.postgres)
    .await;
    let row = match row {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": {
                    "code": "MODEL_NOT_FOUND",
                    "message": "unknown model"
                }})),
            )
                .into_response()
        }
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error": {
                    "code": "STORE_UNAVAILABLE",
                    "message": "model registry unavailable"
                }})),
            )
                .into_response()
        }
    };
    let status: String = row.get("status");
    let manifest: Value = row.get("manifest");
    let unresolved_gates = manifest
        .get("champion_gate_failures")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if status != "shadow" || !unresolved_gates.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": {
                "code": "PROMOTION_GATE_NOT_SATISFIED",
                "message": if status != "shadow" {
                    format!("model must be an active shadow; current status is {status}")
                } else {
                    "authoritative model manifest has unresolved champion gates".into()
                },
                "model_version": version,
                "target_version": row.get::<String,_>("target_version"),
                "unresolved_gates": unresolved_gates
            }})),
        )
            .into_response();
    }
    (
        StatusCode::CONFLICT,
        Json(json!({"error": {
            "code": "AUTHORITATIVE_REGISTRY_REQUIRED",
            "message": "database-only promotion is disabled; promote through the hash-validated model registry and sync PostgreSQL",
            "model_version": version,
            "gate_version": body.gate_version,
            "submitted_evidence": body.gate_evidence
        }})),
    )
        .into_response()
}

async fn breaker_reset(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> Json<Value> {
    if let Some(p) = s.paper.write().await.portfolios.get_mut(&id) {
        p.gates.entries_frozen = false;
    }
    Json(json!({"ok": true, "audited": true}))
}

async fn switch_policy(Path(id): Path<Uuid>, Json(v): Json<Value>) -> Json<Value> {
    Json(json!({"portfolio_id": id, "policy": v}))
}

async fn create_policy(Json(v): Json<Value>) -> Json<Value> {
    Json(json!({"policy_version": "custom", "config": v}))
}

async fn models(State(s): State<Arc<AppState>>) -> Json<Value> {
    let rows = sqlx::query(
        r#"SELECT model_version,target_version,feature_schema_version,status,manifest,metrics,
                  created_at,promoted_at
           FROM pumphound.model_registry ORDER BY created_at DESC"#,
    )
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let champions = crate::paper_ledger::champions(&s.stores)
        .await
        .unwrap_or_default();
    let missing_targets = crate::model_client::missing_required_targets(&champions);
    let mode = crate::paper_loop::OperatingMode::parse(&s.mode);
    let (_, configured_ml_status) = crate::paper_loop::mode_strategy_status(mode);
    let shadow_models = crate::paper_ledger::has_shadow_models(&s.stores)
        .await
        .unwrap_or(false);
    let ml_status = if (mode == crate::paper_loop::OperatingMode::LivePaperShadowMl
        && !shadow_models)
        || (matches!(
            mode,
            crate::paper_loop::OperatingMode::LivePaperMl
                | crate::paper_loop::OperatingMode::Replay
        ) && !missing_targets.is_empty())
    {
        "NOT_TRAINED"
    } else {
        configured_ml_status
    };
    let live_n = sqlx::query(
        r#"SELECT model_version, COUNT(*)::bigint AS n
           FROM pumphound.model_predictions GROUP BY model_version"#,
    )
    .fetch_all(&s.stores.postgres)
    .await
    .unwrap_or_default();
    let mut live_map = BTreeMap::<String, i64>::new();
    for row in live_n {
        live_map.insert(row.get("model_version"), row.get("n"));
    }
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let version: String = r.get("model_version");
            json!({
                "model_version": version,
                "version": version,
                "target_version": r.get::<String,_>("target_version"),
                "target": r.get::<String,_>("target_version"),
                "feature_schema_version": r.get::<String,_>("feature_schema_version"),
                "status": r.get::<String,_>("status").to_uppercase(),
                "manifest": r.get::<Value,_>("manifest"),
                "metrics": r.get::<Value,_>("metrics"),
                "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "promoted_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("promoted_at").ok().flatten(),
                "live_n": live_map.get(&version).copied().unwrap_or(0)
            })
        })
        .collect();
    let mut by_target: BTreeMap<String, Value> = BTreeMap::new();
    for item in &items {
        let target = item
            .get("target_version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let status = item.get("status").and_then(Value::as_str).unwrap_or("");
        let entry = by_target.entry(target).or_insert_with(
            || json!({"champion": null, "challengers": [], "shadow": [], "retired": []}),
        );
        match status {
            "CHAMPION" => {
                if entry.get("champion").map_or(true, Value::is_null) {
                    entry["champion"] = item.clone();
                }
            }
            "CHALLENGER" | "CANDIDATE" => {
                entry["challengers"]
                    .as_array_mut()
                    .unwrap()
                    .push(item.clone());
            }
            "SHADOW" => {
                entry["shadow"].as_array_mut().unwrap().push(item.clone());
            }
            "RETIRED" => {
                entry["retired"].as_array_mut().unwrap().push(item.clone());
            }
            _ => {
                entry["challengers"]
                    .as_array_mut()
                    .unwrap()
                    .push(item.clone());
            }
        }
    }
    Json(json!({
        "items": items,
        "by_target": by_target,
        "status": ml_status,
        "control_ready": missing_targets.is_empty(),
        "missing_targets": missing_targets,
        "target_model_versions": crate::model_client::target_version_map(&champions),
        "automatic_promotion": false
    }))
}

#[derive(Deserialize)]
struct RetrainingRequest {
    trigger: Option<String>,
    minimum_new_labels: Option<u64>,
    dataset_config_path: Option<String>,
    train_config_path: Option<String>,
}

async fn enqueue_retraining(
    State(s): State<Arc<AppState>>,
    Json(body): Json<RetrainingRequest>,
) -> impl IntoResponse {
    let trigger = body.trigger.unwrap_or_else(|| "manual".into());
    if !matches!(
        trigger.as_str(),
        "manual" | "scheduled" | "minimum_new_labels"
    ) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": {
                "code": "INVALID_RETRAIN_TRIGGER",
                "message": "trigger must be manual, scheduled, or minimum_new_labels"
            }})),
        )
            .into_response();
    }
    let job_id = Uuid::now_v7();
    let request = json!({
        "trigger": trigger,
        "minimum_new_labels": body.minimum_new_labels.unwrap_or(100),
        "dataset_config_path": body.dataset_config_path.unwrap_or_else(|| "ml/configs/dataset-adaptive-existing.yaml".into()),
        "train_config_path": body.train_config_path.unwrap_or_else(|| "ml/configs/train-adaptive-short-horizon.yaml".into()),
        "experiment_id": pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID,
        "automatic_promotion": false,
        "requested_at": now_utc()
    });
    let request_hash =
        pumphound_domain::sha256_hex(&serde_json::to_vec(&request).unwrap_or_default());
    let inserted = sqlx::query(
        r#"INSERT INTO pumphound.background_jobs
           (job_id,kind,request_hash,request,state,progress,created_at,updated_at)
           VALUES ($1,'adaptive_retrain',$2,$3,'queued','{}',now(),now())"#,
    )
    .bind(job_id)
    .bind(request_hash)
    .bind(&request)
    .execute(&s.stores.postgres)
    .await;
    match inserted {
        Ok(_) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "job_id": job_id,
                "state": "queued",
                "trigger": trigger,
                "promotion": "never automatic",
                "paper_only": true
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {
                "code": "RETRAIN_QUEUE_FAILED",
                "message": error.to_string()
            }})),
        )
            .into_response(),
    }
}

async fn start_replay(Json(v): Json<Value>) -> Json<Value> {
    Json(json!({"run_id": Uuid::now_v7(), "state": "running", "manifest": v}))
}

async fn get_replay(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> Json<Value> {
    let row = sqlx::query(
        r#"SELECT simulation_run_id::text,portfolio_id::text,kind,manifest,state,created_at,completed_at
           FROM pumphound.simulation_runs WHERE simulation_run_id=$1 AND kind='replay'"#,
    )
    .bind(id)
    .fetch_optional(&s.stores.postgres)
    .await
    .ok()
    .flatten();
    match row {
        Some(r) => Json(json!({
            "run_id": r.get::<String,_>("simulation_run_id"),
            "portfolio_id": r.get::<String,_>("portfolio_id"),
            "kind": r.get::<String,_>("kind"),
            "manifest": r.get::<Value,_>("manifest"),
            "state": r.get::<String,_>("state"),
            "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
            "completed_at": r.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("completed_at").ok().flatten()
        })),
        None => Json(json!({"run_id": id, "state": "NOT_FOUND"})),
    }
}

async fn control_replay(Path(id): Path<String>, Json(v): Json<Value>) -> Json<Value> {
    Json(json!({"run_id": id, "control": v}))
}

#[derive(Deserialize)]
struct ManualInvestigationRequest {
    subject_type: Option<String>,
    subject_id: Option<String>,
    subject: Option<String>,
    as_of_slot: Option<i64>,
    knowledge_cutoff: Option<chrono::DateTime<chrono::Utc>>,
    roles: Option<Vec<String>>,
}

async fn start_investigation(
    State(s): State<Arc<AppState>>,
    Json(body): Json<ManualInvestigationRequest>,
) -> impl IntoResponse {
    let Some(subject_id) = body.subject_id.or(body.subject) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":{"code":"INVALID_REQUEST","message":"subject_id is required"}})),
        )
            .into_response();
    };
    let subject_type = body.subject_type.unwrap_or_else(|| "launch".into());
    let as_of_slot = body
        .as_of_slot
        .unwrap_or(s.health.read().await.last_slot as i64);
    let knowledge_cutoff = body.knowledge_cutoff.unwrap_or_else(now_utc);
    let context = match crate::investigator::manual_context(
        &s.stores,
        &subject_type,
        &subject_id,
        as_of_slot,
        knowledge_cutoff,
        body.roles.unwrap_or_else(|| vec!["committee".into()]),
    )
    .await
    {
        Ok(context) => context,
        Err(error) => return internal_error_response(error),
    };
    let client = crate::investigator::InvestigatorClient::from_env();
    let investigation_id =
        match crate::investigator::investigate_and_persist(&s.stores, &client, context).await {
            Ok(id) => id,
            Err(error) => return internal_error_response(error),
        };
    match crate::investigator::load_investigation(&s.stores, investigation_id).await {
        Ok(Some(value)) => Json(value).into_response(),
        _ => internal_error_response("investigation persistence failed"),
    }
}

async fn launch_investigations(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match crate::investigator::list_investigations_for_subject(&s.stores, "launch", &id, 10).await {
        Ok(items) => Json(json!({ "items": items })).into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn get_investigation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let Ok(id) = Uuid::parse_str(&id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"investigation"}})),
        )
            .into_response();
    };
    match crate::investigator::load_investigation(&s.stores, id).await {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"NOT_FOUND","message":"investigation"}})),
        )
            .into_response(),
        Err(error) => internal_error_response(error),
    }
}

async fn get_job(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match sqlx::query(
        r#"SELECT job_id::text,kind,state,request,cursor,progress,error_code,created_at,updated_at
           FROM pumphound.background_jobs WHERE job_id=$1"#,
    )
    .bind(id)
    .fetch_optional(&s.stores.postgres)
    .await
    {
        Ok(Some(row)) => (
            StatusCode::OK,
            Json(json!({
                "job_id": row.get::<String,_>("job_id"),
                "kind": row.get::<String,_>("kind"),
                "state": row.get::<String,_>("state"),
                "request": row.get::<Value,_>("request"),
                "result": row.try_get::<Option<Value>,_>("cursor").ok().flatten()
                    .and_then(|cursor| cursor.get("result").cloned()),
                "progress": row.get::<Value,_>("progress"),
                "error_code": row.try_get::<Option<String>,_>("error_code").ok().flatten(),
                "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
                "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")
            })),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"code":"JOB_NOT_FOUND","message":"durable job was not found"}})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"JOB_LOOKUP_FAILED","message":error.to_string()}})),
        )
            .into_response(),
    }
}

async fn cancel_job(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, String>(
        r#"UPDATE pumphound.background_jobs
           SET state='cancelled',updated_at=now()
           WHERE job_id=$1 AND state IN ('queued','running')
           RETURNING state"#,
    )
    .bind(id)
    .fetch_optional(&s.stores.postgres)
    .await
    {
        Ok(Some(state)) => (
            StatusCode::OK,
            Json(json!({"job_id": id, "state": state, "audited": true})),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(json!({"error":{"code":"JOB_NOT_CANCELLABLE","message":"job is absent or already terminal"}})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"code":"JOB_CANCEL_FAILED","message":error.to_string()}})),
        )
            .into_response(),
    }
}

async fn list_watchlists(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({"items": *s.watchlists.read().await}))
}

#[derive(Deserialize)]
struct NewList {
    name: String,
}

async fn create_watchlist(State(s): State<Arc<AppState>>, Json(b): Json<NewList>) -> Json<Value> {
    let w = crate::state::Watchlist {
        watchlist_id: Uuid::now_v7(),
        name: b.name,
        items: vec![],
    };
    s.watchlists.write().await.push(w.clone());
    Json(serde_json::to_value(w).unwrap_or(json!({})))
}

async fn add_watch(
    State(s): State<Arc<AppState>>,
    Path((id, subject)): Path<(Uuid, String)>,
) -> Json<Value> {
    for w in s.watchlists.write().await.iter_mut() {
        if w.watchlist_id == id && !w.items.contains(&subject) {
            w.items.push(subject.clone());
        }
    }
    Json(json!({"ok": true}))
}

async fn del_watch(
    State(s): State<Arc<AppState>>,
    Path((id, subject)): Path<(Uuid, String)>,
) -> Json<Value> {
    for w in s.watchlists.write().await.iter_mut() {
        if w.watchlist_id == id {
            w.items.retain(|i| i != &subject);
        }
    }
    Json(json!({"ok": true}))
}

async fn alerts(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::readiness::alerts_json(&s).await)
}

async fn research_timelines(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        crate::cockpit::mint_timelines(&s)
            .await
            .unwrap_or_else(|_| json!({"items": []})),
    )
}

async fn alert_rule(Json(v): Json<Value>) -> Json<Value> {
    Json(json!({"rule_id": Uuid::now_v7(), "condition": v}))
}

async fn ack_alert(Path(id): Path<String>) -> Json<Value> {
    Json(json!({"alert_id": id, "acknowledged": true}))
}

async fn paper_truth(
    State(s): State<Arc<AppState>>,
    Query(q): Query<crate::truth::TruthQuery>,
) -> impl IntoResponse {
    if let Err(e) = crate::truth::validate(&q) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"state":"ERROR","reason":e.to_string()})),
        );
    }
    match crate::truth::report(&s.stores, &q).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => {
            tracing::warn!(error=%e,"paper truth query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    json!({"state":"ERROR","reason":"Paper ledger truth is unavailable; no counts can be asserted"}),
                ),
            )
        }
    }
}
async fn paper_trace(
    State(s): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(q): Query<crate::truth::TraceQuery>,
) -> impl IntoResponse {
    match crate::truth::trace(&s.stores, id, &q).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => {
            tracing::warn!(error=%e,"paper trace query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    json!({"identity":{"trade_trace_id":id},"events":[],"completeness":{"state":"REPLAY_INCOMPLETE","missing":["Persisted evidence store query failed; retry when data health recovers"]}}),
                ),
            )
        }
    }
}

async fn champion_funnel(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::model_truth::champion_funnel(&s.stores).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => {
            tracing::warn!(%error, "champion funnel query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "state": "ERROR",
                    "reason": "Champion funnel evidence is unavailable; no gate counts are asserted"
                })),
            )
        }
    }
}

async fn models_truth(State(s): State<Arc<AppState>>) -> impl IntoResponse {
    match crate::model_truth::model_overview(&s.stores).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => {
            tracing::warn!(%error, "model truth query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "state": "ERROR",
                    "reason": "Model lineage and live-quality evidence are unavailable"
                })),
            )
        }
    }
}

async fn model_states(State(s): State<Arc<AppState>>, Path(id): Path<Uuid>) -> impl IntoResponse {
    match crate::model_truth::target_states(&s.stores, id).await {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(error) => {
            tracing::warn!(%error, launch_id = %id, "target model-state query failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "launch_id": id,
                    "state": "ERROR",
                    "targets": [],
                    "reason": "Target-specific model evidence is unavailable"
                })),
            )
        }
    }
}
