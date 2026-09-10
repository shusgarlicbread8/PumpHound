use anyhow::Context;
use chrono::{DateTime, Utc};
use pumphound_storage::Stores;
use pumphound_venue_core::{HealthState, OptionalNumber, Venue};
use serde_json::Value;
use sqlx::Row;

pub fn opt(n: OptionalNumber) -> Option<f64> {
    n.as_f64()
}

pub async fn seed_static_health(stores: &Stores) -> anyhow::Result<()> {
    let exa = if std::env::var("EXA_API_KEY").ok().filter(|s| !s.is_empty()).is_some() {
        HealthState::Unknown
    } else {
        HealthState::DisabledMissingKey
    };
    upsert_health(
        stores,
        Venue::Hyperliquid,
        "exa_research",
        exa,
        None,
        0,
        0,
        serde_json::json!({"unlocks":"HIP-3/pre-IPO structured evidence"}),
    )
    .await?;
    upsert_health(
        stores,
        Venue::Hyperliquid,
        "liquidations",
        HealthState::Unsupported,
        Some("public liquidation tape requires a user address; not collected".into()),
        0,
        0,
        serde_json::json!({}),
    )
    .await?;
    upsert_health(
        stores,
        Venue::Hyperliquid,
        "external_spot_reference",
        HealthState::Unsupported,
        Some("no defensible public spot feed wired; basis vs Binance is UNKNOWN".into()),
        0,
        0,
        serde_json::json!({}),
    )
    .await?;
    upsert_health(
        stores,
        Venue::Ostium,
        "getSimSlippage",
        HealthState::Unsupported,
        Some("official sim endpoints not exposed on the public builder/app APIs we verified".into()),
        0,
        0,
        serde_json::json!({}),
    )
    .await?;
    Ok(())
}

pub async fn upsert_health(
    stores: &Stores,
    venue: Venue,
    provider: &str,
    state: HealthState,
    last_error: Option<String>,
    observations: i64,
    market_count: i32,
    extra: Value,
) -> anyhow::Result<()> {
    let state = serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "UNKNOWN".into());
    let last_success = if last_error.is_none() {
        Some(Utc::now())
    } else {
        None
    };
    sqlx::query(
        r#"INSERT INTO pumphound.venue_provider_health
           (venue, provider, state, last_success_at, last_error, error_count_1h, observations, market_count, extra, updated_at)
           VALUES ($1,$2,$3,COALESCE($4, now()),$5, CASE WHEN $5 IS NULL THEN 0 ELSE 1 END, $6,$7,$8, now())
           ON CONFLICT (venue, provider) DO UPDATE SET
             state=EXCLUDED.state,
             last_success_at=CASE WHEN EXCLUDED.last_error IS NULL THEN now() ELSE pumphound.venue_provider_health.last_success_at END,
             last_error=EXCLUDED.last_error,
             error_count_1h=CASE WHEN EXCLUDED.last_error IS NULL THEN 0 ELSE pumphound.venue_provider_health.error_count_1h + 1 END,
             observations=EXCLUDED.observations,
             market_count=EXCLUDED.market_count,
             extra=EXCLUDED.extra,
             updated_at=now()"#,
    )
    .bind(venue.as_str())
    .bind(provider)
    .bind(state)
    .bind(last_success)
    .bind(last_error)
    .bind(observations)
    .bind(market_count)
    .bind(extra)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_instrument(
    stores: &Stores,
    venue: Venue,
    native_id: &str,
    display_name: &str,
    kind: &str,
    class: Option<&str>,
    dex: Option<&str>,
    deployer: Option<&str>,
    delisted: bool,
    listing_ts: Option<DateTime<Utc>>,
    meta: Value,
) -> anyhow::Result<DateTime<Utc>> {
    let row = sqlx::query(
        r#"INSERT INTO pumphound.venue_instruments
           (venue, native_id, display_name, instrument_kind, class, dex, deployer, first_observed_at, listing_ts, delisted, meta, updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7, now(), $8, $9, $10, now())
           ON CONFLICT (venue, native_id) DO UPDATE SET
             display_name=EXCLUDED.display_name,
             instrument_kind=EXCLUDED.instrument_kind,
             class=EXCLUDED.class,
             dex=EXCLUDED.dex,
             deployer=COALESCE(EXCLUDED.deployer, pumphound.venue_instruments.deployer),
             listing_ts=COALESCE(pumphound.venue_instruments.listing_ts, EXCLUDED.listing_ts),
             delisted=EXCLUDED.delisted,
             meta=EXCLUDED.meta,
             updated_at=now()
           RETURNING first_observed_at"#,
    )
    .bind(venue.as_str())
    .bind(native_id)
    .bind(display_name)
    .bind(kind)
    .bind(class)
    .bind(dex)
    .bind(deployer)
    .bind(listing_ts)
    .bind(delisted)
    .bind(meta)
    .fetch_one(&stores.postgres)
    .await?;
    Ok(row.get("first_observed_at"))
}

pub async fn insert_hl_quote(
    stores: &Stores,
    native_id: &str,
    source_ts: Option<DateTime<Utc>>,
    mark: OptionalNumber,
    mid: OptionalNumber,
    oracle: OptionalNumber,
    funding: OptionalNumber,
    oi: OptionalNumber,
    premium: OptionalNumber,
    volume: OptionalNumber,
    spread: OptionalNumber,
    impact_bid: OptionalNumber,
    impact_ask: OptionalNumber,
    lifecycle: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.hl_quotes
           (native_id, observed_at, source_ts, mark, mid, oracle, funding, open_interest, premium, day_notional_volume, spread, impact_bid, impact_ask, lifecycle)
           VALUES ($1, now(), $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(native_id)
    .bind(source_ts)
    .bind(opt(mark))
    .bind(opt(mid))
    .bind(opt(oracle))
    .bind(opt(funding))
    .bind(opt(oi))
    .bind(opt(premium))
    .bind(opt(volume))
    .bind(opt(spread))
    .bind(opt(impact_bid))
    .bind(opt(impact_ask))
    .bind(lifecycle)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_hl_l2(
    stores: &Stores,
    native_id: &str,
    source_ts: Option<DateTime<Utc>>,
    best_bid: OptionalNumber,
    best_ask: OptionalNumber,
    mid: OptionalNumber,
    top_levels: Value,
    depth: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.hl_l2_snapshots
           (native_id, observed_at, source_ts, best_bid, best_ask, mid, top_levels, depth)
           VALUES ($1, now(), $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(native_id)
    .bind(source_ts)
    .bind(opt(best_bid))
    .bind(opt(best_ask))
    .bind(opt(mid))
    .bind(top_levels)
    .bind(depth)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_hl_trade(
    stores: &Stores,
    native_id: &str,
    source_ts: DateTime<Utc>,
    px: OptionalNumber,
    sz: OptionalNumber,
    side: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.hl_trades (native_id, source_ts, px, sz, side)
           VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING"#,
    )
    .bind(native_id)
    .bind(source_ts)
    .bind(opt(px))
    .bind(opt(sz))
    .bind(side)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_predicted_funding(
    stores: &Stores,
    native_id: &str,
    venue: &str,
    rate: OptionalNumber,
    next_ts: Option<DateTime<Utc>>,
    interval_hours: Option<i32>,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.hl_predicted_fundings
           (native_id, venue, observed_at, funding_rate, next_funding_time, interval_hours)
           VALUES ($1,$2, now(), $3, $4, $5)"#,
    )
    .bind(native_id)
    .bind(venue)
    .bind(opt(rate))
    .bind(next_ts)
    .bind(interval_hours)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_age_snapshot(
    stores: &Stores,
    native_id: &str,
    target: i64,
    actual: DateTime<Utc>,
    delay: i64,
    payload: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.hl_age_snapshots
           (native_id, target_age_seconds, actual_observed_at, observation_delay_seconds, payload)
           VALUES ($1,$2,$3,$4,$5)
           ON CONFLICT (native_id, target_age_seconds) DO NOTHING"#,
    )
    .bind(native_id)
    .bind(target as i32)
    .bind(actual)
    .bind(delay as i32)
    .bind(payload)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_lifecycle(
    stores: &Stores,
    native_id: &str,
    state: &str,
    evidence: Value,
) -> anyhow::Result<()> {
    let last: Option<String> = sqlx::query_scalar(
        r#"SELECT state FROM pumphound.hl_lifecycle WHERE native_id=$1 ORDER BY observed_at DESC LIMIT 1"#,
    )
    .bind(native_id)
    .fetch_optional(&stores.postgres)
    .await?;
    if last.as_deref() == Some(state) {
        return Ok(());
    }
    sqlx::query(
        r#"INSERT INTO pumphound.hl_lifecycle (native_id, observed_at, state, rule_version, evidence)
           VALUES ($1, now(), $2, 'hl-lifecycle-v1', $3)"#,
    )
    .bind(native_id)
    .bind(state)
    .bind(evidence)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_paper_fill(stores: &Stores, native_id: &str, book: &str, fill: Value) -> anyhow::Result<()> {
    let order_id = uuid::Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO pumphound.hl_paper_orders
           (order_id, experiment_id, book, native_id, side, intent, fill)
           VALUES ($1,'HIP-NEW-MARKET-V1',$2,$3,'buy','{}',$4)"#,
    )
    .bind(order_id)
    .bind(book)
    .bind(native_id)
    .bind(&fill)
    .execute(&stores.postgres)
    .await?;
    sqlx::query(
        r#"INSERT INTO pumphound.hl_paper_traces (trace_id, order_id, payload) VALUES ($1,$2,$3)"#,
    )
    .bind(uuid::Uuid::now_v7())
    .bind(order_id)
    .bind(fill)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_ostium(
    stores: &Stores,
    pair_id: &str,
    symbol: &str,
    from: Option<&str>,
    to: Option<&str>,
    price: OptionalNumber,
    funding_long: OptionalNumber,
    funding_short: OptionalNumber,
    last_funding_rate: OptionalNumber,
    rollover: OptionalNumber,
    rollover_long: OptionalNumber,
    rollover_short: OptionalNumber,
    broker: OptionalNumber,
    neg_roll: Option<bool>,
    oi_long: OptionalNumber,
    oi_short: OptionalNumber,
    max_oi: OptionalNumber,
    util: OptionalNumber,
    impact_k: OptionalNumber,
    sim_status: &str,
    payload: Value,
    meta: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.ostium_markets (pair_id, symbol, from_asset, to_asset, first_observed_at, meta, updated_at)
           VALUES ($1,$2,$3,$4, now(), $5, now())
           ON CONFLICT (pair_id) DO UPDATE SET symbol=EXCLUDED.symbol, from_asset=EXCLUDED.from_asset, to_asset=EXCLUDED.to_asset, meta=EXCLUDED.meta, updated_at=now()"#,
    )
    .bind(pair_id)
    .bind(symbol)
    .bind(from)
    .bind(to)
    .bind(meta)
    .execute(&stores.postgres)
    .await?;
    sqlx::query(
        r#"INSERT INTO pumphound.ostium_observations
           (pair_id, observed_at, price, funding_long, funding_short, last_funding_rate, rollover, rollover_long_acc, rollover_short_acc, broker_premium, is_negative_rollover_allowed, oi_long, oi_short, max_oi_usd, utilization, price_impact_k, sim_slippage_status, payload)
           VALUES ($1, now(), $2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)"#,
    )
    .bind(pair_id)
    .bind(opt(price))
    .bind(opt(funding_long))
    .bind(opt(funding_short))
    .bind(opt(last_funding_rate))
    .bind(opt(rollover))
    .bind(opt(rollover_long))
    .bind(opt(rollover_short))
    .bind(opt(broker))
    .bind(neg_roll)
    .bind(opt(oi_long))
    .bind(opt(oi_short))
    .bind(opt(max_oi))
    .bind(opt(util))
    .bind(opt(impact_k))
    .bind(sim_status)
    .bind(payload)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_prediction_market(
    stores: &Stores,
    market_id: &str,
    event_id: Option<&str>,
    question: &str,
    yes_token: Option<&str>,
    no_token: Option<&str>,
    yes: OptionalNumber,
    no: OptionalNumber,
    bid: OptionalNumber,
    ask: OptionalNumber,
    spread: OptionalNumber,
    volume: OptionalNumber,
    liquidity: OptionalNumber,
    end_date: Option<DateTime<Utc>>,
    resolution_source: Option<&str>,
    resolution_rules: Option<&str>,
    resolved_by: Option<&str>,
    active: Option<bool>,
    closed: Option<bool>,
    payload: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.prediction_markets
           (market_id, event_id, question, yes_token_id, no_token_id, yes_price, no_price, best_bid, best_ask, spread, volume, liquidity, end_date, resolution_source, resolution_rules, resolved_by, active, closed, payload, updated_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19, now())
           ON CONFLICT (market_id) DO UPDATE SET
             event_id=EXCLUDED.event_id, question=EXCLUDED.question, yes_token_id=EXCLUDED.yes_token_id, no_token_id=EXCLUDED.no_token_id,
             yes_price=EXCLUDED.yes_price, no_price=EXCLUDED.no_price, best_bid=EXCLUDED.best_bid, best_ask=EXCLUDED.best_ask,
             spread=EXCLUDED.spread, volume=EXCLUDED.volume, liquidity=EXCLUDED.liquidity, end_date=EXCLUDED.end_date,
             resolution_source=EXCLUDED.resolution_source, resolution_rules=EXCLUDED.resolution_rules, resolved_by=EXCLUDED.resolved_by,
             active=EXCLUDED.active, closed=EXCLUDED.closed, payload=EXCLUDED.payload, updated_at=now()"#,
    )
    .bind(market_id)
    .bind(event_id)
    .bind(question)
    .bind(yes_token)
    .bind(no_token)
    .bind(opt(yes))
    .bind(opt(no))
    .bind(opt(bid))
    .bind(opt(ask))
    .bind(opt(spread))
    .bind(opt(volume))
    .bind(opt(liquidity))
    .bind(end_date)
    .bind(resolution_source)
    .bind(resolution_rules)
    .bind(resolved_by)
    .bind(active)
    .bind(closed)
    .bind(payload)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_prediction_event(
    stores: &Stores,
    event_id: &str,
    title: Option<&str>,
    slug: Option<&str>,
    category: Option<&str>,
    payload: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.prediction_events (event_id, title, slug, category, payload, updated_at)
           VALUES ($1,$2,$3,$4,$5, now())
           ON CONFLICT (event_id) DO UPDATE SET title=EXCLUDED.title, slug=EXCLUDED.slug, category=EXCLUDED.category, payload=EXCLUDED.payload, updated_at=now()"#,
    )
    .bind(event_id)
    .bind(title)
    .bind(slug)
    .bind(category)
    .bind(payload)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn insert_prediction_book(
    stores: &Stores,
    token_id: &str,
    bid: OptionalNumber,
    ask: OptionalNumber,
    mid: OptionalNumber,
    spread: OptionalNumber,
    book: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.prediction_books (token_id, observed_at, best_bid, best_ask, mid, spread, book)
           VALUES ($1, now(), $2,$3,$4,$5,$6)"#,
    )
    .bind(token_id)
    .bind(opt(bid))
    .bind(opt(ask))
    .bind(opt(mid))
    .bind(opt(spread))
    .bind(book)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn upsert_relation(
    stores: &Stores,
    left: &str,
    right: &str,
    relation_type: &str,
    provenance: &str,
    evidence: Value,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO pumphound.prediction_relations (left_id, right_id, relation_type, provenance, evidence, updated_at)
           VALUES ($1,$2,$3,$4,$5, now())
           ON CONFLICT (left_id, right_id, relation_type) DO UPDATE SET
             provenance=CASE
               WHEN pumphound.prediction_relations.provenance='PROVEN_RELATION' THEN pumphound.prediction_relations.provenance
               ELSE EXCLUDED.provenance
             END,
             evidence=EXCLUDED.evidence,
             updated_at=now()"#,
    )
    .bind(left)
    .bind(right)
    .bind(relation_type)
    .bind(provenance)
    .bind(evidence)
    .execute(&stores.postgres)
    .await?;
    Ok(())
}

pub async fn refresh_storage_stats(stores: &Stores) -> anyhow::Result<()> {
    let specs = [
        (
            "pump",
            vec!["launches", "feature_snapshots", "paper_orders", "ingestion_batches"],
            "pump journals + postgres identity; existing retention",
        ),
        (
            "hyperliquid",
            vec![
                "hl_quotes",
                "hl_l2_snapshots",
                "hl_trades",
                "hl_predicted_fundings",
                "hl_age_snapshots",
                "hl_lifecycle",
                "venue_instruments",
            ],
            "postgres quotes/lifecycle + 14d clickhouse venue_observations",
        ),
        (
            "ostium",
            vec!["ostium_markets", "ostium_observations"],
            "bounded pair snapshots; no sim tape yet",
        ),
        (
            "polymarket",
            vec![
                "prediction_markets",
                "prediction_events",
                "prediction_books",
                "prediction_relations",
            ],
            "gamma identity + sampled CLOB books",
        ),
    ];
    for (venue, tables, policy) in specs {
        let mut bytes: i64 = 0;
        for table in tables {
            bytes += sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(pg_total_relation_size(to_regclass($1)),0)",
            )
            .bind(format!("pumphound.{table}"))
            .fetch_one(&stores.postgres)
            .await
            .unwrap_or(0);
        }
        let now = Utc::now();
        sqlx::query(
            r#"INSERT INTO pumphound.venue_storage_stats
               (venue, measured_bytes, measured_at, first_measured_at, first_measured_bytes, bytes_per_minute, estimated_30d_bytes, estimated_90d_bytes, retention_policy, extra)
               VALUES ($1,$2, now(), now(), $2, NULL, NULL, NULL, $3, jsonb_build_object('basis','MEASURED','tables_bytes',$2))
               ON CONFLICT (venue) DO UPDATE SET
                 measured_bytes=EXCLUDED.measured_bytes,
                 measured_at=now(),
                 first_measured_at=COALESCE(pumphound.venue_storage_stats.first_measured_at, now()),
                 first_measured_bytes=COALESCE(pumphound.venue_storage_stats.first_measured_bytes, EXCLUDED.measured_bytes),
                 bytes_per_minute=CASE
                   WHEN pumphound.venue_storage_stats.first_measured_at IS NULL THEN NULL
                   WHEN EXTRACT(EPOCH FROM (now() - pumphound.venue_storage_stats.first_measured_at)) < 60 THEN NULL
                   ELSE (EXCLUDED.measured_bytes - COALESCE(pumphound.venue_storage_stats.first_measured_bytes, EXCLUDED.measured_bytes))
                        / GREATEST(EXTRACT(EPOCH FROM (now() - pumphound.venue_storage_stats.first_measured_at))/60.0, 1)
                 END,
                 estimated_30d_bytes=CASE
                   WHEN EXTRACT(EPOCH FROM (now() - COALESCE(pumphound.venue_storage_stats.first_measured_at, now()))) < 300 THEN NULL
                   ELSE ((EXCLUDED.measured_bytes - COALESCE(pumphound.venue_storage_stats.first_measured_bytes, EXCLUDED.measured_bytes))
                        / GREATEST(EXTRACT(EPOCH FROM (now() - pumphound.venue_storage_stats.first_measured_at)), 1)) * 86400 * 30
                 END,
                 estimated_90d_bytes=CASE
                   WHEN EXTRACT(EPOCH FROM (now() - COALESCE(pumphound.venue_storage_stats.first_measured_at, now()))) < 300 THEN NULL
                   ELSE ((EXCLUDED.measured_bytes - COALESCE(pumphound.venue_storage_stats.first_measured_bytes, EXCLUDED.measured_bytes))
                        / GREATEST(EXTRACT(EPOCH FROM (now() - pumphound.venue_storage_stats.first_measured_at)), 1)) * 86400 * 90
                 END,
                 retention_policy=EXCLUDED.retention_policy,
                 extra=jsonb_build_object('basis','MEASURED','as_of',$4::text)"#,
        )
        .bind(venue)
        .bind(bytes)
        .bind(policy)
        .bind(now.to_rfc3339())
        .execute(&stores.postgres)
        .await
        .with_context(|| format!("storage stats {venue}"))?;
    }
    Ok(())
}

pub async fn append_clickhouse(
    stores: &Stores,
    venue: &str,
    native_id: &str,
    kind: &str,
    source_ts: Option<DateTime<Utc>>,
    payload: &Value,
) -> anyhow::Result<()> {
    let url = std::env::var("PUMPHOUND_CLICKHOUSE_URL").unwrap_or_else(|_| "http://127.0.0.1:8123".into());
    let user = std::env::var("PUMPHOUND_CLICKHOUSE_USER").unwrap_or_else(|_| "pumphound".into());
    let pass = std::env::var("PUMPHOUND_CLICKHOUSE_PASSWORD").unwrap_or_else(|_| "pumphound".into());
    let row = serde_json::json!({
        "venue": venue,
        "native_id": native_id,
        "observed_at": Utc::now().to_rfc3339(),
        "source_ts": source_ts.map(|t| t.to_rfc3339()),
        "kind": kind,
        "payload": payload.to_string(),
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{url}/?database=pumphound&query=INSERT+INTO+venue_observations+FORMAT+JSONEachRow"
        ))
        .basic_auth(user, Some(pass))
        .body(row.to_string())
        .send()
        .await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::debug!(body, "clickhouse venue_observations insert skipped/failed");
    }
    let _ = stores;
    Ok(())
}
