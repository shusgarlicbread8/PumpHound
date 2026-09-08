use crate::state::AppState;
use serde_json::{json, Value};
use sqlx::Row;

const LEARN: &str = pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID;

pub async fn home(state: &AppState) -> anyhow::Result<Value> {
    let readiness = crate::readiness::as_json(state).await;
    let hot = crate::hot_set::report(&state.stores)
        .await
        .unwrap_or_else(|_| json!({"active_count": 0, "members": [], "recent_events": []}));
    let strategies = crate::paper_ledger::strategy_reports(&state.stores)
        .await
        .unwrap_or_else(|_| json!({"items": []}));
    let learning = learning_slice(state).await.unwrap_or_else(|_| json!({}));
    let portfolio = portfolio_slice(state).await.unwrap_or_else(|error| {
        tracing::warn!(%error, "home portfolio truth summary failed");
        json!({
            "status": "UNAVAILABLE",
            "reason": error.to_string(),
            "open_positions": null,
            "closed_positions_today": null,
            "realized_pnl": null
        })
    });
    let opportunities = opportunity_feed(state).await.unwrap_or_default();
    let activity = away_feed(state).await.unwrap_or_default();
    let alerts = crate::readiness::alerts_json(state).await;
    let health = state.health.read().await;
    let now = pumphound_domain::now_utc();
    Ok(json!({
        "generated_at": now,
        "paper_only": true,
        "automatic_promotion": false,
        "mode": state.mode,
        "source": state.source_label,
        "header": {
            "mode": state.mode,
            "chain": crate::readiness::display_chain(&health.chain, health.last_event_at, now),
            "last_slot": health.last_slot.to_string(),
            "last_event_at": health.last_event_at,
            "lag_ms": health.lag_ms,
            "idle_ms": crate::readiness::idle_ms(health.last_event_at, now),
            "decoded": health.decoded,
            "quarantined": health.quarantined
        },
        "readiness": readiness,
        "headline": readiness.get("headline").cloned(),
        "alerts": alerts.get("items").cloned().unwrap_or_else(|| json!([])),
        "portfolio": portfolio,
        "strategies": strategies.get("items").cloned().unwrap_or_else(|| json!([])),
        "opportunities": opportunities,
        "learning": learning,
        "activity": activity,
        "hot_set": hot,
        "disclaimers": [
            "All capital is simulated. No live signing.",
            "Strategy status UNPROVEN unless closed_trades >= 30.",
            "AI is advisory unless an experiment reports a measured lift."
        ]
    }))
}

async fn portfolio_slice(state: &AppState) -> anyhow::Result<Value> {
    // Home consumes the same canonical active-book projection as Trade. It must
    // not independently reinterpret running/archived book scope.
    let truth = crate::truth::report(
        &state.stores,
        &crate::truth::TruthQuery {
            summary_only: true,
            ..Default::default()
        },
    )
    .await?;
    let summary = truth.get("summary").cloned().unwrap_or_else(|| json!({}));
    let scope = truth.get("scope").cloned().unwrap_or_else(|| json!({}));
    let labels_today: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM pumphound.outcome_labels
           WHERE status='MATURED' AND matured_at >= date_trunc('day', now())"#,
    )
    .fetch_one(&state.stores.postgres)
    .await
    .unwrap_or(0);
    let launches_today: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM pumphound.launches l
           WHERE known_at >= date_trunc('day', now())
             AND COALESCE(l.attributes->>'integrity_state','')<>'QUARANTINED'
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.entity_type='LAUNCH' AND iq.entity_id=l.launch_id::text AND iq.status='ACTIVE'
             )"#,
    )
    .fetch_one(&state.stores.postgres)
    .await
    .unwrap_or(0);
    Ok(json!({
        "open_positions": summary.get("open_positions"),
        "closed_positions": summary.get("closed_positions"),
        "closed_positions_today": summary.get("closed_positions_today"),
        "closed_trades": summary.get("closed_trades"),
        "closed_trades_today": summary.get("closed_trades_today"),
        "closed_trade_basis": summary.get("closed_trade_basis"),
        "realized_pnl": summary.get("realized_pnl"),
        "realized_pnl_quote_atomic": summary.get("realized_pnl_quote_atomic"),
        "realized_pnl_by_asset": summary.get("realized_pnl_by_asset"),
        "unrealized_pnl": summary.get("unrealized_pnl"),
        "unrealized_pnl_state": summary.get("unrealized_pnl_state"),
        "integrity_quarantined_positions": summary.get("integrity_quarantined_positions"),
        "integrity_quarantined_orders": summary.get("integrity_quarantined_orders"),
        "labels_matured_today": labels_today,
        "launches_observed_today": launches_today,
        "capital": "SIMULATED",
        "equity_status": "PARTIAL",
        "scope": scope
    }))
}

async fn learning_slice(state: &AppState) -> anyhow::Result<Value> {
    let matured: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.outcome_labels WHERE status='MATURED'")
            .fetch_one(&state.stores.postgres)
            .await
            .unwrap_or(0);
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM pumphound.outcome_labels WHERE status='PENDING'")
            .fetch_one(&state.stores.postgres)
            .await
            .unwrap_or(0);
    let models = sqlx::query(
        r#"SELECT model_version,target_version,status,promoted_at,created_at
           FROM pumphound.model_registry ORDER BY created_at DESC LIMIT 12"#,
    )
    .fetch_all(&state.stores.postgres)
    .await
    .unwrap_or_default();
    let transitions = sqlx::query(
        r#"SELECT model_version,target_version,from_status,to_status,reason,occurred_at
           FROM pumphound.model_transition_events ORDER BY occurred_at DESC LIMIT 8"#,
    )
    .fetch_all(&state.stores.postgres)
    .await
    .unwrap_or_default();
    let jobs = sqlx::query(
        r#"SELECT kind,state,created_at FROM pumphound.background_jobs
           WHERE kind IN ('label_maturation','adaptive_retrain')
           ORDER BY created_at DESC LIMIT 8"#,
    )
    .fetch_all(&state.stores.postgres)
    .await
    .unwrap_or_default();
    Ok(json!({
        "labels_matured": matured,
        "labels_pending": pending,
        "models": models.iter().map(|row| json!({
            "model_version": row.get::<String,_>("model_version"),
            "target_version": row.get::<String,_>("target_version"),
            "status": row.get::<String,_>("status").to_uppercase(),
            "promoted_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("promoted_at").ok().flatten(),
            "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
        })).collect::<Vec<_>>(),
        "transitions": transitions.iter().map(|row| json!({
            "model_version": row.get::<String,_>("model_version"),
            "target_version": row.get::<String,_>("target_version"),
            "from_status": row.try_get::<Option<String>,_>("from_status").ok().flatten(),
            "to_status": row.get::<String,_>("to_status"),
            "reason": row.get::<String,_>("reason"),
            "occurred_at": row.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at")
        })).collect::<Vec<_>>(),
        "jobs": jobs.iter().map(|row| json!({
            "kind": row.get::<String,_>("kind"),
            "state": row.get::<String,_>("state"),
            "created_at": row.get::<chrono::DateTime<chrono::Utc>,_>("created_at")
        })).collect::<Vec<_>>()
    }))
}

async fn opportunity_feed(state: &AppState) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(
        r#"SELECT d.decision_id::text, d.launch_id::text, d.action, d.heuristic_version,
                  d.decision_at, d.reason_codes, d.exact_inputs, l.mint, l.creator, l.creation_time
           FROM pumphound.decisions d
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           WHERE d.action IN ('BUY','WATCH')
             AND COALESCE(l.attributes->>'integrity_state','')<>'QUARANTINED'
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.entity_type='LAUNCH' AND iq.entity_id=l.launch_id::text AND iq.status='ACTIVE'
             )
             AND (d.exact_inputs->>'experiment_id' = $1 OR d.heuristic_version IN
                  ('HEURISTIC_V1','INFORMATION_VALUE_EXPLORER_V1','CHAMPION_SHORT_HORIZON_V1','RANDOM_CONTROL_V1'))
           ORDER BY d.decision_at DESC LIMIT 20"#,
    )
    .bind(LEARN)
    .fetch_all(&state.stores.postgres)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let inputs: Value = row.get("exact_inputs");
            json!({
                "decision_id": row.get::<String,_>("decision_id"),
                "launch_id": row.get::<String,_>("launch_id"),
                "mint": row.get::<String,_>("mint"),
                "creator": row.get::<String,_>("creator"),
                "action": row.get::<String,_>("action"),
                "strategy": row.get::<String,_>("heuristic_version"),
                "decision_at": row.get::<chrono::DateTime<chrono::Utc>,_>("decision_at"),
                "reasons": row.get::<Value,_>("reason_codes"),
                "age_seconds": inputs.pointer("/feature_snapshot/values/age_seconds"),
                "momentum": inputs.pointer("/learning_evidence/champion/momentum")
                    .or_else(|| inputs.pointer("/learning_evidence/shadow_prediction/probabilities/short_momentum_2m_v1")),
                "adverse": inputs.pointer("/learning_evidence/champion/adverse")
                    .or_else(|| inputs.pointer("/learning_evidence/shadow_prediction/probabilities/adverse_move_2m_v1")),
                "expected_mfe": inputs.pointer("/learning_evidence/champion/expected_mfe"),
                "creation_time": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("creation_time").ok().flatten()
            })
        })
        .collect())
}

async fn away_feed(state: &AppState) -> anyhow::Result<Vec<Value>> {
    let mut items = Vec::new();
    let positions = sqlx::query(
        r#"SELECT pos.position_id::text, pos.status, pos.opened_at, pos.closed_at,
                  pos.realized_pnl_quote_atomic::text, l.mint, r.manifest->>'strategy' AS strategy
           FROM pumphound.paper_positions pos
           JOIN pumphound.launches l ON l.launch_id=pos.launch_id
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=pos.simulation_run_id
           WHERE NOT EXISTS (
             SELECT 1 FROM pumphound.entity_integrity_quarantines iq
             WHERE iq.status='ACTIVE' AND (
               (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
               OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
             )
           )
           ORDER BY COALESCE(pos.closed_at, pos.opened_at) DESC LIMIT 12"#,
    )
    .fetch_all(&state.stores.postgres)
    .await
    .unwrap_or_default();
    for row in positions {
        items.push(json!({
            "kind": if row.get::<String,_>("status") == "CLOSED" { "POSITION_CLOSED" } else { "POSITION_OPENED" },
            "at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("closed_at").ok().flatten()
                .or(row.try_get("opened_at").ok()),
            "mint": row.get::<String,_>("mint"),
            "strategy": row.try_get::<Option<String>,_>("strategy").ok().flatten(),
            "pnl": row.get::<String,_>("realized_pnl_quote_atomic")
        }));
    }
    let promotions = sqlx::query(
        r#"SELECT model_version,target_version,to_status,reason,occurred_at
           FROM pumphound.model_transition_events
           WHERE to_status IN ('CHAMPION','SHADOW','RETIRED')
           ORDER BY occurred_at DESC LIMIT 6"#,
    )
    .fetch_all(&state.stores.postgres)
    .await
    .unwrap_or_default();
    for row in promotions {
        items.push(json!({
            "kind": "MODEL_TRANSITION",
            "at": row.get::<chrono::DateTime<chrono::Utc>,_>("occurred_at"),
            "model_version": row.get::<String,_>("model_version"),
            "target_version": row.get::<String,_>("target_version"),
            "to_status": row.get::<String,_>("to_status"),
            "reason": row.get::<String,_>("reason")
        }));
    }
    items.sort_by(|a, b| {
        b.get("at")
            .and_then(Value::as_str)
            .cmp(&a.get("at").and_then(Value::as_str))
    });
    items.truncate(16);
    Ok(items)
}

pub async fn wallet_index(state: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT wallet, launches_entered, first10_appearances, buys, sells,
                  creator_linked_launches, behaviour, pnl_status, last_seen_slot, updated_at
           FROM pumphound.wallet_profiles
           ORDER BY last_seen_slot DESC NULLS LAST LIMIT 80"#,
    )
    .fetch_all(&state.stores.postgres)
    .await?;
    Ok(json!({
        "items": rows.iter().map(|row| json!({
            "wallet": row.get::<String,_>("wallet"),
            "launches_entered": row.get::<i32,_>("launches_entered"),
            "early_appearances": row.get::<i32,_>("first10_appearances"),
            "buys": row.get::<i32,_>("buys"),
            "sells": row.get::<i32,_>("sells"),
            "creator_linked_launches": row.get::<i32,_>("creator_linked_launches"),
            "behaviour": row.try_get::<Option<String>,_>("behaviour").ok().flatten(),
            "pnl_status": row.get::<String,_>("pnl_status"),
            "last_seen_slot": row.get::<i64,_>("last_seen_slot").to_string(),
            "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")
        })).collect::<Vec<_>>(),
        "fabricated": false
    }))
}

pub async fn creator_index(state: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT creator, launches, connected_buyers,
                  graduation_rate::double precision AS graduation_rate,
                  collapse_rate::double precision AS collapse_rate,
                  pnl_status, last_seen_slot, updated_at
           FROM pumphound.creator_profiles
           ORDER BY last_seen_slot DESC NULLS LAST LIMIT 80"#,
    )
    .fetch_all(&state.stores.postgres)
    .await?;
    Ok(json!({
        "items": rows.iter().map(|row| json!({
            "creator": row.get::<String,_>("creator"),
            "launches": row.get::<i32,_>("launches"),
            "connected_buyers": row.get::<i32,_>("connected_buyers"),
            "graduation_rate": row.try_get::<Option<f64>,_>("graduation_rate").ok().flatten(),
            "collapse_rate": row.try_get::<Option<f64>,_>("collapse_rate").ok().flatten(),
            "pnl_status": row.get::<String,_>("pnl_status"),
            "last_seen_slot": row.try_get::<Option<i64>,_>("last_seen_slot").ok().flatten().map(|s| s.to_string()),
            "updated_at": row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")
        })).collect::<Vec<_>>(),
        "fabricated": false
    }))
}

fn quote_sol(atomic: Option<&str>) -> Value {
    match atomic {
        Some(raw) if !raw.is_empty() && raw != "NULL" => {
            let signed = raw.parse::<i128>().unwrap_or(0);
            json!({
                "atomic": raw,
                "decimals": 9,
                "symbol": "SOL",
                "asset_mint": pumphound_domain::WSOL_MINT,
                "display": format_signed_sol(signed),
                "usd": Value::Null,
                "usd_status": "UNAVAILABLE"
            })
        }
        _ => json!({
            "status": "UNKNOWN",
            "symbol": "SOL",
            "usd": Value::Null,
            "usd_status": "UNAVAILABLE"
        }),
    }
}

fn format_signed_sol(atomic: i128) -> String {
    let neg = atomic < 0;
    let abs = atomic.unsigned_abs();
    let whole = abs / 1_000_000_000;
    let frac = abs % 1_000_000_000;
    let mut frac_s = format!("{frac:09}");
    while frac_s.ends_with('0') && frac_s.len() > 1 {
        frac_s.pop();
    }
    if frac_s == "0" {
        format!("{}{whole} SOL", if neg { "−" } else { "+" })
    } else {
        format!("{}{whole}.{frac_s} SOL", if neg { "−" } else { "+" })
    }
}

pub async fn mint_timelines(state: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT l.mint, l.launch_id::text, l.creation_slot, l.creation_time, l.known_at,
                  (SELECT MIN(f.known_at) FROM pumphound.graph_facts f
                    WHERE f.asset_mint=l.mint AND f.edge_type IN ('BOUGHT','SOLD')) AS first_trade_at,
                  (SELECT MIN(e.first_observed_at) FROM pumphound.candidate_lifecycle_events e
                    WHERE e.launch_id=l.launch_id) AS first_lifecycle_observed_at,
                  (SELECT MIN(e.snapshot_at) FROM pumphound.candidate_lifecycle_events e
                    WHERE e.launch_id=l.launch_id AND e.snapshot_at IS NOT NULL) AS first_lifecycle_snapshot_at,
                  (SELECT MIN(d.decision_at) FROM pumphound.decisions d
                    WHERE d.launch_id=l.launch_id) AS first_decision_at
           FROM pumphound.launches l
           ORDER BY l.known_at DESC
           LIMIT 30"#,
    )
    .fetch_all(&state.stores.postgres)
    .await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(_) => {
            sqlx::query(
                r#"SELECT mint, launch_id::text, creation_slot, creation_time, known_at
                   FROM pumphound.launches ORDER BY known_at DESC LIMIT 30"#,
            )
            .fetch_all(&state.stores.postgres)
            .await?
        }
    };
    let now = pumphound_domain::now_utc();
    Ok(json!({
        "generated_at": now,
        "note": "Individual mint clocks. Chain create time is null when logs-only scout has no block_time. Age uses known_at in that case and is labelled first_observed.",
        "items": rows.iter().map(|row| {
            let created = row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("creation_time").ok().flatten();
            let known = row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("known_at").ok().flatten();
            let age = crate::launch_age::launch_age(created, known, now);
            let first_obs = row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("first_lifecycle_observed_at").ok().flatten();
            let first_snap = row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("first_lifecycle_snapshot_at").ok().flatten();
            let observe_to_snapshot_ms = match (first_obs.or(known), first_snap) {
                (Some(a), Some(b)) if b >= a => Some((b - a).num_milliseconds()),
                _ => None
            };
            json!({
                "mint": row.get::<String,_>("mint"),
                "launch_id": row.get::<String,_>("launch_id"),
                "creation_slot": row.get::<i64,_>("creation_slot"),
                "creation_time": created,
                "known_at": known,
                "first_trade_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("first_trade_at").ok().flatten(),
                "first_snapshot_at": first_snap,
                "first_decision_at": row.try_get::<Option<chrono::DateTime<chrono::Utc>>,_>("first_decision_at").ok().flatten(),
                "age_seconds": age.seconds,
                "age_basis": age.basis,
                "observe_to_snapshot_ms": observe_to_snapshot_ms
            })
        }).collect::<Vec<_>>()
    }))
}
