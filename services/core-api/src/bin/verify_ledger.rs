use anyhow::Context;
use serde_json::json;
use sqlx::Row;
use std::collections::BTreeMap;
use uuid::Uuid;

const FROZEN_LOOP_MANIFESTS: [(&str, &str); 6] = [
    (
        "CONTROL_FLOW_V1",
        "c0307ef06856641f54b174bec2f15ff0c26bf640264eaa61a49539c1a8e8f9e0",
    ),
    (
        "FULL_ML_SHADOW",
        "e41e05a74ee8fd3ccdcce5d28c69966da54fa31b486f1607798c13a1bea26bb6",
    ),
    (
        "HEURISTIC_GRAPH_AI_V1",
        "e4aa3e05848543a474f16e54b9cfdff8b5f692576d0e7b915d610c6309c6cc3c",
    ),
    (
        "HEURISTIC_V1",
        "a59e5589ff7cde607a1890663545541ba71f7b3ea966ac34af21a6383d62770e",
    ),
    (
        "MARKET_ONLY_SHADOW",
        "d6c8a28e0afcd9be61e245b5d7415887b940087cd6c3bcef1ffc5621c3a780a3",
    ),
    (
        "RESEARCH_EXPLORATION_V1",
        "3f16835e9059ecadbc2d76c0608abdb126ca27f4c3c91801b100fe59336f7609",
    ),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let portfolio_id = std::env::args()
        .nth(1)
        .map(|value| Uuid::parse_str(&value).context("portfolio id must be a UUID"))
        .transpose()?;
    let stores = pumphound_storage::Stores::connect()
        .await
        .context("connect stores")?;

    let mismatch = sqlx::query(
        r#"WITH lots AS (
          SELECT l.simulation_run_id,l.base_mint,
            SUM(l.base_atomic)-COALESCE(SUM(c.used),0) remaining,
            SUM(COALESCE(c.pnl,0)) realized
          FROM pumphound.acquisition_lots l
          JOIN pumphound.paper_fills entry_fill ON entry_fill.fill_id=l.fill_id
          JOIN pumphound.paper_orders entry_order ON entry_order.order_id=entry_fill.order_id
          JOIN pumphound.decisions entry_decision ON entry_decision.decision_id=entry_order.decision_id
          LEFT JOIN (
            SELECT lot_id,SUM(base_atomic) used,SUM(realized_pnl_atomic) pnl
            FROM pumphound.lot_consumptions GROUP BY lot_id
          ) c USING(lot_id)
          WHERE ($1::uuid IS NULL OR l.portfolio_id=$1)
            AND NOT EXISTS (
              SELECT 1 FROM pumphound.entity_integrity_quarantines iq
              WHERE iq.status='ACTIVE' AND (
                (iq.entity_type='LOT' AND iq.entity_id=l.lot_id::text)
                OR (iq.entity_type='LAUNCH' AND iq.entity_id=entry_decision.launch_id::text)
              )
            )
            AND NOT EXISTS (
              SELECT 1 FROM pumphound.paper_positions p
              WHERE p.simulation_run_id=l.simulation_run_id
                AND p.base_mint=l.base_mint
                AND EXISTS (
                  SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                  WHERE iq.status='ACTIVE' AND (
                    (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                    OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
                  )
                )
            )
          GROUP BY l.simulation_run_id,l.base_mint
        ), positions AS (
          SELECT simulation_run_id,base_mint,base_atomic,realized_pnl_quote_atomic
          FROM pumphound.paper_positions
          WHERE ($1::uuid IS NULL OR portfolio_id=$1)
            AND NOT EXISTS (
              SELECT 1 FROM pumphound.entity_integrity_quarantines iq
              WHERE iq.status='ACTIVE' AND (
                (iq.entity_type='POSITION' AND iq.entity_id=paper_positions.position_id::text)
                OR (iq.entity_type='LAUNCH' AND iq.entity_id=paper_positions.launch_id::text)
              )
            )
        )
        SELECT
          COUNT(*) FILTER(WHERE COALESCE(positions.base_atomic,0)<>COALESCE(lots.remaining,0))::bigint base_mismatch,
          COUNT(*) FILTER(WHERE COALESCE(positions.realized_pnl_quote_atomic,0)<>COALESCE(lots.realized,0))::bigint pnl_mismatch
        FROM positions FULL JOIN lots USING(simulation_run_id,base_mint)"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let base_mismatch: i64 = mismatch.get("base_mismatch");
    let pnl_mismatch: i64 = mismatch.get("pnl_mismatch");

    let fill_run_mismatch: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.paper_fills f
        JOIN pumphound.paper_orders o USING(order_id)
        WHERE f.simulation_run_id<>o.simulation_run_id
          AND ($1::uuid IS NULL OR o.portfolio_id=$1)"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let positions_without_entry: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.paper_positions p
        WHERE ($1::uuid IS NULL OR p.portfolio_id=$1)
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
            )
          )
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.acquisition_lots l
            JOIN pumphound.paper_fills f ON f.fill_id=l.fill_id
            JOIN pumphound.paper_orders o ON o.order_id=f.order_id
            JOIN pumphound.decisions d ON d.decision_id=o.decision_id
            WHERE l.simulation_run_id=p.simulation_run_id AND l.base_mint=p.base_mint
              AND d.launch_id=p.launch_id AND o.side='buy'
          )"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let closed_without_exit_trace: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.paper_positions p
        WHERE p.status='CLOSED' AND ($1::uuid IS NULL OR p.portfolio_id=$1)
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
            )
          )
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.lot_consumptions c
            JOIN pumphound.acquisition_lots l USING(lot_id)
            JOIN pumphound.paper_fills f ON f.fill_id=c.sell_fill_id
            JOIN pumphound.paper_orders o ON o.order_id=f.order_id
            JOIN pumphound.decisions d ON d.decision_id=o.decision_id
            WHERE l.simulation_run_id=p.simulation_run_id AND l.base_mint=p.base_mint
              AND d.decision_kind='EXIT' AND o.side='sell'
          )"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let model_lineage_missing: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.decisions d
        CROSS JOIN LATERAL jsonb_each_text(
          CASE WHEN jsonb_typeof(d.model_versions)='object' THEN d.model_versions ELSE '{}'::jsonb END
        ) model
        LEFT JOIN pumphound.model_predictions p
          ON p.feature_snapshot_id=d.feature_snapshot_id AND p.model_version=model.value
        WHERE p.prediction_id IS NULL
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text AND iq.status='ACTIVE'
          )
          AND ($1::uuid IS NULL OR d.simulation_run_id IN (
            SELECT simulation_run_id FROM pumphound.simulation_runs WHERE portfolio_id=$1
          ))"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let shadow_control_violations: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.decisions d
        CROSS JOIN LATERAL jsonb_each_text(
          CASE WHEN jsonb_typeof(d.model_versions)='object' THEN d.model_versions ELSE '{}'::jsonb END
        ) model
        LEFT JOIN pumphound.model_predictions p
          ON p.feature_snapshot_id=d.feature_snapshot_id AND p.model_version=model.value
        WHERE d.heuristic_version='CHAMPION_SHORT_HORIZON_V1'
          AND (p.prediction_id IS NULL OR p.payload->>'ml_status'<>'CONTROL')
          AND NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text AND iq.status='ACTIVE'
          )
          AND ($1::uuid IS NULL OR d.simulation_run_id IN (
            SELECT simulation_run_id FROM pumphound.simulation_runs WHERE portfolio_id=$1
          ))"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;

    let identity = sqlx::query(
        r#"SELECT
          (SELECT COUNT(*) FROM pumphound.paper_positions p
           JOIN pumphound.launches l USING(launch_id)
           WHERE p.base_mint<>l.mint
             AND ($1::uuid IS NULL OR p.portfolio_id=$1)
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
               )
             ))::bigint AS position_unquarantined,
          (SELECT COUNT(*) FROM pumphound.paper_orders o
           JOIN pumphound.decisions d USING(decision_id)
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           WHERE ((o.side='buy' AND o.output_mint<>l.mint)
               OR (o.side='sell' AND o.input_mint<>l.mint))
             AND ($1::uuid IS NULL OR o.portfolio_id=$1)
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
               )
             ))::bigint AS order_unquarantined,
          (SELECT COUNT(*) FROM pumphound.acquisition_lots lot
           JOIN pumphound.paper_fills f ON f.fill_id=lot.fill_id
           JOIN pumphound.paper_orders o ON o.order_id=f.order_id
           JOIN pumphound.decisions d ON d.decision_id=o.decision_id
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           WHERE lot.base_mint<>l.mint
             AND ($1::uuid IS NULL OR lot.portfolio_id=$1)
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='LOT' AND iq.entity_id=lot.lot_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
               )
             ))::bigint AS lot_unquarantined,
          (SELECT COUNT(DISTINCT l.launch_id) FROM pumphound.launches l
           JOIN pumphound.markets m USING(launch_id)
           WHERE l.protocol='pump_swap' AND l.mint=m.market
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.entity_type='LAUNCH' AND iq.entity_id=l.launch_id::text AND iq.status='ACTIVE'
             )
             AND ($1::uuid IS NULL OR EXISTS (
               SELECT 1 FROM pumphound.decisions d
               JOIN pumphound.simulation_runs r USING(simulation_run_id)
               WHERE d.launch_id=l.launch_id AND r.portfolio_id=$1
             )))::bigint AS launch_pool_alias_unquarantined,
          (SELECT COUNT(DISTINCT p.position_id)
           FROM pumphound.paper_positions p
           JOIN pumphound.acquisition_lots lot
             ON lot.simulation_run_id=p.simulation_run_id
            AND lot.base_mint=p.base_mint
           JOIN pumphound.entity_integrity_quarantines lot_iq
             ON lot_iq.entity_type='LOT'
            AND lot_iq.entity_id=lot.lot_id::text
            AND lot_iq.status='ACTIVE'
           WHERE ($1::uuid IS NULL OR p.portfolio_id=$1)
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
               )
             ))::bigint AS position_lot_contamination_unquarantined,
          (SELECT COUNT(*) FROM pumphound.entity_integrity_quarantines iq
           WHERE iq.status='ACTIVE'
             AND ($1::uuid IS NULL OR
               (iq.entity_type='POSITION' AND EXISTS (
                  SELECT 1 FROM pumphound.paper_positions p
                  WHERE p.position_id::text=iq.entity_id AND p.portfolio_id=$1
               )) OR
               (iq.entity_type='ORDER' AND EXISTS (
                  SELECT 1 FROM pumphound.paper_orders o
                  WHERE o.order_id::text=iq.entity_id AND o.portfolio_id=$1
               )) OR
               (iq.entity_type='LOT' AND EXISTS (
                  SELECT 1 FROM pumphound.acquisition_lots lot
                  WHERE lot.lot_id::text=iq.entity_id AND lot.portfolio_id=$1
               )) OR
               (iq.entity_type='LAUNCH' AND EXISTS (
                  SELECT 1 FROM pumphound.decisions d
                  JOIN pumphound.simulation_runs r USING(simulation_run_id)
                  WHERE d.launch_id::text=iq.entity_id AND r.portfolio_id=$1
               ))
             ))::bigint AS quarantined"#,
    )
    .bind(portfolio_id)
    .fetch_one(&stores.postgres)
    .await?;
    let position_identity_unquarantined: i64 = identity.get("position_unquarantined");
    let order_identity_unquarantined: i64 = identity.get("order_unquarantined");
    let lot_identity_unquarantined: i64 = identity.get("lot_unquarantined");
    let launch_pool_alias_unquarantined: i64 = identity.get("launch_pool_alias_unquarantined");
    let position_lot_contamination_unquarantined: i64 =
        identity.get("position_lot_contamination_unquarantined");
    let quarantined_identity_rows: i64 = identity.get("quarantined");

    let active = sqlx::query(
        r#"WITH active_runs AS (
          SELECT r.simulation_run_id FROM pumphound.simulation_runs r
          JOIN pumphound.portfolios pf USING(portfolio_id)
          WHERE r.state='running' AND pf.archived_at IS NULL AND r.manifest ? 'strategy'
        ), selected_positions AS (
          SELECT pos.* FROM pumphound.paper_positions pos
          JOIN active_runs r USING(simulation_run_id)
          WHERE NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='POSITION' AND iq.entity_id=pos.position_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=pos.launch_id::text)
            )
          )
        ), closed_lots AS (
          SELECT lot.lot_id FROM pumphound.acquisition_lots lot
          JOIN pumphound.paper_fills entry_fill ON entry_fill.fill_id=lot.fill_id
          JOIN pumphound.paper_orders entry_order ON entry_order.order_id=entry_fill.order_id
          JOIN pumphound.decisions entry_decision ON entry_decision.decision_id=entry_order.decision_id
          JOIN selected_positions pos
            ON pos.simulation_run_id=lot.simulation_run_id AND pos.base_mint=lot.base_mint
          JOIN pumphound.lot_consumptions c ON c.lot_id=lot.lot_id
          WHERE NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='LOT' AND iq.entity_id=lot.lot_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=entry_decision.launch_id::text)
            )
          )
          GROUP BY lot.lot_id,lot.base_atomic HAVING SUM(c.base_atomic)=lot.base_atomic
        ), orphan_closed_lots AS (
          SELECT lot.lot_id FROM pumphound.acquisition_lots lot
          JOIN active_runs r USING(simulation_run_id)
          JOIN pumphound.paper_fills entry_fill ON entry_fill.fill_id=lot.fill_id
          JOIN pumphound.paper_orders entry_order ON entry_order.order_id=entry_fill.order_id
          JOIN pumphound.decisions entry_decision ON entry_decision.decision_id=entry_order.decision_id
          JOIN pumphound.lot_consumptions c USING(lot_id)
          WHERE NOT EXISTS (
            SELECT 1 FROM pumphound.entity_integrity_quarantines iq
            WHERE iq.status='ACTIVE' AND (
              (iq.entity_type='LOT' AND iq.entity_id=lot.lot_id::text)
              OR (iq.entity_type='LAUNCH' AND iq.entity_id=entry_decision.launch_id::text)
            )
          )
          AND NOT EXISTS (
            SELECT 1 FROM selected_positions pos
            WHERE pos.simulation_run_id=lot.simulation_run_id AND pos.base_mint=lot.base_mint
          )
          GROUP BY lot.lot_id,lot.base_atomic HAVING SUM(c.base_atomic)=lot.base_atomic
        )
        SELECT COUNT(*) FILTER(WHERE pos.status IN ('OPEN','UNMARKABLE'))::bigint open_n,
          COUNT(*) FILTER(WHERE pos.status='CLOSED')::bigint closed_n,
          COALESCE(SUM(pos.realized_pnl_quote_atomic),0)::text pnl_atomic,
          COUNT(DISTINCT r.simulation_run_id)::bigint books_n,
          (SELECT COUNT(*) FROM closed_lots)::bigint closed_trades_n,
          (SELECT COUNT(*) FROM orphan_closed_lots)::bigint orphan_closed_trades_n
        FROM active_runs r LEFT JOIN selected_positions pos
          ON pos.simulation_run_id=r.simulation_run_id"#,
    )
    .fetch_one(&stores.postgres)
    .await?;

    let frozen_rows = sqlx::query(
        r#"SELECT manifest->>'strategy' strategy,manifest_sha256
        FROM pumphound.simulation_runs
        WHERE manifest->>'experiment_id'='ALPHA-LOOP-20260908'
          AND manifest->>'experiment_frozen'='true'"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let actual_frozen = frozen_rows
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("strategy"),
                row.get::<String, _>("manifest_sha256"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let expected_frozen = FROZEN_LOOP_MANIFESTS
        .into_iter()
        .map(|(strategy, hash)| (strategy.to_string(), hash.to_string()))
        .collect::<BTreeMap<_, _>>();
    let frozen_intact = actual_frozen == expected_frozen;

    let registry_path = std::env::var("PUMPHOUND_MODEL_REGISTRY_PATH")
        .unwrap_or_else(|_| "artifacts/models/registry.json".into());
    let artifact_registry: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&registry_path)
            .with_context(|| format!("read authoritative model registry {registry_path}"))?,
    )
    .with_context(|| format!("parse authoritative model registry {registry_path}"))?;
    let artifact_models = artifact_registry["models"]
        .as_object()
        .context("model registry models must be an object")?
        .iter()
        .filter_map(|(version, record)| {
            record["status"]
                .as_str()
                .map(|status| (version.clone(), status.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let artifact_champions = artifact_registry["champions"]
        .as_object()
        .context("model registry champions must be an object")?
        .iter()
        .filter_map(|(target, version)| {
            version
                .as_str()
                .map(|version| (target.clone(), version.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let artifact_shadows = artifact_registry["shadows"]
        .as_object()
        .context("model registry shadows must be an object")?
        .iter()
        .filter_map(|(target, version)| {
            version
                .as_str()
                .map(|version| (target.clone(), version.to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let registry_rows =
        sqlx::query("SELECT model_version,target_version,status FROM pumphound.model_registry")
            .fetch_all(&stores.postgres)
            .await?;
    let database_models = registry_rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("model_version"),
                row.get::<String, _>("status"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let database_champions = registry_rows
        .iter()
        .filter(|row| row.get::<String, _>("status") == "champion")
        .map(|row| {
            (
                row.get::<String, _>("target_version"),
                row.get::<String, _>("model_version"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let database_shadows = registry_rows
        .iter()
        .filter(|row| row.get::<String, _>("status") == "shadow")
        .map(|row| {
            (
                row.get::<String, _>("target_version"),
                row.get::<String, _>("model_version"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let model_status_mismatches = artifact_models
        .iter()
        .filter(|(version, status)| database_models.get(*version) != Some(*status))
        .count() as i64;
    let database_models_absent_from_artifact = database_models
        .iter()
        .filter(|(version, status)| {
            matches!(status.as_str(), "champion" | "shadow")
                && !artifact_models.contains_key(*version)
        })
        .count() as i64;
    let champion_index_mismatch = i64::from(artifact_champions != database_champions);
    let shadow_index_mismatch = i64::from(artifact_shadows != database_shadows);

    let failures = base_mismatch
        + pnl_mismatch
        + fill_run_mismatch
        + positions_without_entry
        + closed_without_exit_trace
        + model_lineage_missing
        + shadow_control_violations
        + position_identity_unquarantined
        + order_identity_unquarantined
        + lot_identity_unquarantined
        + launch_pool_alias_unquarantined
        + position_lot_contamination_unquarantined
        + model_status_mismatches
        + database_models_absent_from_artifact
        + champion_index_mismatch
        + shadow_index_mismatch
        + i64::from(!frozen_intact);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope": portfolio_id.map(|id| id.to_string()).unwrap_or_else(|| "ALL_PERSISTED_BOOKS".into()),
            "active_book_summary": {
                "books": active.get::<i64,_>("books_n"),
                "open_positions": active.get::<i64,_>("open_n"),
                "closed_positions": active.get::<i64,_>("closed_n"),
                "closed_trades": active.get::<i64,_>("closed_trades_n"),
                "closed_trade_basis": "FULLY_FIFO_CONSUMED_ENTRY_LOT",
                "closed_trades_without_verified_position": active.get::<i64,_>("orphan_closed_trades_n"),
                "realized_pnl_quote_atomic": active.get::<String,_>("pnl_atomic"),
                "quote_unit": "SOL_LAMPORTS"
            },
            "violations": {
                "position_base_vs_fifo": base_mismatch,
                "position_pnl_vs_fifo": pnl_mismatch,
                "fill_order_run_identity": fill_run_mismatch,
                "position_entry_trace": positions_without_entry,
                "closed_position_exit_trace": closed_without_exit_trace,
                "decision_model_prediction_link": model_lineage_missing,
                "shadow_model_control": shadow_control_violations,
                "position_launch_mint_identity_unquarantined": position_identity_unquarantined,
                "order_launch_mint_identity_unquarantined": order_identity_unquarantined,
                "lot_launch_mint_identity_unquarantined": lot_identity_unquarantined,
                "pumpswap_pool_alias_launch_unquarantined": launch_pool_alias_unquarantined,
                "position_lot_contamination_unquarantined": position_lot_contamination_unquarantined,
                "model_artifact_database_status_mismatch": model_status_mismatches,
                "active_database_models_absent_from_artifact_registry": database_models_absent_from_artifact,
                "model_champion_index_mismatch": champion_index_mismatch,
                "model_shadow_index_mismatch": shadow_index_mismatch,
                "frozen_alpha_loop_manifest": i64::from(!frozen_intact)
            },
            "model_registry": {
                "authoritative_path": registry_path,
                "artifact_champions": artifact_champions,
                "database_champions": database_champions,
                "artifact_shadows": artifact_shadows,
                "database_shadows": database_shadows
            },
            "integrity_quarantine": {
                "active_rows": quarantined_identity_rows,
                "source_rows_preserved": true
            },
            "frozen_alpha_loop_intact": frozen_intact,
            "result": if failures == 0 { "PASS" } else { "FAIL" }
        }))?
    );
    anyhow::ensure!(
        failures == 0,
        "{failures} Product Truth invariant violations"
    );
    Ok(())
}
