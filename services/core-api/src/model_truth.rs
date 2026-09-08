//! Read projections and additive audit for target-specific model evidence.
use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

pub fn availability(
    role: Option<&str>,
    prediction: Option<&Value>,
    missing: &[String],
    schema_matches: bool,
) -> Value {
    let (state, reason) = if role.is_none() {
        (
            "NO_MODEL",
            "No active registered model for this target".to_string(),
        )
    } else if !schema_matches {
        (
            "SCHEMA_MISMATCH",
            "Feature schema does not match model schema".into(),
        )
    } else if let Some(reason) = prediction
        .and_then(|p| p.get("abstention_reason"))
        .and_then(Value::as_str)
    {
        if reason.contains("familiarity") || reason.contains("ood") {
            ("OOD", reason.into())
        } else if reason.contains("missing_required") || reason.contains("feature") {
            ("FEATURES_NOT_READY", reason.into())
        } else if reason.contains("error") || reason.contains("timeout") {
            ("MODEL_ERROR", reason.into())
        } else {
            ("PREDICTION_WITHHELD", reason.into())
        }
    } else if !missing.is_empty() {
        (
            "FEATURES_NOT_READY",
            format!("Required features missing: {}", missing.join(", ")),
        )
    } else if prediction
        .and_then(|p| p.get("score"))
        .and_then(Value::as_f64)
        .is_some()
    {
        (
            "PREDICTION_AVAILABLE",
            "Persisted score for this model and feature snapshot".into(),
        )
    } else {
        (
            "MODEL_AVAILABLE_TOKEN_NOT_SCOREABLE",
            "No persisted prediction for this model on the latest token snapshot".into(),
        )
    };
    json!({"state":state,"reason":reason,"missing_features":missing})
}

pub fn evaluation_metrics(metrics: &Value, manifest: &Value) -> Value {
    if metrics.as_object().is_some_and(|m| !m.is_empty()) {
        metrics.clone()
    } else {
        manifest.get("metrics").cloned().unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_model_is_distinct_from_waiting_features() {
        assert_eq!(
            availability(None, None, &["return_30".into()], true)["state"],
            "NO_MODEL"
        );
        assert_eq!(
            availability(Some("champion"), None, &["return_30".into()], true)["state"],
            "FEATURES_NOT_READY"
        );
    }
    #[test]
    fn ood_is_not_a_healthy_available_prediction() {
        let p = json!({"score":0.7,"abstention_reason":"familiarity_below_threshold:0.2<0.65"});
        assert_eq!(
            availability(Some("shadow"), Some(&p), &[], true)["state"],
            "OOD"
        );
    }
    #[test]
    fn nested_champion_evaluation_is_exposed() {
        let metrics = json!({"held_out":{"n":1102,"roc_auc":0.465},"validation":{"n":500}});
        assert_eq!(
            evaluation_metrics(&metrics, &json!({}))["held_out"]["n"],
            1102
        );
        assert_eq!(
            evaluation_metrics(&json!({}), &json!({"metrics":metrics}))["held_out"]["n"],
            1102
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn record_funnel(
    stores: &Stores,
    key: &str,
    book: Uuid,
    launch: Option<Uuid>,
    snapshot: Option<&str>,
    phase: &str,
    reason: &str,
    gates: &Value,
    evidence: &Value,
) -> anyhow::Result<()> {
    sqlx::query(r#"INSERT INTO pumphound.champion_funnel_events
        (event_id,idempotency_key,book_id,launch_id,feature_snapshot_id,phase,reason_code,gates,evidence)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (idempotency_key) DO NOTHING"#)
        .bind(Uuid::now_v7()).bind(key).bind(book).bind(launch).bind(snapshot)
        .bind(phase).bind(reason).bind(gates).bind(evidence).execute(&stores.postgres).await?;
    Ok(())
}

pub async fn champion_funnel(stores: &Stores) -> anyhow::Result<Value> {
    let event_summary: Value = sqlx::query_scalar(
        r#"SELECT jsonb_build_object(
        'first_instrumented_at',min(occurred_at),'latest_at',max(occurred_at),'events_n',count(*),
        'launches_n',count(DISTINCT launch_id),'snapshot_n',count(DISTINCT feature_snapshot_id))
        FROM pumphound.champion_funnel_events"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let phases: Vec<Value> = sqlx::query_scalar(
        r#"SELECT jsonb_build_object('phase',phase,'reason',reason_code,
        'n',count(*),'launches_n',count(DISTINCT launch_id)) FROM pumphound.champion_funnel_events
        GROUP BY phase,reason_code ORDER BY phase,count(*) DESC"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let gates: Vec<Value> = sqlx::query_scalar(
        r#"SELECT jsonb_build_object('gate',g.key,
        'passed_n',count(*) FILTER(WHERE g.value='true'::jsonb),
        'failed_n',count(*) FILTER(WHERE g.value='false'::jsonb),
        'unknown_n',count(*) FILTER(WHERE g.value='null'::jsonb),'evaluation_n',count(*))
        FROM pumphound.champion_funnel_events e CROSS JOIN LATERAL jsonb_each(e.gates) g
        WHERE e.phase='EVALUATION' GROUP BY g.key ORDER BY g.key"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let rejections: Vec<Value> = sqlx::query_scalar(r#"SELECT jsonb_build_object('reason',reason,'n',count(*))
        FROM pumphound.decisions d CROSS JOIN LATERAL jsonb_array_elements_text(d.reason_codes) reason
        WHERE d.heuristic_version='CHAMPION_SHORT_HORIZON_V1' AND d.decision_kind='ENTRY'
        GROUP BY reason ORDER BY count(*) DESC"#).fetch_all(&stores.postgres).await?;
    let ledger: Value = sqlx::query_scalar(r#"WITH d AS (
        SELECT decision_id,simulation_run_id,launch_id FROM pumphound.decisions
        WHERE heuristic_version='CHAMPION_SHORT_HORIZON_V1' AND decision_kind='ENTRY'),
        o AS (SELECT o.* FROM pumphound.paper_orders o JOIN d ON d.decision_id=o.decision_id)
        SELECT jsonb_build_object('decisions_n',(SELECT count(*) FROM d),
          'buy_n',(SELECT count(*) FROM pumphound.decisions WHERE heuristic_version='CHAMPION_SHORT_HORIZON_V1' AND action='BUY' AND decision_kind='ENTRY'),
          'orders_created_n',(SELECT count(*) FROM o),
          'orders_arrived_n',(SELECT count(*) FROM o WHERE arrival_snapshot_id IS NOT NULL),
          'fills_n',(SELECT count(*) FROM pumphound.paper_fills f JOIN o ON o.order_id=f.order_id),
          'positions_opened_n',(SELECT count(DISTINCT p.position_id)
            FROM pumphound.paper_positions p JOIN d
              ON d.simulation_run_id=p.simulation_run_id AND d.launch_id=p.launch_id
            JOIN o ON o.decision_id=d.decision_id
            JOIN pumphound.paper_fills f ON f.order_id=o.order_id),
          'positions_closed_n',(SELECT count(DISTINCT p.position_id)
            FROM pumphound.paper_positions p JOIN d
              ON d.simulation_run_id=p.simulation_run_id AND d.launch_id=p.launch_id
            JOIN o ON o.decision_id=d.decision_id
            JOIN pumphound.paper_fills f ON f.order_id=o.order_id
            WHERE p.status='CLOSED'))"#)
        .fetch_one(&stores.postgres).await?;
    let latest: Option<Value> = sqlx::query_scalar(
        r#"SELECT to_jsonb(e) FROM pumphound.champion_funnel_events e
        WHERE phase='READINESS' ORDER BY occurred_at DESC LIMIT 1"#,
    )
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(
        json!({"scope":{"strategy":"CHAMPION_SHORT_HORIZON_V1","experiment_id":"ALPHA-LEARN","window":"all_persisted_history"},
        "instrumentation":event_summary,"phases":phases,"gates":gates,"historical_rejection_reasons":rejections,
        "ledger":ledger,"latest_readiness":latest,"gate_count_semantics":"Independent pass/fail counts per persisted candidate age-bucket evaluation; not a sequential conversion funnel",
        "historical_limitations":["Gate-level evidence is only available since instrumentation began; no backfilled or fabricated historical gates", "Readiness counts are minute samples; snapshot and deduplication counts are per launch per minute"]}),
    )
}

pub async fn model_overview(stores: &Stores) -> anyhow::Result<Value> {
    let rows = sqlx::query(r#"SELECT r.*,to_jsonb(ds) AS dataset,
        (SELECT jsonb_build_object('prediction_n',count(*),'latest_at',max(p.computed_at),
          'withheld_n',count(*) FILTER(WHERE p.abstention_reason IS NOT NULL),
          'mean_familiarity',avg(p.familiarity),'low_familiarity_n',count(*) FILTER(WHERE p.familiarity<0.5),
          'schema_mismatch_n',count(*) FILTER(WHERE f.schema_version<>r.feature_schema_version),
          'latest_feature_cutoff',max(f.knowledge_cutoff),
          'mean_serving_missingness',avg((SELECT count(*)::float / NULLIF(jsonb_array_length(r.manifest->'feature_names'),0)
            FROM jsonb_array_elements_text(COALESCE(r.manifest->'feature_names','[]'::jsonb)) name
            WHERE f.values->name IS NULL OR f.values->name='null'::jsonb)))
         FROM pumphound.model_predictions p LEFT JOIN pumphound.feature_snapshots f USING(feature_snapshot_id)
         WHERE p.model_version=r.model_version AND p.computed_at>now()-interval '24 hours') AS live,
        (SELECT count(*) FROM pumphound.decisions d
          WHERE d.model_versions @> jsonb_build_object(r.target_version,r.model_version)
             OR d.exact_inputs#>'{learning_evidence,champion,model_versions}'
                  @> jsonb_build_object(r.target_version,r.model_version)) AS decisions_n
        FROM pumphound.model_registry r LEFT JOIN pumphound.dataset_snapshots ds
          ON ds.content_sha256=r.manifest->>'data_manifest_sha256'
        ORDER BY r.created_at DESC"#).fetch_all(&stores.postgres).await?;
    let items: Vec<Value> = rows.iter().map(|r| {
        let manifest: Value=r.get("manifest"); let metrics=evaluation_metrics(&r.get::<Value,_>("metrics"),&manifest);
        let live:Value=r.get("live"); let role:String=r.get("status");
        let state=if live["schema_mismatch_n"].as_i64().unwrap_or(0)>0 {"SCHEMA_MISMATCH"}
            else if live["prediction_n"].as_i64().unwrap_or(0)==0 {"INSUFFICIENT_LIVE_EVIDENCE"}
            else if live["mean_serving_missingness"].as_f64().unwrap_or(0.0)>0.5 {"DATA_QUALITY_BLOCKED"}
            else if live["low_familiarity_n"].as_i64().unwrap_or(0)*2
                > live["prediction_n"].as_i64().unwrap_or(0) {"MODEL_DEGRADED"}
            else {"MODEL_READY"};
        json!({"target":r.get::<String,_>("target_version"),"model_version":r.get::<String,_>("model_version"),
            "role":role.to_uppercase(),"state":state,"feature_schema_version":r.get::<String,_>("feature_schema_version"),
            "feature_names":manifest["feature_names"],"training_rows":manifest.pointer("/sample_counts/train/n"),
            "sample_counts":manifest["sample_counts"],"dataset":r.get::<Option<Value>,_>("dataset"),
            "dataset_hash":manifest["data_manifest_sha256"],"training_manifest_hash":manifest["training_manifest_sha256"],
            "artifacts":manifest["artifacts"],"offline":metrics,
            "evaluation_state":if metrics.is_null(){"EVALUATION_MISSING"}else{"EVALUATION_AVAILABLE"},
            "live":live,"live_window":"last_24_hours","live_calibration":null,"drift":null,
            "live_limitations":["Calibration and drift have no verified aggregate here; unavailable, not zero", "Persisted scores are a selected subset; missingness does not describe unscored market candidates"],
            "paper_decisions_referencing_version":r.get::<i64,_>("decisions_n"),
            "promotion":{"role":role,"gate_failures":manifest["champion_gate_failures"],"artifact_validation":manifest["artifact_validation"]}})
    }).collect();
    Ok(json!({"items":items,"generated_at":pumphound_domain::now_utc(),"paper_only":true}))
}

pub async fn target_states(stores: &Stores, launch_id: Uuid) -> anyhow::Result<Value> {
    let feature: Option<Value> = sqlx::query_scalar("SELECT to_jsonb(f) FROM pumphound.feature_snapshots f WHERE launch_id=$1 ORDER BY knowledge_cutoff DESC,computed_at DESC LIMIT 1")
        .bind(launch_id).fetch_optional(&stores.postgres).await?;
    let rows = sqlx::query(r#"SELECT DISTINCT ON (target_version) target_version,model_version,status,manifest,feature_schema_version
        FROM pumphound.model_registry WHERE status IN ('champion','shadow','challenger','candidate')
        ORDER BY target_version,CASE status WHEN 'champion' THEN 0 WHEN 'shadow' THEN 1 ELSE 2 END,created_at DESC"#)
        .fetch_all(&stores.postgres).await?;
    let mut targets = std::collections::BTreeMap::<String, Value>::new();
    for target in [
        "short_momentum_2m_v1",
        "adverse_move_2m_v1",
        "expected_mfe_5m_v1",
        "exit_hazard_60s_v1",
        "graduated_by_6h_v1",
        "severe_collapse_1h_v1",
        "up_50_before_down_20_15m_v1",
    ] {
        targets.insert(target.into(),json!({"target":target,"role":null,"model_version":null,"state":"NO_MODEL","reason":"No active registered model for this target","score":null}));
    }
    for row in rows {
        let target: String = row.get("target_version");
        let version: String = row.get("model_version");
        let role: String = row.get("status");
        let manifest: Value = row.get("manifest");
        let prediction:Option<Value>=sqlx::query_scalar(r#"SELECT jsonb_build_object('score',outputs->$3->'p','abstention_reason',abstention_reason,
            'prediction_id',prediction_id,'feature_snapshot_id',feature_snapshot_id,'computed_at',computed_at,'familiarity',familiarity)
            FROM pumphound.model_predictions WHERE launch_id=$1 AND model_version=$2 ORDER BY computed_at DESC LIMIT 1"#)
            .bind(launch_id).bind(&version).bind(&target).fetch_optional(&stores.postgres).await?;
        let feature_id = feature
            .as_ref()
            .and_then(|f| f["feature_snapshot_id"].as_str());
        let current = prediction
            .as_ref()
            .filter(|p| p["feature_snapshot_id"].as_str() == feature_id);
        let missing: Vec<String> = manifest["feature_names"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| {
                feature
                    .as_ref()
                    .and_then(|f| f["values"].get(*name))
                    .map_or(true, Value::is_null)
            })
            .map(str::to_owned)
            .collect();
        let schema_ok = feature.as_ref().map_or(true, |f| {
            f["schema_version"].as_str()
                == Some(row.get::<String, _>("feature_schema_version").as_str())
        });
        let mut item = availability(Some(&role), current, &missing, schema_ok);
        item["target"] = json!(target);
        item["role"] = json!(role.to_uppercase());
        item["model_version"] = json!(version);
        item["score"] = if item["state"] == "PREDICTION_AVAILABLE" {
            current.map(|p| p["score"].clone()).unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        item["latest_persisted_prediction"] = json!(prediction);
        item["feature_snapshot_id"] = json!(feature_id);
        item["feature_knowledge_cutoff"] = feature
            .as_ref()
            .map(|f| f["knowledge_cutoff"].clone())
            .unwrap_or(Value::Null);
        targets.insert(target, item);
    }
    Ok(
        json!({"launch_id":launch_id,"targets":targets.into_values().collect::<Vec<_>>(),"generated_at":pumphound_domain::now_utc()}),
    )
}
