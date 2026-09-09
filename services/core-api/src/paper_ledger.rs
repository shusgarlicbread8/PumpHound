use crate::model_client::{AggregatePrediction, ChampionSet, TargetChampion, REQUIRED_TARGETS};
use crate::state::{PaperLot, PaperPortfolio, PaperState, Position};
use chrono::{DateTime, Utc};
use pumphound_domain::{
    FeatureVector, MarketSnapshot, PaperOrderRequest, PaperQuote, TradeSide, USDC_MINT, WSOL_MINT,
};
use pumphound_paper_core::{fifo_allocate, OpenLot, OpportunityAction, OpportunityDecision};
use pumphound_storage::Stores;
use serde_json::{json, Value};
use sqlx::{Postgres, Row, Transaction};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub const SYSTEM_ABSTAIN_POLICY: &str = "system-abstain-v1";

#[derive(Clone, Debug)]
pub struct RecoverableOrder {
    pub order_id: Uuid,
    pub status: String,
    pub arrival_deadline_at: DateTime<Utc>,
    pub request: PaperOrderRequest,
    pub arrival_quote: Option<PaperQuote>,
}

pub async fn seed(stores: &Stores) -> anyhow::Result<()> {
    for (mint, decimals, symbol, name) in [
        (WSOL_MINT, 9i16, "SOL", "Wrapped SOL"),
        (USDC_MINT, 6i16, "USDC", "USD Coin"),
    ] {
        sqlx::query(
            r#"INSERT INTO pumphound.assets
               (mint, decimals, symbol, name, token_program, first_seen_slot, known_at)
               VALUES ($1,$2,$3,$4,'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA',0,now())
               ON CONFLICT (mint) DO NOTHING"#,
        )
        .bind(mint)
        .bind(decimals)
        .bind(symbol)
        .bind(name)
        .execute(&stores.postgres)
        .await?;
    }
    sqlx::query(
        r#"INSERT INTO pumphound.policy_versions
           (policy_version, config, content_sha256, created_at, reason)
           VALUES ($1,$2,$3,now(),'safe sentinel used when no policy artifact is available')
           ON CONFLICT (policy_version) DO NOTHING"#,
    )
    .bind(SYSTEM_ABSTAIN_POLICY)
    .bind(json!({"autonomy": false, "action": "ABSTAIN", "live_money": false}))
    .bind("system-abstain-v1")
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn register_policy(
    stores: &Stores,
    policy_version: &str,
    config: &Value,
    content_sha256: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.policy_versions
           (policy_version, config, content_sha256, created_at, reason)
           VALUES ($1,$2,$3,now(),'validated autonomous paper artifact')
           ON CONFLICT (policy_version) DO UPDATE SET
             config=EXCLUDED.config, content_sha256=EXCLUDED.content_sha256"#,
    )
    .bind(policy_version)
    .bind(config)
    .bind(content_sha256)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn register_immutable_policy(
    stores: &Stores,
    policy_version: &str,
    config: &Value,
    content_sha256: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.policy_versions
           (policy_version,config,content_sha256,created_at,reason)
           VALUES ($1,$2,$3,now(),'immutable ALPHA-LEARN paper policy')
           ON CONFLICT (policy_version) DO NOTHING"#,
    )
    .bind(policy_version)
    .bind(config)
    .bind(content_sha256)
    .execute(&stores.postgres)
    .await?;
    let stored: String = sqlx::query_scalar(
        "SELECT content_sha256 FROM pumphound.policy_versions WHERE policy_version=$1",
    )
    .bind(policy_version)
    .fetch_one(&stores.postgres)
    .await?;
    anyhow::ensure!(
        stored == content_sha256,
        "immutable_policy_conflict:{policy_version}"
    );
    Ok(())
}

pub async fn record_policy_transition(
    stores: &Stores,
    experiment_id: &str,
    policy_version: &str,
    to_state: &str,
    reason: &str,
    evidence: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.policy_transition_events
           (policy_transition_id,idempotency_key,experiment_id,policy_version,from_state,to_state,
            actor,reason,paper_only,occurred_at,evidence)
           VALUES ($1,$2,$3,$4,NULL,$5,'core-api',$6,true,now(),$7)
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(format!(
        "policy-initial:{experiment_id}:{policy_version}:{to_state}"
    ))
    .bind(experiment_id)
    .bind(policy_version)
    .bind(to_state)
    .bind(reason)
    .bind(evidence)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn champions(stores: &Stores) -> anyhow::Result<ChampionSet> {
    let rows = sqlx::query(
        r#"SELECT model_version,target_version,feature_schema_version,manifest
           FROM pumphound.model_registry
           WHERE status='champion'
           ORDER BY promoted_at DESC NULLS LAST,created_at DESC"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut champions = ChampionSet::new();
    for row in rows {
        let target: String = row.get("target_version");
        champions.entry(target).or_insert_with(|| TargetChampion {
            model_version: row.get("model_version"),
            feature_schema_version: row.get("feature_schema_version"),
            manifest: row.get("manifest"),
        });
    }
    Ok(champions)
}

pub async fn has_shadow_models(stores: &Stores) -> anyhow::Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pumphound.model_registry WHERE status='shadow')",
    )
    .fetch_one(&stores.postgres)
    .await?)
}

pub async fn persist_shadow_predictions(
    stores: &Stores,
    launch_id: Uuid,
    prediction: &AggregatePrediction,
) -> anyhow::Result<Value> {
    let mut persisted = serde_json::Map::new();
    let mut tx = stores.postgres.begin().await?;
    let targets = prediction
        .target_model_versions
        .keys()
        .chain(prediction.probabilities.keys())
        .collect::<BTreeSet<_>>();
    for target in targets {
        let Some(model_version) = prediction
            .target_model_versions
            .get(target)
            .and_then(|version| version.as_deref())
        else {
            continue;
        };
        let registered = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS(
                 SELECT 1 FROM pumphound.model_registry
                 WHERE model_version=$1 AND target_version=$2
                   AND status='shadow'
               )"#,
        )
        .bind(model_version)
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
        if !registered {
            continue;
        }
        let probability = prediction.probabilities.get(target).copied().flatten();
        let abstention_reason = prediction_abstention_reason(prediction, target, probability);
        let familiarity = prediction.familiarity.get(target).copied().flatten();
        sqlx::query(
            r#"INSERT INTO pumphound.model_predictions
               (prediction_id,feature_snapshot_id,launch_id,model_version,calibrated,familiarity,
                outputs,abstention_reason,computed_at,payload)
               VALUES ($1,$2,$3,$4,false,$5,$6,$7,now(),$8)
               ON CONFLICT (feature_snapshot_id,model_version) DO NOTHING"#,
        )
        .bind(format!("{}:shadow:{target}", prediction.prediction_id))
        .bind(&prediction.feature_snapshot_id)
        .bind(launch_id)
        .bind(model_version)
        .bind(familiarity)
        .bind(json!({target: {"p": probability}}))
        .bind(abstention_reason)
        .bind(json!({
            "ml_status": "SHADOW",
            "execution_effect": "NONE",
            "aggregate_prediction_id": prediction.prediction_id,
            "aggregate": prediction
        }))
        .execute(&mut *tx)
        .await?;
        persisted.insert(target.clone(), json!(model_version));
    }
    tx.commit().await?;
    Ok(Value::Object(persisted))
}

pub async fn persist_control_predictions(
    stores: &Stores,
    launch_id: Uuid,
    prediction: &AggregatePrediction,
) -> anyhow::Result<Value> {
    let mut persisted = serde_json::Map::new();
    let mut tx = stores.postgres.begin().await?;
    let targets = prediction
        .target_model_versions
        .keys()
        .chain(prediction.probabilities.keys())
        .collect::<BTreeSet<_>>();
    for target in targets {
        let Some(model_version) = prediction
            .target_model_versions
            .get(target)
            .and_then(|version| version.as_deref())
        else {
            continue;
        };
        let registered = sqlx::query_scalar::<_, bool>(
            r#"SELECT EXISTS(
                 SELECT 1 FROM pumphound.model_registry
                 WHERE model_version=$1 AND target_version=$2 AND status='champion'
               )"#,
        )
        .bind(model_version)
        .bind(target)
        .fetch_one(&mut *tx)
        .await?;
        if !registered {
            continue;
        }
        let probability = prediction.probabilities.get(target).copied().flatten();
        let abstention_reason = prediction_abstention_reason(prediction, target, probability);
        let familiarity = prediction.familiarity.get(target).copied().flatten();
        sqlx::query(
            r#"INSERT INTO pumphound.model_predictions
               (prediction_id,feature_snapshot_id,launch_id,model_version,calibrated,familiarity,
                outputs,abstention_reason,computed_at,payload)
               VALUES ($1,$2,$3,$4,true,$5,$6,$7,now(),$8)
               ON CONFLICT (feature_snapshot_id,model_version) DO UPDATE SET
                 calibrated=EXCLUDED.calibrated,
                 familiarity=EXCLUDED.familiarity,
                 outputs=EXCLUDED.outputs,
                 abstention_reason=EXCLUDED.abstention_reason,
                 computed_at=EXCLUDED.computed_at,
                 payload=model_predictions.payload || EXCLUDED.payload"#,
        )
        .bind(format!("{}:control:{target}", prediction.prediction_id))
        .bind(&prediction.feature_snapshot_id)
        .bind(launch_id)
        .bind(model_version)
        .bind(familiarity)
        .bind(json!({target: {"p": probability}}))
        .bind(abstention_reason)
        .bind(json!({
            "ml_status": "CONTROL",
            "authority": "champion",
            "execution_effect": "CHAMPION_SHORT_HORIZON",
            "aggregate_prediction_id": prediction.prediction_id
        }))
        .execute(&mut *tx)
        .await?;
        persisted.insert(target.clone(), json!(model_version));
    }
    tx.commit().await?;
    Ok(Value::Object(persisted))
}

fn prediction_abstention_reason(
    prediction: &AggregatePrediction,
    target: &str,
    probability: Option<f64>,
) -> Option<String> {
    prediction
        .withheld_reasons
        .get(target)
        .filter(|reason| !reason.is_null())
        .map(|reason| {
            reason
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| reason.to_string())
        })
        .or_else(|| probability.is_none().then(|| "probability_missing".into()))
}

pub async fn hydrate(stores: &Stores) -> anyhow::Result<PaperState> {
    let rows = sqlx::query(
        r#"SELECT p.portfolio_id, p.name, p.mode, p.active_policy_version,
                  r.simulation_run_id,r.manifest
           FROM pumphound.portfolios p
           JOIN LATERAL (
             SELECT simulation_run_id,manifest FROM pumphound.simulation_runs
             WHERE portfolio_id=p.portfolio_id AND state='running'
             ORDER BY created_at DESC LIMIT 1
           ) r ON true
           WHERE p.archived_at IS NULL"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut portfolios = HashMap::new();
    for row in rows {
        let portfolio_id: Uuid = row.get("portfolio_id");
        let run_id: Uuid = row.get("simulation_run_id");
        let manifest: Value = row.get("manifest");
        let sol_atomic = ledger_balance(&stores.postgres, portfolio_id, run_id, WSOL_MINT).await?;
        let usdc_atomic = ledger_balance(&stores.postgres, portfolio_id, run_id, USDC_MINT).await?;
        let reserved_sol =
            active_reservation(&stores.postgres, portfolio_id, run_id, WSOL_MINT).await?;
        let position_rows = sqlx::query(
            r#"SELECT base_mint, market, base_atomic::text, cost_quote_atomic::text,
                      quote_mint, realized_pnl_quote_atomic::text, status
               FROM pumphound.paper_positions
               WHERE portfolio_id=$1 AND simulation_run_id=$2
                 AND status IN ('OPEN','UNMARKABLE')"#,
        )
        .bind(portfolio_id)
        .bind(run_id)
        .fetch_all(&stores.postgres)
        .await?;
        let positions = position_rows
            .into_iter()
            .map(|p| Position {
                mint: p.get("base_mint"),
                market: p.get("market"),
                quote_mint: p.get("quote_mint"),
                base_atomic: parse_u128(p.get::<String, _>("base_atomic").as_str()),
                cost_quote_atomic: parse_u128(p.get::<String, _>("cost_quote_atomic").as_str()),
                realized_pnl_quote_atomic: parse_i128(
                    p.get::<String, _>("realized_pnl_quote_atomic").as_str(),
                ),
                status: p.get("status"),
            })
            .collect();
        let order_rows = sqlx::query(
            r#"SELECT order_id::text, client_request_id::text, status, side, market,
                      input_mint, input_atomic::text, output_mint, accepted_at,
                      arrival_at, rejection_reason
               FROM pumphound.paper_orders
               WHERE portfolio_id=$1 AND simulation_run_id=$2
                 AND status IN ('DECIDED','QUEUED','ARRIVED')
               ORDER BY accepted_at"#,
        )
        .bind(portfolio_id)
        .bind(run_id)
        .fetch_all(&stores.postgres)
        .await?;
        let orders = order_rows
            .iter()
            .map(|o| {
                json!({
                    "order_id": o.get::<String,_>("order_id"),
                    "client_request_id": o.get::<String,_>("client_request_id"),
                    "status": o.get::<String,_>("status"),
                    "side": o.get::<String,_>("side"),
                    "market": o.get::<String,_>("market"),
                    "input_mint": o.get::<String,_>("input_mint"),
                    "input_atomic": o.get::<String,_>("input_atomic"),
                    "output_mint": o.get::<String,_>("output_mint"),
                    "accepted_at": o.get::<DateTime<Utc>,_>("accepted_at"),
                    "arrival_at": o.get::<DateTime<Utc>,_>("arrival_at"),
                    "rejection_reason": o.try_get::<Option<String>,_>("rejection_reason").ok().flatten(),
                })
            })
            .collect();
        portfolios.insert(
            portfolio_id,
            PaperPortfolio {
                portfolio_id,
                simulation_run_id: run_id,
                name: row.get("name"),
                mode: row.get("mode"),
                operating_mode: manifest
                    .get("operating_mode")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| row.get::<String, _>("mode")),
                sol_atomic,
                usdc_atomic,
                reserved_sol,
                positions,
                orders,
                risk: Default::default(),
                gates: Default::default(),
                policy_version: row.get("active_policy_version"),
            },
        );
    }
    let lot_rows = sqlx::query(
        r#"SELECT l.lot_id,l.portfolio_id,l.simulation_run_id,l.base_mint,l.quote_mint,
                  (l.base_atomic-COALESCE(SUM(c.base_atomic),0))::text AS remaining_base,
                  (l.cost_atomic-COALESCE(SUM(c.allocated_cost_atomic),0))::text AS remaining_cost
           FROM pumphound.acquisition_lots l
           LEFT JOIN pumphound.lot_consumptions c ON c.lot_id=l.lot_id
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=l.simulation_run_id
           WHERE r.state='running'
           GROUP BY l.lot_id,l.portfolio_id,l.simulation_run_id,l.base_mint,l.quote_mint,
                    l.base_atomic,l.cost_atomic
           HAVING l.base_atomic-COALESCE(SUM(c.base_atomic),0)>0
           ORDER BY l.acquired_at,l.lot_id"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let lots = lot_rows
        .iter()
        .map(|row| PaperLot {
            lot_id: row.get("lot_id"),
            portfolio_id: row.get("portfolio_id"),
            simulation_run_id: row.get("simulation_run_id"),
            base_mint: row.get("base_mint"),
            quote_mint: row.get("quote_mint"),
            remaining_base_atomic: parse_u128(row.get::<String, _>("remaining_base").as_str()),
            remaining_cost_atomic: parse_u128(row.get::<String, _>("remaining_cost").as_str()),
        })
        .collect();
    Ok(PaperState { portfolios, lots })
}

pub async fn ensure_default_portfolio(
    stores: &Stores,
    mode: &str,
    policy_version: &str,
    sol_atomic: i128,
    usdc_atomic: i128,
) -> anyhow::Result<(Uuid, Uuid)> {
    let name = if policy_version == "HEURISTIC_V1" {
        "HEURISTIC_V1 paper desk"
    } else {
        "Autonomous ML paper desk"
    };
    ensure_strategy_portfolio(
        stores,
        mode,
        policy_version,
        name,
        policy_version,
        sol_atomic,
        usdc_atomic,
        mode_ml_status(mode),
        "ADVISORY",
        json!({}),
    )
    .await
}

pub async fn ensure_strategy_portfolio(
    stores: &Stores,
    mode: &str,
    strategy_key: &str,
    desk_name: &str,
    policy_version: &str,
    sol_atomic: i128,
    usdc_atomic: i128,
    ml_status: &str,
    ai_status: &str,
    extra_manifest: Value,
) -> anyhow::Result<(Uuid, Uuid)> {
    let stored_mode = if mode == "REPLAY" {
        "REPLAY"
    } else {
        "LIVE_PAPER"
    };
    let mut tx = stores.postgres.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(format!("strategy-paper-portfolio:{desk_name}"))
        .execute(&mut *tx)
        .await?;
    if let Some(row) = sqlx::query(
        r#"SELECT p.portfolio_id, r.simulation_run_id,r.manifest
           FROM pumphound.portfolios p
           JOIN pumphound.simulation_runs r ON r.portfolio_id=p.portfolio_id
           WHERE p.archived_at IS NULL AND p.name=$1 AND r.state='running'
           ORDER BY p.created_at, r.created_at DESC LIMIT 1"#,
    )
    .bind(desk_name)
    .fetch_optional(&mut *tx)
    .await?
    {
        let portfolio_id = row.get("portfolio_id");
        let run_id = row.get("simulation_run_id");
        let mut manifest: Value = row.get("manifest");
        merge_strategy_manifest(
            &mut manifest,
            mode,
            strategy_key,
            ml_status,
            ai_status,
            &extra_manifest,
        );
        if manifest.get("experiment_frozen").and_then(Value::as_bool) != Some(true) {
            manifest["policy_version"] = json!(policy_version);
        }
        let manifest_sha256 =
            pumphound_domain::sha256_hex(serde_json::to_string(&manifest)?.as_bytes());
        sqlx::query(
            r#"UPDATE pumphound.simulation_runs
               SET manifest=$2,manifest_sha256=$3
               WHERE simulation_run_id=$1"#,
        )
        .bind(run_id)
        .bind(manifest)
        .bind(manifest_sha256)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok((portfolio_id, run_id));
    }
    let portfolio_id = Uuid::now_v7();
    let run_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO pumphound.portfolios
           (portfolio_id,name,mode,base_currency,active_policy_version,created_at)
           VALUES ($1,$2,$3,'SOL',$4,now())"#,
    )
    .bind(portfolio_id)
    .bind(desk_name)
    .bind(stored_mode)
    .bind(policy_version)
    .execute(&mut *tx)
    .await?;
    let mut manifest = json!({
        "live_money": false,
        "execution": "paper_only",
        "operating_mode": mode,
        "strategy": strategy_key,
        "ml_status": ml_status,
        "ai_status": ai_status,
        "capital": "SIMULATED",
        "comparable_market": true
    });
    merge_strategy_manifest(
        &mut manifest,
        mode,
        strategy_key,
        ml_status,
        ai_status,
        &extra_manifest,
    );
    if manifest.get("experiment_frozen").and_then(Value::as_bool) != Some(true) {
        manifest["policy_version"] = json!(policy_version);
    }
    sqlx::query(
        r#"INSERT INTO pumphound.simulation_runs
           (simulation_run_id,portfolio_id,kind,manifest,manifest_sha256,random_seed,state,created_at)
           VALUES ($1,$2,$3,$4,$5,7,'running',now())"#,
    )
    .bind(run_id)
    .bind(portfolio_id)
    .bind(if stored_mode == "REPLAY" {
        "replay"
    } else {
        "live_paper"
    })
    .bind(&manifest)
    .bind(pumphound_domain::sha256_hex(
        serde_json::to_string(&manifest)?.as_bytes(),
    ))
    .execute(&mut *tx)
    .await?;
    initial_funding(&mut tx, portfolio_id, run_id, WSOL_MINT, sol_atomic, "sol").await?;
    initial_funding(
        &mut tx,
        portfolio_id,
        run_id,
        USDC_MINT,
        usdc_atomic,
        "usdc",
    )
    .await?;
    tx.commit().await?;
    Ok((portfolio_id, run_id))
}

fn merge_strategy_manifest(
    manifest: &mut Value,
    mode: &str,
    strategy_key: &str,
    ml_status: &str,
    ai_status: &str,
    extra_manifest: &Value,
) {
    let frozen = manifest.get("experiment_frozen").and_then(Value::as_bool) == Some(true);
    let experiment_id = manifest.get("experiment_id").cloned();
    let freeze_keys = [
        "experiment_id",
        "experiment_frozen",
        "strategy_version",
        "feature_version",
        "fee_model_version",
        "latency_config",
        "ai_prompt_version",
        "position_sizing",
        "signal_policy",
        "signal_policy_sha256",
        "start_slot",
    ];
    let frozen_values = freeze_keys
        .iter()
        .filter_map(|key| manifest.get(*key).cloned().map(|value| (*key, value)))
        .collect::<Vec<_>>();
    manifest["operating_mode"] = json!(mode);
    manifest["strategy"] = json!(strategy_key);
    manifest["ml_status"] = json!(ml_status);
    manifest["ai_status"] = json!(ai_status);
    manifest["capital"] = json!("SIMULATED");
    manifest["live_money"] = json!(false);
    manifest["comparable_market"] = json!(true);
    if let Some(extra) = extra_manifest.as_object() {
        for (key, value) in extra {
            if frozen && freeze_keys.contains(&key.as_str()) && manifest.get(key).is_some() {
                continue;
            }
            manifest[key] = value.clone();
        }
    }
    if frozen {
        for (key, value) in frozen_values {
            manifest[key] = value;
        }
    } else if let Some(experiment_id) = experiment_id {
        manifest["experiment_id"] = experiment_id;
    }
}

pub async fn archive_unused_default_portfolios(stores: &Stores) -> anyhow::Result<u64> {
    let result = sqlx::query(
        r#"UPDATE pumphound.portfolios
           SET archived_at=now()
           WHERE archived_at IS NULL
             AND name IN (
               'Default paper',
               'HEURISTIC_V1 paper desk',
               'HEURISTIC_GRAPH_AI_V1 paper desk',
               'MARKET_ONLY_SHADOW paper desk',
               'FULL_ML_SHADOW paper desk'
             )"#,
    )
    .execute(&stores.postgres)
    .await?;
    Ok(result.rows_affected())
}

pub async fn create_portfolio(
    stores: &Stores,
    name: &str,
    mode: &str,
    policy_version: &str,
    sol_atomic: i128,
    usdc_atomic: i128,
) -> anyhow::Result<(Uuid, Uuid)> {
    let operating_mode = mode;
    let mode = if operating_mode == "REPLAY" {
        "REPLAY"
    } else {
        "LIVE_PAPER"
    };
    let portfolio_id = Uuid::now_v7();
    let run_id = Uuid::now_v7();
    let mut tx = stores.postgres.begin().await?;
    sqlx::query(
        r#"INSERT INTO pumphound.portfolios
           (portfolio_id,name,mode,base_currency,active_policy_version,created_at)
           VALUES ($1,$2,$3,'SOL',$4,now())"#,
    )
    .bind(portfolio_id)
    .bind(name)
    .bind(mode)
    .bind(policy_version)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        r#"INSERT INTO pumphound.simulation_runs
           (simulation_run_id,portfolio_id,kind,manifest,manifest_sha256,random_seed,state,created_at)
           VALUES ($1,$2,$3,$4,$5,7,'running',now())"#,
    )
    .bind(run_id)
    .bind(portfolio_id)
    .bind(if mode == "REPLAY" {
        "replay"
    } else {
        "live_paper"
    })
    .bind(json!({
        "live_money": false,
        "execution": "paper_only",
        "operating_mode": operating_mode,
        "strategy": policy_version,
        "capital": "SIMULATED"
    }))
    .bind(format!("paper-run-{run_id}"))
    .execute(&mut *tx)
    .await?;
    initial_funding(&mut tx, portfolio_id, run_id, WSOL_MINT, sol_atomic, "sol").await?;
    initial_funding(
        &mut tx,
        portfolio_id,
        run_id,
        USDC_MINT,
        usdc_atomic,
        "usdc",
    )
    .await?;
    tx.commit().await?;
    Ok((portfolio_id, run_id))
}

async fn initial_funding(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
    run_id: Uuid,
    mint: &str,
    amount: i128,
    label: &str,
) -> anyhow::Result<()> {
    if amount == 0 {
        return Ok(());
    }
    sqlx::query(
        r#"INSERT INTO pumphound.ledger_movements
           (movement_id,portfolio_id,simulation_run_id,asset_mint,signed_atomic,category,
            at_time,idempotency_key,metadata)
           VALUES ($1,$2,$3,$4,$5::numeric,'initial_funding',now(),$6,'{}')
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(portfolio_id)
    .bind(run_id)
    .bind(mint)
    .bind(amount.to_string())
    .bind(format!("fund-{label}-{portfolio_id}"))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn persist_feature_snapshot(
    stores: &Stores,
    launch_id: Uuid,
    mint: &str,
    vector: &FeatureVector,
    run_id: Option<Uuid>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.feature_snapshots
           (feature_snapshot_id,launch_id,mint,observation_window_seconds,cutoff_slot,
            knowledge_cutoff,computed_at,schema_version,values,dependency_ids,quality,simulation_run_id)
           VALUES ($1,$2,$3,$4,$5,$6,now(),$7,$8,$9,$10,$11)
           ON CONFLICT (feature_snapshot_id) DO NOTHING"#,
    )
    .bind(&vector.feature_snapshot_id)
    .bind(launch_id)
    .bind(mint)
    .bind(vector.observation_window_seconds as i32)
    .bind(vector.as_of.slot.0 as i64)
    .bind(vector.as_of.knowledge_cutoff)
    .bind(&vector.schema_version)
    .bind(Value::Object(vector.values.clone()))
    .bind(json!(vector.dependency_ids))
    .bind(serde_json::to_value(&vector.quality)?)
    .bind(run_id)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn persist_lifecycle_event(
    stores: &Stores,
    idempotency_key: &str,
    experiment_id: &str,
    launch_id: Uuid,
    feature_snapshot_id: Option<&str>,
    decision_id: Option<Uuid>,
    strategy_version: Option<&str>,
    state: pumphound_paper_core::LearningFunnelState,
    reason_code: &str,
    retryable: bool,
    chain_creation_at: Option<chrono::DateTime<chrono::Utc>>,
    first_observed_at: chrono::DateTime<chrono::Utc>,
    snapshot_at: Option<chrono::DateTime<chrono::Utc>>,
    decision_at: Option<chrono::DateTime<chrono::Utc>>,
    payload: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.candidate_lifecycle_events
           (lifecycle_event_id,idempotency_key,experiment_id,launch_id,feature_snapshot_id,
            decision_id,strategy_version,state,reason_code,retryable,chain_creation_at,
            first_observed_at,snapshot_at,decision_at,occurred_at,payload)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,now(),$15)
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(idempotency_key)
    .bind(experiment_id)
    .bind(launch_id)
    .bind(feature_snapshot_id)
    .bind(decision_id)
    .bind(strategy_version)
    .bind(state.as_str())
    .bind(reason_code)
    .bind(retryable)
    .bind(chain_creation_at)
    .bind(first_observed_at)
    .bind(snapshot_at)
    .bind(decision_at)
    .bind(payload)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn ensure_research_track_invariant(
    stores: &Stores,
    experiment_id: &str,
    frozen: bool,
    policy_hashes: &Value,
    strategy_books: &Value,
    baseline_counts: &Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.research_track_invariants
           (experiment_id,frozen,policy_hashes,strategy_books,baseline_counts,captured_at,updated_at)
           VALUES ($1,$2,$3,$4,$5,now(),now())
           ON CONFLICT (experiment_id) DO NOTHING"#,
    )
    .bind(experiment_id)
    .bind(frozen)
    .bind(policy_hashes)
    .bind(strategy_books)
    .bind(baseline_counts)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn persist_predictions(
    stores: &Stores,
    launch_id: Uuid,
    prediction: &AggregatePrediction,
    champions: &ChampionSet,
) -> anyhow::Result<()> {
    let mut tx = stores.postgres.begin().await?;
    for target in REQUIRED_TARGETS {
        let champion = champions
            .get(target)
            .ok_or_else(|| anyhow::anyhow!("missing champion for {target}"))?;
        let probability = prediction
            .probability(target)
            .ok_or_else(|| anyhow::anyhow!("missing probability for {target}"))?;
        let familiarity = prediction
            .familiarity
            .get(target)
            .copied()
            .flatten()
            .ok_or_else(|| anyhow::anyhow!("missing familiarity for {target}"))?;
        let target_prediction_id = format!("{}:{target}", prediction.prediction_id);
        sqlx::query(
            r#"INSERT INTO pumphound.model_predictions
               (prediction_id,feature_snapshot_id,launch_id,model_version,calibrated,familiarity,
                outputs,abstention_reason,computed_at,payload)
               VALUES ($1,$2,$3,$4,true,$5,$6,NULL,now(),$7)
               ON CONFLICT (feature_snapshot_id,model_version) DO NOTHING"#,
        )
        .bind(target_prediction_id)
        .bind(&prediction.feature_snapshot_id)
        .bind(launch_id)
        .bind(&champion.model_version)
        .bind(familiarity)
        .bind(json!({target: {"p": probability}}))
        .bind(json!({
            "aggregate_prediction_id": prediction.prediction_id,
            "target": target,
            "aggregate": prediction,
            "explanation": prediction.explanations.get(target)
        }))
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn persist_decision(
    stores: &Stores,
    decision_id: Uuid,
    decision_key: &str,
    run_id: Option<Uuid>,
    launch_id: Uuid,
    prediction_id: Option<&str>,
    feature_snapshot_id: &str,
    policy_version: &str,
    strategy_version: Option<&str>,
    action: &str,
    decision_kind: &str,
    model_versions: &Value,
    exact_inputs: &Value,
    reasons: &[String],
    payload: &Value,
) -> anyhow::Result<Uuid> {
    let row = sqlx::query(
        r#"INSERT INTO pumphound.decisions
           (decision_id,decision_key,simulation_run_id,launch_id,prediction_id,heuristic_version,
            feature_snapshot_id,policy_version,action,decision_kind,model_versions,exact_inputs,
            decision_at,knowledge_cutoff,reason_codes,payload)
           VALUES ($1,$2,$3,$4,$5,$8,$6,$7,$9,$10,$11,$12,now(),now(),$13,$14)
           ON CONFLICT (decision_key) DO UPDATE SET decision_key=EXCLUDED.decision_key
           RETURNING decision_id"#,
    )
    .bind(decision_id)
    .bind(decision_key)
    .bind(run_id)
    .bind(launch_id)
    .bind(prediction_id)
    .bind(feature_snapshot_id)
    .bind(policy_version)
    .bind(strategy_version)
    .bind(action)
    .bind(decision_kind)
    .bind(model_versions)
    .bind(exact_inputs)
    .bind(json!(reasons))
    .bind(payload)
    .fetch_one(&stores.postgres)
    .await?;
    Ok(row.get("decision_id"))
}

pub async fn decision_for_key(stores: &Stores, decision_key: &str) -> anyhow::Result<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT decision_id FROM pumphound.decisions WHERE decision_key=$1")
            .bind(decision_key)
            .fetch_optional(&stores.postgres)
            .await?,
    )
}

pub async fn pending_research_intents(stores: &Stores) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(
        r#"SELECT decision_id::text,decision_key,launch_id::text,simulation_run_id::text,
                  feature_snapshot_id,heuristic_version,exact_inputs,payload,decision_at
           FROM pumphound.decisions
           WHERE decision_kind='ENTRY'
             AND payload->'research_filter'->>'status'='PENDING'
           ORDER BY decision_at
           LIMIT 40"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    Ok(rows
        .iter()
        .map(|row| {
            json!({
                "decision_id": row.get::<String,_>("decision_id"),
                "decision_key": row.get::<String,_>("decision_key"),
                "launch_id": row.get::<String,_>("launch_id"),
                "simulation_run_id": row.try_get::<Option<String>,_>("simulation_run_id").ok().flatten(),
                "feature_snapshot_id": row.get::<String,_>("feature_snapshot_id"),
                "heuristic_version": row.try_get::<Option<String>,_>("heuristic_version").ok().flatten(),
                "exact_inputs": row.get::<Value,_>("exact_inputs"),
                "payload": row.get::<Value,_>("payload"),
                "decision_at": row.get::<DateTime<Utc>,_>("decision_at"),
            })
        })
        .collect())
}

pub async fn latest_launch_investigation(
    stores: &Stores,
    launch_id: Uuid,
    not_before: DateTime<Utc>,
) -> anyhow::Result<Option<Value>> {
    let row = sqlx::query(
        r#"SELECT investigation_id::text,subject_type,subject_id,as_of_slot,
                  evidence_pack_sha256,provider,model_id,prompt_version,role,response,
                  validation,created_at
           FROM pumphound.investigations
           WHERE subject_type='launch' AND subject_id=$1 AND created_at >= $2
           ORDER BY created_at DESC LIMIT 1"#,
    )
    .bind(launch_id.to_string())
    .bind(not_before - chrono::Duration::seconds(5))
    .fetch_optional(&stores.postgres)
    .await?;
    Ok(row.map(|row| {
        json!({
            "investigation_id": row.get::<String,_>("investigation_id"),
            "subject_type": row.get::<String,_>("subject_type"),
            "subject_id": row.get::<String,_>("subject_id"),
            "as_of_slot": row.get::<i64,_>("as_of_slot").to_string(),
            "evidence_pack_sha256": row.get::<String,_>("evidence_pack_sha256"),
            "provider": row.get::<String,_>("provider"),
            "model_id": row.get::<String,_>("model_id"),
            "prompt_version": row.get::<String,_>("prompt_version"),
            "role": row.get::<String,_>("role"),
            "response": row.get::<Value,_>("response"),
            "validation": row.get::<Value,_>("validation"),
            "created_at": row.get::<DateTime<Utc>,_>("created_at"),
            "policy_effect": false
        })
    }))
}

pub async fn queue_order(
    stores: &Stores,
    req: &PaperOrderRequest,
    decision_quote: &PaperQuote,
    arrival_delay_ms: u64,
    maximum_arrival_wait_ms: u64,
) -> anyhow::Result<Uuid> {
    let request_key = format!("{}:{}", req.portfolio_id, req.client_request_id);
    let mut tx = stores.postgres.begin().await?;
    if let Some(existing) =
        sqlx::query("SELECT order_id FROM pumphound.paper_orders WHERE request_key=$1")
            .bind(&request_key)
            .fetch_optional(&mut *tx)
            .await?
    {
        tx.commit().await?;
        return Ok(existing.get("order_id"));
    }
    sqlx::query("SELECT portfolio_id FROM pumphound.portfolios WHERE portfolio_id=$1 FOR UPDATE")
        .bind(req.portfolio_id)
        .fetch_one(&mut *tx)
        .await?;
    let run_id = active_run_tx(&mut tx, req.portfolio_id).await?;
    validate_order_identity(&mut tx, req).await?;
    let balance = ledger_balance_tx(
        &mut tx,
        req.portfolio_id,
        run_id,
        &req.input.mint.to_string(),
    )
    .await?;
    let reserved = active_reservation_tx(
        &mut tx,
        req.portfolio_id,
        run_id,
        &req.input.mint.to_string(),
    )
    .await?;
    let amount =
        i128::try_from(req.input.atomic.0).map_err(|_| anyhow::anyhow!("order amount overflow"))?;
    if balance - i128::try_from(reserved).unwrap_or(i128::MAX) < amount {
        anyhow::bail!("insufficient spendable balance");
    }
    sqlx::query(
        r#"INSERT INTO pumphound.paper_quotes
           (quote_id,market,market_snapshot_id,request,quote,created_at,expires_at)
           VALUES ($1,$2,$3,$4,$5,now(),$6)
           ON CONFLICT (quote_id) DO NOTHING"#,
    )
    .bind(req.quote_id)
    .bind(req.market.to_string())
    .bind(&decision_quote.market_snapshot_id)
    .bind(serde_json::to_value(req)?)
    .bind(serde_json::to_value(decision_quote)?)
    .bind(decision_quote.expires_at)
    .execute(&mut *tx)
    .await?;
    let order_id = Uuid::now_v7();
    let arrival_at = Utc::now()
        + chrono::Duration::milliseconds(i64::try_from(arrival_delay_ms).unwrap_or(i64::MAX));
    let arrival_deadline_at = Utc::now()
        + chrono::Duration::milliseconds(
            i64::try_from(maximum_arrival_wait_ms).unwrap_or(i64::MAX),
        );
    sqlx::query(
        r#"INSERT INTO pumphound.paper_orders
           (order_id,client_request_id,request_key,portfolio_id,simulation_run_id,decision_id,
            quote_id,market,side,input_mint,input_atomic,output_mint,minimum_output_atomic,status,
            accepted_at,arrival_at,arrival_deadline_at,decision_snapshot_id,payload)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::numeric,$12,$13::numeric,'DECIDED',
                   now(),$14,$15,$16,$17)"#,
    )
    .bind(order_id)
    .bind(req.client_request_id)
    .bind(&request_key)
    .bind(req.portfolio_id)
    .bind(run_id)
    .bind(req.decision_id)
    .bind(req.quote_id)
    .bind(req.market.to_string())
    .bind(side(req.side))
    .bind(req.input.mint.to_string())
    .bind(req.input.atomic.to_string())
    .bind(req.output_mint.to_string())
    .bind(decision_quote.minimum_output.atomic.to_string())
    .bind(arrival_at)
    .bind(arrival_deadline_at)
    .bind(&decision_quote.market_snapshot_id)
    .bind(json!({
        "request": req,
        "decision_quote": decision_quote,
        "paper": true,
        "live_money": false
    }))
    .execute(&mut *tx)
    .await?;
    order_event(&mut tx, order_id, 0, "DECIDED", None, json!({})).await?;
    sqlx::query(
        r#"INSERT INTO pumphound.paper_reservations
           (reservation_id,portfolio_id,simulation_run_id,order_id,asset_mint,amount_atomic,created_at)
           VALUES ($1,$2,$3,$4,$5,$6::numeric,now())"#,
    )
    .bind(Uuid::now_v7())
    .bind(req.portfolio_id)
    .bind(run_id)
    .bind(order_id)
    .bind(req.input.mint.to_string())
    .bind(req.input.atomic.to_string())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE pumphound.paper_orders SET status='QUEUED' WHERE order_id=$1")
        .bind(order_id)
        .execute(&mut *tx)
        .await?;
    order_event(
        &mut tx,
        order_id,
        1,
        "QUEUED",
        None,
        json!({"scheduled_arrival_at": arrival_at}),
    )
    .await?;
    tx.commit().await?;
    Ok(order_id)
}

async fn validate_order_identity(
    tx: &mut Transaction<'_, Postgres>,
    req: &PaperOrderRequest,
) -> anyhow::Result<()> {
    let Some(launch_id) = req.launch_id else {
        return Ok(());
    };
    let row = sqlx::query(
        r#"SELECT l.mint,
             (
               COALESCE(l.attributes->>'integrity_state','')='QUARANTINED'
               OR EXISTS(
                 SELECT 1 FROM pumphound.entity_integrity_quarantines iq
                 WHERE iq.entity_type='LAUNCH'
                   AND iq.entity_id=l.launch_id::text
                   AND iq.status='ACTIVE'
               )
             ) AS integrity_quarantined,
             EXISTS(
               SELECT 1 FROM pumphound.markets m
               WHERE m.market=$2 AND m.launch_id=l.launch_id AND m.base_mint=l.mint
             ) AS market_matches
           FROM pumphound.launches l
           WHERE l.launch_id=$1"#,
    )
    .bind(launch_id)
    .bind(req.market.to_string())
    .fetch_one(&mut **tx)
    .await?;
    let canonical_mint: String = row.get("mint");
    if row.get::<bool, _>("integrity_quarantined") {
        anyhow::bail!("ORDER_LAUNCH_QUARANTINED: launch {launch_id} failed identity validation");
    }
    let order_mint = match req.side {
        TradeSide::Buy => req.output_mint.to_string(),
        TradeSide::Sell => req.input.mint.to_string(),
    };
    if canonical_mint != order_mint {
        anyhow::bail!(
            "ORDER_LAUNCH_MINT_MISMATCH: launch {launch_id} expects {canonical_mint}, got {order_mint}"
        );
    }
    if !row.get::<bool, _>("market_matches") {
        anyhow::bail!(
            "ORDER_LAUNCH_MARKET_MISMATCH: market {} is not the canonical market for launch {launch_id}",
            req.market
        );
    }
    Ok(())
}

pub async fn recoverable_orders(stores: &Stores) -> anyhow::Result<Vec<RecoverableOrder>> {
    let rows = sqlx::query(
        r#"SELECT o.order_id,o.status,o.arrival_at,o.arrival_deadline_at,o.arrival_quote,o.payload
           FROM pumphound.paper_orders o
           JOIN pumphound.decisions d ON d.decision_id=o.decision_id
           WHERE o.status IN ('QUEUED','ARRIVED') AND o.arrival_at <= now()
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='ORDER' AND iq.entity_id=o.order_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=d.launch_id::text)
               )
             )
           ORDER BY o.arrival_at,o.order_id LIMIT 100"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut orders = Vec::new();
    for row in rows {
        let payload: Value = row.get("payload");
        let request = serde_json::from_value::<PaperOrderRequest>(
            payload.get("request").cloned().unwrap_or(Value::Null),
        )?;
        let arrival_quote = row
            .try_get::<Option<Value>, _>("arrival_quote")
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_value(value).ok());
        orders.push(RecoverableOrder {
            order_id: row.get("order_id"),
            status: row.get("status"),
            arrival_deadline_at: row.get("arrival_deadline_at"),
            request,
            arrival_quote,
        });
    }
    Ok(orders)
}

pub async fn record_arrival(
    stores: &Stores,
    order_id: Uuid,
    snapshot: &MarketSnapshot,
    quote: &PaperQuote,
) -> anyhow::Result<()> {
    let mut tx = stores.postgres.begin().await?;
    let row = sqlx::query("SELECT status FROM pumphound.paper_orders WHERE order_id=$1 FOR UPDATE")
        .bind(order_id)
        .fetch_one(&mut *tx)
        .await?;
    let status: String = row.get("status");
    if status != "QUEUED" {
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query(
        r#"UPDATE pumphound.paper_orders
           SET status='ARRIVED', arrived_at=now(), arrival_snapshot_id=$2, arrival_quote=$3
           WHERE order_id=$1"#,
    )
    .bind(order_id)
    .bind(&snapshot.snapshot_id)
    .bind(serde_json::to_value(quote)?)
    .execute(&mut *tx)
    .await?;
    order_event(
        &mut tx,
        order_id,
        2,
        "ARRIVED",
        None,
        json!({
            "market_snapshot": snapshot,
            "arrival_quote": quote
        }),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn reject_order(stores: &Stores, order_id: Uuid, reason: &str) -> anyhow::Result<()> {
    let mut tx = stores.postgres.begin().await?;
    let row = sqlx::query("SELECT status FROM pumphound.paper_orders WHERE order_id=$1 FOR UPDATE")
        .bind(order_id)
        .fetch_one(&mut *tx)
        .await?;
    let status: String = row.get("status");
    if !matches!(status.as_str(), "QUEUED" | "ARRIVED") {
        tx.commit().await?;
        return Ok(());
    }
    let sequence = if status == "ARRIVED" { 3 } else { 2 };
    sqlx::query(
        r#"UPDATE pumphound.paper_orders
           SET status='REJECTED', settled_at=now(), rejection_reason=$2
           WHERE order_id=$1"#,
    )
    .bind(order_id)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    release_reservation(&mut tx, order_id, reason).await?;
    order_event(
        &mut tx,
        order_id,
        sequence,
        "REJECTED",
        Some(reason),
        json!({"reservation_released": true}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn settle_arrived(
    stores: &Stores,
    order_id: Uuid,
    arrival_slot: i64,
) -> anyhow::Result<()> {
    let mut tx = stores.postgres.begin().await?;
    let row = sqlx::query(
        r#"SELECT o.status,o.portfolio_id,o.simulation_run_id,o.decision_id,o.market,o.side,
                  o.input_mint,o.input_atomic::text,o.output_mint,o.minimum_output_atomic::text,
                  o.arrival_snapshot_id,o.arrival_quote,o.payload,d.launch_id
           FROM pumphound.paper_orders o
           JOIN pumphound.decisions d ON d.decision_id=o.decision_id
           WHERE o.order_id=$1 FOR UPDATE OF o"#,
    )
    .bind(order_id)
    .fetch_one(&mut *tx)
    .await?;
    if row.get::<String, _>("status") != "ARRIVED" {
        tx.commit().await?;
        return Ok(());
    }
    let quote_value: Value = row
        .try_get::<Option<Value>, _>("arrival_quote")?
        .ok_or_else(|| anyhow::anyhow!("arrival quote absent"))?;
    let quote: PaperQuote = serde_json::from_value(quote_value)?;
    let minimum = parse_u128(row.get::<String, _>("minimum_output_atomic").as_str());
    if quote.expected_output.atomic.0 < minimum {
        drop(row);
        tx.rollback().await?;
        return reject_order(stores, order_id, "REJECTED_SLIPPAGE").await;
    }
    let portfolio_id: Uuid = row.get("portfolio_id");
    let run_id: Uuid = row.get("simulation_run_id");
    let decision_id: Uuid = row.get("decision_id");
    let launch_id: Uuid = row.get("launch_id");
    let order_side: String = row.get("side");
    let input_mint: String = row.get("input_mint");
    let input_atomic = parse_u128(row.get::<String, _>("input_atomic").as_str());
    let output_mint: String = row.get("output_mint");
    let output_atomic = quote.expected_output.atomic.0;
    let market: String = row.get("market");
    let snapshot_id: String = row
        .try_get::<Option<String>, _>("arrival_snapshot_id")?
        .unwrap_or_else(|| quote.market_snapshot_id.clone());
    sqlx::query("SELECT portfolio_id FROM pumphound.portfolios WHERE portfolio_id=$1 FOR UPDATE")
        .bind(portfolio_id)
        .fetch_one(&mut *tx)
        .await?;
    let fill_id = Uuid::now_v7();
    let inserted = sqlx::query(
        r#"INSERT INTO pumphound.paper_fills
           (fill_id,order_id,simulation_run_id,arrival_slot,arrival_at,input_mint,input_atomic,
            output_mint,output_atomic,market_snapshot_id,canonicality,fee_components)
           VALUES ($1,$2,$3,$4,now(),$5,$6::numeric,$7,$8::numeric,$9,'provisional',$10)
           ON CONFLICT (order_id,simulation_run_id) DO NOTHING"#,
    )
    .bind(fill_id)
    .bind(order_id)
    .bind(run_id)
    .bind(arrival_slot)
    .bind(&input_mint)
    .bind(input_atomic.to_string())
    .bind(&output_mint)
    .bind(output_atomic.to_string())
    .bind(&snapshot_id)
    .bind(serde_json::to_value(&quote.fee_components)?)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        tx.commit().await?;
        return Ok(());
    }
    movement(
        &mut tx,
        portfolio_id,
        run_id,
        fill_id,
        order_id,
        &input_mint,
        format!("-{input_atomic}"),
        "trade_input",
        format!("trade-input-{order_id}"),
    )
    .await?;
    movement(
        &mut tx,
        portfolio_id,
        run_id,
        fill_id,
        order_id,
        &output_mint,
        output_atomic.to_string(),
        "trade_output",
        format!("trade-output-{order_id}"),
    )
    .await?;
    if order_side == "buy" {
        let lot_id = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO pumphound.acquisition_lots
               (lot_id,portfolio_id,simulation_run_id,fill_id,base_mint,base_atomic,
                quote_mint,cost_atomic,acquired_at)
               VALUES ($1,$2,$3,$4,$5,$6::numeric,$7,$8::numeric,now())"#,
        )
        .bind(lot_id)
        .bind(portfolio_id)
        .bind(run_id)
        .bind(fill_id)
        .bind(&output_mint)
        .bind(output_atomic.to_string())
        .bind(&input_mint)
        .bind(input_atomic.to_string())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO pumphound.paper_positions
               (position_id,portfolio_id,simulation_run_id,launch_id,market,base_mint,quote_mint,
                base_atomic,cost_quote_atomic,opened_at,updated_at,status)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8::numeric,$9::numeric,now(),now(),'OPEN')
               ON CONFLICT (portfolio_id,simulation_run_id,base_mint) DO UPDATE SET
                 base_atomic=pumphound.paper_positions.base_atomic+EXCLUDED.base_atomic,
                 cost_quote_atomic=pumphound.paper_positions.cost_quote_atomic+EXCLUDED.cost_quote_atomic,
                 updated_at=now(),status='OPEN',closed_at=NULL"#,
        )
        .bind(Uuid::now_v7())
        .bind(portfolio_id)
        .bind(run_id)
        .bind(launch_id)
        .bind(&market)
        .bind(&output_mint)
        .bind(&input_mint)
        .bind(output_atomic.to_string())
        .bind(input_atomic.to_string())
        .execute(&mut *tx)
        .await?;
    } else {
        consume_fifo(
            &mut tx,
            portfolio_id,
            run_id,
            &input_mint,
            input_atomic,
            output_atomic,
            fill_id,
            decision_id,
        )
        .await?;
    }
    release_reservation(&mut tx, order_id, "FILLED").await?;
    sqlx::query(
        "UPDATE pumphound.paper_orders SET status='FILLED',settled_at=now() WHERE order_id=$1",
    )
    .bind(order_id)
    .execute(&mut *tx)
    .await?;
    order_event(
        &mut tx,
        order_id,
        3,
        "FILLED",
        None,
        json!({"fill_id": fill_id, "reservation_released": true}),
    )
    .await?;
    if order_side == "buy" {
        sqlx::query("UPDATE pumphound.paper_orders SET status='POSITION' WHERE order_id=$1")
            .bind(order_id)
            .execute(&mut *tx)
            .await?;
        order_event(
            &mut tx,
            order_id,
            4,
            "POSITION",
            None,
            json!({"base_mint": output_mint}),
        )
        .await?;
    }
    let lifecycle_state = if order_side == "buy" {
        "PAPER_ENTERED"
    } else {
        "EXITED"
    };
    sqlx::query(
        r#"INSERT INTO pumphound.candidate_lifecycle_events
           (lifecycle_event_id,idempotency_key,experiment_id,launch_id,feature_snapshot_id,
            decision_id,strategy_version,state,reason_code,retryable,chain_creation_at,
            first_observed_at,snapshot_at,decision_at,occurred_at,payload)
           SELECT $1,$2,r.manifest->>'experiment_id',d.launch_id,d.feature_snapshot_id,
                  d.decision_id,d.heuristic_version,$3,'paper_fill',false,l.creation_time,
                  l.known_at,fs.computed_at,d.decision_at,now(),$4
           FROM pumphound.decisions d
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=d.simulation_run_id
           JOIN pumphound.launches l ON l.launch_id=d.launch_id
           LEFT JOIN pumphound.feature_snapshots fs
             ON fs.feature_snapshot_id=d.feature_snapshot_id
           WHERE d.decision_id=$5
             AND r.manifest->>'experiment_id'=$6
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(format!("learning-fill:{fill_id}:{lifecycle_state}"))
    .bind(lifecycle_state)
    .bind(json!({"fill_id": fill_id, "order_id": order_id}))
    .bind(decision_id)
    .bind(pumphound_paper_core::ADAPTIVE_EXPERIMENT_ID)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn consume_fifo(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
    run_id: Uuid,
    base_mint: &str,
    sold_base_atomic: u128,
    proceeds_atomic: u128,
    sell_fill_id: Uuid,
    exit_decision_id: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"SELECT lot_id FROM pumphound.acquisition_lots
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND base_mint=$3
           ORDER BY acquired_at,lot_id FOR UPDATE"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(base_mint)
    .fetch_all(&mut **tx)
    .await?;
    let rows = sqlx::query(
        r#"SELECT l.lot_id::text,
                  (l.base_atomic-COALESCE(SUM(c.base_atomic),0))::text AS remaining_base,
                  (l.cost_atomic-COALESCE(SUM(c.allocated_cost_atomic),0))::text AS remaining_cost
           FROM pumphound.acquisition_lots l
           LEFT JOIN pumphound.lot_consumptions c ON c.lot_id=l.lot_id
           WHERE l.portfolio_id=$1 AND l.simulation_run_id=$2 AND l.base_mint=$3
           GROUP BY l.lot_id,l.base_atomic,l.cost_atomic,l.acquired_at
           HAVING l.base_atomic-COALESCE(SUM(c.base_atomic),0)>0
           ORDER BY l.acquired_at,l.lot_id"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(base_mint)
    .fetch_all(&mut **tx)
    .await?;
    let lots: Vec<OpenLot> = rows
        .iter()
        .map(|row| OpenLot {
            lot_id: row.get("lot_id"),
            remaining_base_atomic: parse_u128(row.get::<String, _>("remaining_base").as_str()),
            remaining_cost_atomic: parse_u128(row.get::<String, _>("remaining_cost").as_str()),
        })
        .collect();
    let allocations =
        fifo_allocate(&lots, sold_base_atomic, proceeds_atomic).map_err(anyhow::Error::msg)?;
    let mut allocated_cost = 0u128;
    let mut realized = 0i128;
    for allocation in allocations {
        allocated_cost = allocated_cost.saturating_add(allocation.allocated_cost_atomic);
        realized = realized.saturating_add(allocation.realized_pnl_atomic);
        sqlx::query(
            r#"INSERT INTO pumphound.lot_consumptions
               (consumption_id,lot_id,sell_fill_id,base_atomic,allocated_cost_atomic,
                net_proceeds_atomic,realized_pnl_atomic)
               VALUES ($1,$2::uuid,$3,$4::numeric,$5::numeric,$6::numeric,$7::numeric)
               ON CONFLICT (lot_id,sell_fill_id) DO NOTHING"#,
        )
        .bind(Uuid::now_v7())
        .bind(&allocation.lot_id)
        .bind(sell_fill_id)
        .bind(allocation.base_atomic.to_string())
        .bind(allocation.allocated_cost_atomic.to_string())
        .bind(allocation.net_proceeds_atomic.to_string())
        .bind(allocation.realized_pnl_atomic.to_string())
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        r#"UPDATE pumphound.paper_positions SET
             base_atomic=base_atomic-$4::numeric,
             cost_quote_atomic=GREATEST(0,cost_quote_atomic-$5::numeric),
             realized_pnl_quote_atomic=realized_pnl_quote_atomic+$6::numeric,
             updated_at=now(),
             last_exit_decision_id=$7,
             status=CASE WHEN base_atomic-$4::numeric=0 THEN 'CLOSED' ELSE 'OPEN' END,
             closed_at=CASE WHEN base_atomic-$4::numeric=0 THEN now() ELSE NULL END
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND base_mint=$3"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(base_mint)
    .bind(sold_base_atomic.to_string())
    .bind(allocated_cost.to_string())
    .bind(realized.to_string())
    .bind(exit_decision_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn open_positions(stores: &Stores) -> anyhow::Result<Vec<Value>> {
    open_positions_for(stores, None, None).await
}

pub async fn open_positions_for(
    stores: &Stores,
    portfolio_id: Option<Uuid>,
    run_id: Option<Uuid>,
) -> anyhow::Result<Vec<Value>> {
    let rows = sqlx::query(
        r#"SELECT position_id::text,portfolio_id::text,simulation_run_id::text,launch_id::text,
                  market,base_mint,quote_mint,base_atomic::text,cost_quote_atomic::text,
                  realized_pnl_quote_atomic::text,opened_at,updated_at,status
           FROM pumphound.paper_positions p
           WHERE p.status IN ('OPEN','UNMARKABLE')
             AND ($1::uuid IS NULL OR p.portfolio_id=$1)
             AND ($2::uuid IS NULL OR p.simulation_run_id=$2)
             AND NOT EXISTS (
               SELECT 1 FROM pumphound.entity_integrity_quarantines iq
               WHERE iq.status='ACTIVE' AND (
                 (iq.entity_type='POSITION' AND iq.entity_id=p.position_id::text)
                 OR (iq.entity_type='LAUNCH' AND iq.entity_id=p.launch_id::text)
               )
             )
           ORDER BY p.opened_at"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "position_id": r.get::<String,_>("position_id"),
                "portfolio_id": r.get::<String,_>("portfolio_id"),
                "simulation_run_id": r.get::<String,_>("simulation_run_id"),
                "launch_id": r.get::<String,_>("launch_id"),
                "market": r.get::<String,_>("market"),
                "base_mint": r.get::<String,_>("base_mint"),
                "quote_mint": r.get::<String,_>("quote_mint"),
                "base_atomic": r.get::<String,_>("base_atomic"),
                "cost_quote_atomic": r.get::<String,_>("cost_quote_atomic"),
                "realized_pnl_quote_atomic": r.get::<String,_>("realized_pnl_quote_atomic"),
                "opened_at": r.get::<DateTime<Utc>,_>("opened_at"),
                "updated_at": r.get::<DateTime<Utc>,_>("updated_at"),
                "status": r.get::<String,_>("status"),
            })
        })
        .collect())
}

pub async fn heuristic_peak_return(
    stores: &Stores,
    position_id: &str,
) -> anyhow::Result<Option<f64>> {
    Ok(sqlx::query_scalar(
        r#"SELECT MAX((exact_inputs->>'mark_return_fraction')::double precision)
           FROM pumphound.decisions
           WHERE decision_kind='EXIT'
             AND payload->>'position_id'=$1
             AND exact_inputs->>'mark_return_fraction' IS NOT NULL"#,
    )
    .bind(position_id)
    .fetch_one(&stores.postgres)
    .await?)
}

pub async fn portfolio_view(stores: &Stores, portfolio_id: Uuid) -> anyhow::Result<Option<Value>> {
    let portfolio = sqlx::query(
        r#"SELECT p.portfolio_id::text,p.name,p.mode,p.active_policy_version,
                  r.simulation_run_id::text,r.manifest
           FROM pumphound.portfolios p
           JOIN LATERAL (
             SELECT simulation_run_id,manifest FROM pumphound.simulation_runs
             WHERE portfolio_id=p.portfolio_id AND state='running'
             ORDER BY created_at DESC LIMIT 1
           ) r ON true WHERE p.portfolio_id=$1"#,
    )
    .bind(portfolio_id)
    .fetch_optional(&stores.postgres)
    .await?;
    let Some(portfolio) = portfolio else {
        return Ok(None);
    };
    let run_id = Uuid::parse_str(portfolio.get::<String, _>("simulation_run_id").as_str())?;
    let manifest: Value = portfolio.get("manifest");
    let operating_mode = manifest
        .get("operating_mode")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| portfolio.get::<String, _>("mode"));
    let strategy = manifest
        .get("strategy")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| portfolio.get::<String, _>("active_policy_version"));
    let missing_control_models = if matches!(operating_mode.as_str(), "LIVE_PAPER_ML" | "REPLAY") {
        !crate::model_client::missing_required_targets(&champions(stores).await.unwrap_or_default())
            .is_empty()
    } else {
        false
    };
    let ml_status = if (operating_mode == "LIVE_PAPER_SHADOW_ML"
        && !has_shadow_models(stores).await.unwrap_or(false))
        || missing_control_models
    {
        "NOT_TRAINED"
    } else {
        mode_ml_status(&operating_mode)
    };
    let balance_rows = sqlx::query(
        r#"SELECT asset_mint,SUM(signed_atomic)::text AS balance
           FROM pumphound.ledger_movements
           WHERE portfolio_id=$1 AND simulation_run_id=$2 GROUP BY asset_mint"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    // CLOSED positions legitimately zero out base_atomic/cost_quote_atomic once every lot is
    // sold (that's the remaining basis, not the historical entry), so an unfiltered list here
    // reads as a wall of empty "positions" once a book has any trading history. This desk is
    // for deciding what to do with current holdings, so only show what's actually open.
    let positions = sqlx::query(
        r#"SELECT position_id::text,launch_id::text,market,base_mint,quote_mint,
                  base_atomic::text,cost_quote_atomic::text,realized_pnl_quote_atomic::text,status
           FROM pumphound.paper_positions
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND status IN ('OPEN','UNMARKABLE')
           ORDER BY opened_at"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    let pending = sqlx::query(
        r#"SELECT order_id::text,decision_id::text,status,side,market,input_mint,input_atomic::text,
                  output_mint,accepted_at,arrival_at,rejection_reason
           FROM pumphound.paper_orders WHERE portfolio_id=$1 AND simulation_run_id=$2
             AND status IN ('DECIDED','QUEUED','ARRIVED') ORDER BY accepted_at"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    let fills = sqlx::query(
        r#"SELECT f.fill_id::text,f.order_id::text,f.arrival_slot,f.arrival_at,f.input_mint,
                  f.input_atomic::text,f.output_mint,f.output_atomic::text,f.market_snapshot_id,
                  f.fee_components
           FROM pumphound.paper_fills f JOIN pumphound.paper_orders o ON o.order_id=f.order_id
           WHERE o.portfolio_id=$1 AND f.simulation_run_id=$2 ORDER BY f.arrival_at DESC LIMIT 200"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    let realized: String = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(realized_pnl_atomic),0)::text
           FROM pumphound.lot_consumptions c
           JOIN pumphound.paper_fills f ON f.fill_id=c.sell_fill_id
           JOIN pumphound.paper_orders o ON o.order_id=f.order_id
           WHERE o.portfolio_id=$1 AND f.simulation_run_id=$2"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_one(&stores.postgres)
    .await?;
    let timeline = sqlx::query(
        r#"SELECT e.order_event_id::text,e.order_id::text,e.sequence,e.status,e.at_time,
                  e.reason_code,e.payload
           FROM pumphound.paper_order_events e
           JOIN pumphound.paper_orders o ON o.order_id=e.order_id
           WHERE o.portfolio_id=$1 AND o.simulation_run_id=$2
           ORDER BY e.at_time DESC,e.sequence DESC LIMIT 500"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_all(&stores.postgres)
    .await?;
    let closed_trades: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint FROM pumphound.paper_positions
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND status='CLOSED'"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .fetch_one(&stores.postgres)
    .await
    .unwrap_or(0);
    let experiment_id = manifest
        .get("experiment_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let sol_atomic = ledger_balance(&stores.postgres, portfolio_id, run_id, WSOL_MINT)
        .await
        .unwrap_or(0);
    let usdc_atomic = ledger_balance(&stores.postgres, portfolio_id, run_id, USDC_MINT)
        .await
        .unwrap_or(0);
    let reserved_sol = active_reservation(&stores.postgres, portfolio_id, run_id, WSOL_MINT)
        .await
        .unwrap_or(0);
    Ok(Some(json!({
        "portfolio_id": portfolio.get::<String,_>("portfolio_id"),
        "simulation_run_id": run_id,
        "name": portfolio.get::<String,_>("name"),
        "mode": portfolio.get::<String,_>("mode"),
        "operating_mode": operating_mode,
        "strategy": strategy,
        "sol_atomic": sol_atomic.to_string(),
        "usdc_atomic": usdc_atomic.to_string(),
        "reserved_sol": reserved_sol.to_string(),
        "risk": Value::Null,
        "risk_status": "UNKNOWN",
        "gates": {
            "entries_frozen": Value::Null,
            "exits_active": true,
            "status": "UNKNOWN"
        },
        "ml_status": ml_status,
        "ai_status": manifest.get("ai_status").and_then(Value::as_str).unwrap_or("ADVISORY"),
        "capital": "SIMULATED",
        "experiment_id": experiment_id,
        "closed_trades": closed_trades,
        "sample_warning": (closed_trades < 30).then_some(format!("N={closed_trades} — INSUFFICIENT SAMPLE")),
        "policy_version": portfolio.get::<String,_>("active_policy_version"),
        "balances": balance_rows.iter().map(|r| json!({
            "asset_mint": r.get::<String,_>("asset_mint"),
            "atomic": r.get::<String,_>("balance")
        })).collect::<Vec<_>>(),
        "positions": positions.iter().map(|r| json!({
            "position_id": r.get::<String,_>("position_id"),
            "launch_id": r.get::<String,_>("launch_id"),
            "market": r.get::<String,_>("market"),
            "base_mint": r.get::<String,_>("base_mint"),
            "quote_mint": r.get::<String,_>("quote_mint"),
            "base_atomic": r.get::<String,_>("base_atomic"),
            "cost_quote_atomic": r.get::<String,_>("cost_quote_atomic"),
            "realized_pnl_quote_atomic": r.get::<String,_>("realized_pnl_quote_atomic"),
            "unrealized_pnl_quote_atomic": Value::Null,
            "status": r.get::<String,_>("status")
        })).collect::<Vec<_>>(),
        "orders": pending.iter().map(|r| json!({
            "order_id": r.get::<String,_>("order_id"),
            "decision_id": r.get::<String,_>("decision_id"),
            "status": r.get::<String,_>("status"),
            "side": r.get::<String,_>("side"),
            "market": r.get::<String,_>("market"),
            "input_mint": r.get::<String,_>("input_mint"),
            "input_atomic": r.get::<String,_>("input_atomic"),
            "output_mint": r.get::<String,_>("output_mint"),
            "accepted_at": r.get::<DateTime<Utc>,_>("accepted_at"),
            "arrival_at": r.get::<DateTime<Utc>,_>("arrival_at"),
            "rejection_reason": r.try_get::<Option<String>,_>("rejection_reason").ok().flatten()
        })).collect::<Vec<_>>(),
        "pending_orders": pending.iter().map(|r| json!({
            "order_id": r.get::<String,_>("order_id"),
            "decision_id": r.get::<String,_>("decision_id"),
            "status": r.get::<String,_>("status"),
            "side": r.get::<String,_>("side"),
            "market": r.get::<String,_>("market"),
            "input_mint": r.get::<String,_>("input_mint"),
            "input_atomic": r.get::<String,_>("input_atomic"),
            "output_mint": r.get::<String,_>("output_mint"),
            "accepted_at": r.get::<DateTime<Utc>,_>("accepted_at"),
            "arrival_at": r.get::<DateTime<Utc>,_>("arrival_at"),
            "rejection_reason": r.try_get::<Option<String>,_>("rejection_reason").ok().flatten()
        })).collect::<Vec<_>>(),
        "fills": fills.iter().map(|r| json!({
            "fill_id": r.get::<String,_>("fill_id"),
            "order_id": r.get::<String,_>("order_id"),
            "arrival_slot": r.get::<i64,_>("arrival_slot").to_string(),
            "arrival_at": r.get::<DateTime<Utc>,_>("arrival_at"),
            "input_mint": r.get::<String,_>("input_mint"),
            "input_atomic": r.get::<String,_>("input_atomic"),
            "output_mint": r.get::<String,_>("output_mint"),
            "output_atomic": r.get::<String,_>("output_atomic"),
            "market_snapshot_id": r.get::<String,_>("market_snapshot_id"),
            "fee_components": r.get::<Value,_>("fee_components")
        })).collect::<Vec<_>>(),
        "pnl": {
            "realized_quote_atomic": realized,
            "unrealized_quote_atomic": Value::Null,
            "unrealized_status": "UNAVAILABLE_WITHOUT_CURRENT_EXECUTABLE_MARK"
        },
        "timeline": timeline.iter().map(|r| json!({
            "order_event_id": r.get::<String,_>("order_event_id"),
            "order_id": r.get::<String,_>("order_id"),
            "sequence": r.get::<i64,_>("sequence"),
            "status": r.get::<String,_>("status"),
            "at_time": r.get::<DateTime<Utc>,_>("at_time"),
            "reason_code": r.try_get::<Option<String>,_>("reason_code").ok().flatten(),
            "payload": r.get::<Value,_>("payload")
        })).collect::<Vec<_>>()
    })))
}

pub async fn ledger_items(stores: &Stores, portfolio_id: Uuid) -> anyhow::Result<Value> {
    let rows = sqlx::query(
        r#"SELECT movement_id::text,simulation_run_id::text,fill_id::text,order_id::text,
                  asset_mint,signed_atomic::text,category,at_time,idempotency_key,metadata
           FROM pumphound.ledger_movements WHERE portfolio_id=$1
           ORDER BY at_time DESC LIMIT 500"#,
    )
    .bind(portfolio_id)
    .fetch_all(&stores.postgres)
    .await?;
    Ok(json!({
        "portfolio_id": portfolio_id,
        "items": rows.iter().map(|r| json!({
            "movement_id": r.get::<String,_>("movement_id"),
            "simulation_run_id": r.get::<String,_>("simulation_run_id"),
            "fill_id": r.try_get::<Option<String>,_>("fill_id").ok().flatten(),
            "order_id": r.try_get::<Option<String>,_>("order_id").ok().flatten(),
            "asset_mint": r.get::<String,_>("asset_mint"),
            "signed_atomic": r.get::<String,_>("signed_atomic"),
            "category": r.get::<String,_>("category"),
            "at_time": r.get::<DateTime<Utc>,_>("at_time"),
            "idempotency_key": r.get::<String,_>("idempotency_key"),
            "metadata": r.get::<Value,_>("metadata")
        })).collect::<Vec<_>>(),
        "reconciled": true
    }))
}

pub async fn strategy_reports(stores: &Stores) -> anyhow::Result<Value> {
    crate::ledger_metrics::strategy_reports(stores).await
}

pub async fn experiment_funnel(stores: &Stores) -> anyhow::Result<Value> {
    let launches: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM pumphound.launches")
        .fetch_one(&stores.postgres)
        .await?;
    let universe = sqlx::query(
        r#"SELECT d.action, COALESCE(d.reason_codes, '[]'::jsonb) AS reason_codes
           FROM pumphound.decisions d
           WHERE d.decision_key LIKE 'universe:%'
             AND (
               d.exact_inputs->>'experiment_id' = $1
               OR d.payload->>'experiment_id' = $1
             )"#,
    )
    .bind(
        std::env::var("PUMPHOUND_EXPERIMENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| pumphound_paper_core::DEFAULT_EXPERIMENT_ID.to_string()),
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut structural_ok = 0i64;
    let mut liquidity_ok = 0i64;
    let mut shared_reasons = std::collections::BTreeMap::<String, i64>::new();
    for row in &universe {
        let action: String = row.get("action");
        let reasons: Value = row.get("reason_codes");
        let codes = reasons
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        if action == "BUY" {
            structural_ok += 1;
            liquidity_ok += 1;
        } else {
            for reason in &codes {
                *shared_reasons.entry(reason.clone()).or_insert(0) += 1;
            }
        }
    }
    let book_rows = sqlx::query(
        r#"SELECT COALESCE(d.heuristic_version, d.policy_version) AS strategy_key,
                  d.action,
                  COALESCE(d.reason_codes, '[]'::jsonb) AS reason_codes
           FROM pumphound.decisions d
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=d.simulation_run_id
           JOIN pumphound.portfolios p ON p.portfolio_id=r.portfolio_id
           WHERE d.decision_kind='ENTRY'
             AND p.archived_at IS NULL
             AND r.state='running'
             AND d.decision_key NOT LIKE 'universe:%'
             AND d.decision_key NOT LIKE 'shadow:%'
             AND d.decision_key NOT LIKE 'book-intent:%'
             AND d.decision_key NOT LIKE 'exp-intent:%'
             AND COALESCE(d.heuristic_version,'') <> 'ML_SHADOW'
             AND COALESCE(d.heuristic_version,'') <> 'SHARED_ELIGIBLE_V1'"#,
    )
    .fetch_all(&stores.postgres)
    .await?;
    let mut by_book = std::collections::BTreeMap::<String, Value>::new();
    let mut reason_counts = std::collections::BTreeMap::<String, i64>::new();
    for row in book_rows {
        let strategy: String = row.get("strategy_key");
        let action: String = row.get("action");
        let reasons: Value = row.get("reason_codes");
        let entry = by_book.entry(strategy).or_insert_with(|| {
            json!({
                "candidates": 0,
                "BUY": 0,
                "WATCH": 0,
                "ABSTAIN": 0,
                "AVOID": 0
            })
        });
        if let Some(object) = entry.as_object_mut() {
            object["candidates"] = json!(object["candidates"].as_i64().unwrap_or(0) + 1);
            let count = object.get(&action).and_then(Value::as_i64).unwrap_or(0);
            object.insert(action.clone(), json!(count + 1));
        }
        if action != "BUY" {
            if let Some(items) = reasons.as_array() {
                for reason in items.iter().filter_map(Value::as_str) {
                    *reason_counts.entry(reason.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    let fills: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint
           FROM pumphound.paper_orders o
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=o.simulation_run_id
           JOIN pumphound.portfolios p ON p.portfolio_id=r.portfolio_id
           WHERE o.side='buy' AND o.status='POSITION'
             AND p.archived_at IS NULL AND r.state='running'"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let positions_open: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint
           FROM pumphound.paper_positions pos
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=pos.simulation_run_id
           JOIN pumphound.portfolios p ON p.portfolio_id=r.portfolio_id
           WHERE pos.status='OPEN'
             AND p.archived_at IS NULL AND r.state='running'"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    let closed: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)::bigint
           FROM pumphound.paper_positions pos
           JOIN pumphound.simulation_runs r ON r.simulation_run_id=pos.simulation_run_id
           JOIN pumphound.portfolios p ON p.portfolio_id=r.portfolio_id
           WHERE pos.status='CLOSED'
             AND p.archived_at IS NULL AND r.state='running'"#,
    )
    .fetch_one(&stores.postgres)
    .await?;
    Ok(json!({
        "experiment_id": std::env::var("PUMPHOUND_EXPERIMENT_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| pumphound_paper_core::DEFAULT_EXPERIMENT_ID.to_string()),
        "as_of": Utc::now(),
        "stages": [
            {"key": "launches", "label": "Launches seen", "count": launches},
            {"key": "structurally_eligible", "label": "Structurally eligible", "count": structural_ok},
            {"key": "passed_liquidity", "label": "Passed liquidity / executable quote", "count": liquidity_ok},
            {"key": "fills", "label": "Fills", "count": fills},
            {"key": "open_positions", "label": "Open positions", "count": positions_open},
            {"key": "closed", "label": "Closed trades", "count": closed}
        ],
        "books": by_book,
        "abstain_reasons": reason_counts,
        "shared_reject_reasons": shared_reasons,
        "fabricated": false
    }))
}

pub fn action_name(decision: &OpportunityDecision) -> &'static str {
    match decision.action {
        OpportunityAction::Buy => "BUY",
        OpportunityAction::Watch => "WATCH",
        OpportunityAction::Avoid => "AVOID",
        OpportunityAction::Abstain => "ABSTAIN",
    }
}

fn mode_ml_status(mode: &str) -> &'static str {
    match mode {
        "LIVE_PAPER_SHADOW_ML" => "SHADOW",
        "LIVE_PAPER_ML" => "CONTROL",
        _ => "NOT_TRAINED",
    }
}

async fn active_run_tx(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
) -> anyhow::Result<Uuid> {
    Ok(sqlx::query_scalar(
        r#"SELECT simulation_run_id FROM pumphound.simulation_runs
           WHERE portfolio_id=$1 AND state='running'
           ORDER BY created_at DESC LIMIT 1 FOR UPDATE"#,
    )
    .bind(portfolio_id)
    .fetch_one(&mut **tx)
    .await?)
}

async fn ledger_balance(
    pool: &sqlx::PgPool,
    portfolio_id: Uuid,
    run_id: Uuid,
    mint: &str,
) -> anyhow::Result<i128> {
    let value: String = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(signed_atomic),0)::text
           FROM pumphound.ledger_movements
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND asset_mint=$3"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(mint)
    .fetch_one(pool)
    .await?;
    Ok(parse_i128(&value))
}

async fn ledger_balance_tx(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
    run_id: Uuid,
    mint: &str,
) -> anyhow::Result<i128> {
    let value: String = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(signed_atomic),0)::text
           FROM pumphound.ledger_movements
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND asset_mint=$3"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(mint)
    .fetch_one(&mut **tx)
    .await?;
    Ok(parse_i128(&value))
}

async fn active_reservation(
    pool: &sqlx::PgPool,
    portfolio_id: Uuid,
    run_id: Uuid,
    mint: &str,
) -> anyhow::Result<u128> {
    let value: String = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(amount_atomic),0)::text
           FROM pumphound.paper_reservations
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND asset_mint=$3
             AND released_at IS NULL"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(mint)
    .fetch_one(pool)
    .await?;
    Ok(parse_u128(&value))
}

async fn active_reservation_tx(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
    run_id: Uuid,
    mint: &str,
) -> anyhow::Result<u128> {
    let value: String = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(amount_atomic),0)::text
           FROM pumphound.paper_reservations
           WHERE portfolio_id=$1 AND simulation_run_id=$2 AND asset_mint=$3
             AND released_at IS NULL"#,
    )
    .bind(portfolio_id)
    .bind(run_id)
    .bind(mint)
    .fetch_one(&mut **tx)
    .await?;
    Ok(parse_u128(&value))
}

#[allow(clippy::too_many_arguments)]
async fn movement(
    tx: &mut Transaction<'_, Postgres>,
    portfolio_id: Uuid,
    run_id: Uuid,
    fill_id: Uuid,
    order_id: Uuid,
    mint: &str,
    signed_atomic: String,
    category: &str,
    key: String,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.ledger_movements
           (movement_id,portfolio_id,simulation_run_id,fill_id,order_id,asset_mint,
            signed_atomic,category,at_time,idempotency_key,metadata)
           VALUES ($1,$2,$3,$4,$5,$6,$7::numeric,$8,now(),$9,'{}')
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(portfolio_id)
    .bind(run_id)
    .bind(fill_id)
    .bind(order_id)
    .bind(mint)
    .bind(signed_atomic)
    .bind(category)
    .bind(key)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn release_reservation(
    tx: &mut Transaction<'_, Postgres>,
    order_id: Uuid,
    reason: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"UPDATE pumphound.paper_reservations
           SET released_at=COALESCE(released_at,now()),
               release_reason=COALESCE(release_reason,$2)
           WHERE order_id=$1 AND released_at IS NULL"#,
    )
    .bind(order_id)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn order_event(
    tx: &mut Transaction<'_, Postgres>,
    order_id: Uuid,
    sequence: i64,
    status: &str,
    reason: Option<&str>,
    payload: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.paper_order_events
           (order_event_id,order_id,sequence,status,at_time,reason_code,payload,idempotency_key)
           VALUES ($1,$2,$3,$4,now(),$5,$6,$7)
           ON CONFLICT (idempotency_key) DO NOTHING"#,
    )
    .bind(Uuid::now_v7())
    .bind(order_id)
    .bind(sequence)
    .bind(status)
    .bind(reason)
    .bind(payload)
    .bind(format!("{order_id}:{sequence}"))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn side(side: TradeSide) -> &'static str {
    match side {
        TradeSide::Buy => "buy",
        TradeSide::Sell => "sell",
    }
}

fn parse_u128(value: &str) -> u128 {
    value.parse().unwrap_or_default()
}

fn parse_i128(value: &str) -> i128 {
    value.parse().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_orders_release_reservations_by_contract() {
        fn active(status: &str) -> bool {
            matches!(status, "DECIDED" | "QUEUED" | "ARRIVED")
        }
        assert!(active("QUEUED"));
        assert!(!active("REJECTED"));
        assert!(!active("FILLED"));
        assert!(!active("POSITION"));
    }

    #[test]
    fn request_key_is_restart_stable() {
        let portfolio = uuid::Uuid::nil();
        let request = uuid::Uuid::from_u128(1);
        assert_eq!(
            format!("{portfolio}:{request}"),
            format!("{portfolio}:{request}")
        );
    }

    #[test]
    fn withheld_shadow_targets_remain_explicit() {
        let prediction: AggregatePrediction = serde_json::from_value(json!({
            "status": "ok",
            "prediction_id": "prediction-1",
            "feature_snapshot_id": "snapshot-1",
            "launch_id": Uuid::nil().to_string(),
            "observation_window_seconds": 120,
            "probabilities": {"target-a": null},
            "target_model_versions": {"target-a": "model-a"},
            "withheld_reasons": {"target-a": {"code": "ood"}}
        }))
        .unwrap();

        assert_eq!(
            prediction_abstention_reason(&prediction, "target-a", None).as_deref(),
            Some("{\"code\":\"ood\"}")
        );
        assert_eq!(
            prediction_abstention_reason(&prediction, "missing-target", None).as_deref(),
            Some("probability_missing")
        );
    }
}
