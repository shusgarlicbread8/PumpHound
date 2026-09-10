//! Research reads for picker-mode: Strategy Diff, Information Value forensics,
//! and EXEC-SELECT matched comparison. No fabrication.

use crate::state::AppState;
use pumphound_paper_core::{
    strategy_diff, strategy_manifest, EXEC_SELECT_EXPERIMENT_ID, STRATEGY_FULL_ML_SHADOW,
    STRATEGY_INFORMATION_VALUE_EXPLORER_V1, STRATEGY_MARKET_ONLY_SHADOW, STRATEGY_RANDOM_CONTROL_V1,
};
use serde_json::{json, Value};
use sqlx::Row;

const FORENSIC_FEATURES: &[&str] = &[
    "age_seconds",
    "unique_buyers_30",
    "buy_count_30",
    "sell_count_30",
    "buy_sell_imbalance_30",
    "return_30",
    "curve_progress",
    "holder_top_1_share",
    "creator_risk",
    "same_slot_first10_share",
    "same_funder_first10_count",
    "entry_impact_bps",
    "round_trip_cost_fraction",
];

pub async fn strategy_diff_value(a: &str, b: &str) -> Value {
    let diff = strategy_diff(a, b);
    json!({
        "a": strategy_manifest(a),
        "b": strategy_manifest(b),
        "status": diff.status,
        "same": diff.same,
        "different": diff.different,
        "confounders": diff.confounders,
        "note": "COMPARABLE means simulator, size, timing, exit and fee assumptions match. Selection differences are the intended treatment.",
        "fabricated": false
    })
}

pub fn catalog_value() -> Value {
    let keys = [
        STRATEGY_RANDOM_CONTROL_V1,
        STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
        STRATEGY_MARKET_ONLY_SHADOW,
        STRATEGY_FULL_ML_SHADOW,
        "RANDOM_EXECUTION_CONTROL_V1",
        "MICROSTRUCTURE_BASELINE_V1",
        "RANDOM_SELECT_V1",
        "MARKET_CONFIRMED_V1",
        "LOSS_FILTER_V1",
        "IV_SELECT_V1",
        "FAST_SCOUT_V1",
        "SURVIVOR_PICKER_V1",
    ];
    json!({
        "items": keys.into_iter().filter_map(strategy_manifest).collect::<Vec<_>>(),
        "experiment_id": EXEC_SELECT_EXPERIMENT_ID,
        "fabricated": false
    })
}

pub async fn selection_forensics(state: &AppState) -> anyhow::Result<Value> {
    let books = [
        STRATEGY_INFORMATION_VALUE_EXPLORER_V1,
        STRATEGY_RANDOM_CONTROL_V1,
        STRATEGY_MARKET_ONLY_SHADOW,
        STRATEGY_FULL_ML_SHADOW,
        "RANDOM_SELECT_V1",
        "MARKET_CONFIRMED_V1",
        "LOSS_FILTER_V1",
        "IV_SELECT_V1",
    ];
    let mut feature_selects = String::new();
    for (i, name) in FORENSIC_FEATURES.iter().enumerate() {
        if i > 0 {
            feature_selects.push(',');
        }
        feature_selects.push_str(&format!(
            "percentile_cont(0.5) WITHIN GROUP (ORDER BY NULLIF(fs.values->>'{name}','')::double precision) FILTER (WHERE fs.values ? '{name}' AND jsonb_typeof(fs.values->'{name}')='number') AS med_{name}"
        ));
    }
    let sql = format!(
        r#"SELECT d.policy_version,
                  COUNT(*)::bigint AS n,
                  {feature_selects}
           FROM pumphound.decisions d
           JOIN pumphound.feature_snapshots fs ON fs.feature_snapshot_id=d.feature_snapshot_id
           WHERE d.decision_kind='ENTRY' AND d.action='BUY'
             AND d.policy_version = ANY($1)
           GROUP BY d.policy_version"#
    );
    let rows = sqlx::query(&sql)
        .bind(&books[..])
        .fetch_all(&state.stores.postgres)
        .await?;
    let mut items = Vec::new();
    for row in rows {
        let policy: String = row.get("policy_version");
        let n: i64 = row.get("n");
        let mut medians = serde_json::Map::new();
        for name in FORENSIC_FEATURES {
            let col = format!("med_{name}");
            let value: Option<f64> = row.try_get(col.as_str()).ok().flatten();
            medians.insert(
                (*name).into(),
                value.map(Value::from).unwrap_or(Value::Null),
            );
        }
        items.push(json!({
            "strategy_version": policy,
            "n": n,
            "entry_medians": medians,
            "unknown_means_feature_absent": true
        }));
    }
    let iv = items.iter().find(|item| {
        item.get("strategy_version").and_then(Value::as_str)
            == Some(STRATEGY_INFORMATION_VALUE_EXPLORER_V1)
    });
    let mut contrasts = Vec::new();
    if let Some(iv) = iv {
        for item in &items {
            let other = item.get("strategy_version").and_then(Value::as_str).unwrap_or("");
            if other == STRATEGY_INFORMATION_VALUE_EXPLORER_V1 {
                continue;
            }
            let mut deltas = serde_json::Map::new();
            for name in FORENSIC_FEATURES {
                let a = iv
                    .pointer(&format!("/entry_medians/{name}"))
                    .and_then(Value::as_f64);
                let b = item
                    .pointer(&format!("/entry_medians/{name}"))
                    .and_then(Value::as_f64);
                deltas.insert(
                    (*name).into(),
                    match (a, b) {
                        (Some(a), Some(b)) => Value::from(a - b),
                        _ => Value::Null,
                    },
                );
            }
            contrasts.push(json!({
                "versus": other,
                "median_delta_iv_minus_other": deltas
            }));
        }
    }
    Ok(json!({
        "question": "Why does Information Value select different tokens?",
        "as_of": pumphound_domain::now_utc(),
        "books": items,
        "contrasts": contrasts,
        "note": "Medians are entry-time feature snapshots on BUY decisions only. Null is absent, never zero. This describes selection, not edge.",
        "fabricated": false
    }))
}

pub async fn exec_select_board(state: &AppState) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT d.policy_version,
                  COUNT(*) FILTER (WHERE d.action='BUY')::bigint AS buys,
                  COUNT(*) FILTER (WHERE d.action='WATCH')::bigint AS watches,
                  COUNT(*) FILTER (WHERE d.action='AVOID')::bigint AS avoids,
                  COUNT(*) FILTER (WHERE d.action='ABSTAIN')::bigint AS abstains
           FROM pumphound.decisions d
           WHERE d.decision_kind='ENTRY'
             AND d.payload->>'experiment_id'=$1
           GROUP BY d.policy_version
           ORDER BY d.policy_version"#,
    )
    .bind(EXEC_SELECT_EXPERIMENT_ID)
    .fetch_all(&state.stores.postgres)
    .await;
    let rows = match rows {
        Ok(rows) => rows,
        Err(_) => sqlx::query(
            r#"SELECT d.policy_version,
                      COUNT(*) FILTER (WHERE d.action='BUY')::bigint AS buys,
                      COUNT(*) FILTER (WHERE d.action='WATCH')::bigint AS watches,
                      COUNT(*) FILTER (WHERE d.action='AVOID')::bigint AS avoids,
                      COUNT(*) FILTER (WHERE d.action='ABSTAIN')::bigint AS abstains
               FROM pumphound.decisions d
               WHERE d.decision_kind='ENTRY'
                 AND d.exact_inputs->>'experiment_id'=$1
               GROUP BY d.policy_version
               ORDER BY d.policy_version"#,
        )
        .bind(EXEC_SELECT_EXPERIMENT_ID)
        .fetch_all(&state.stores.postgres)
        .await?,
    };
    Ok(json!({
        "experiment_id": EXEC_SELECT_EXPERIMENT_ID,
        "simulator_version": pumphound_execution_twin::SIMULATOR_VERSION,
        "identity": "picker_not_sniper",
        "matched": ["RANDOM_SELECT_V1","MARKET_CONFIRMED_V1","LOSS_FILTER_V1","IV_SELECT_V1"],
        "timing_labelled": ["FAST_SCOUT_V1","SURVIVOR_PICKER_V1"],
        "items": rows.into_iter().map(|row| json!({
            "strategy_version": row.get::<String,_>("policy_version"),
            "buys": row.get::<i64,_>("buys"),
            "watches": row.get::<i64,_>("watches"),
            "avoids": row.get::<i64,_>("avoids"),
            "abstains": row.get::<i64,_>("abstains"),
            "manifest": strategy_manifest(&row.get::<String,_>("policy_version"))
        })).collect::<Vec<_>>(),
        "fabricated": false
    }))
}
