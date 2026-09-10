use crate::store;
use anyhow::Context;
use chrono::Utc;
use pumphound_hyperliquid_research::{eligible, microstructure_score, random_pick, Book};
use pumphound_hyperliquid_twin::{fee_quote, simulate_taker, FeeInputs, OrderIntent, OrderType, Side};
use pumphound_storage::Stores;
use pumphound_venue_core::{HealthState, OptionalNumber, Venue};
use pumphound_venue_hyperliquid::{
    age_snapshot, classify_lifecycle, depth_notional, HipClass, HyperliquidClient, LifecycleInput,
    AGE_TARGETS_SECONDS,
};
use std::sync::atomic::{AtomicU64, Ordering};

static TICK: AtomicU64 = AtomicU64::new(0);
static LAST_PAPER: AtomicU64 = AtomicU64::new(0);

pub async fn tick(stores: &Stores) -> anyhow::Result<()> {
    let n = TICK.fetch_add(1, Ordering::Relaxed);
    let client = HyperliquidClient::default();
    let dexs = client.perp_dexs().await.context("perpDexs")?;
    let mut deployer_by_dex = std::collections::BTreeMap::new();
    for dex in &dexs {
        if let Some(deployer) = &dex.deployer {
            deployer_by_dex.insert(dex.dex.clone(), deployer.clone());
        }
    }

    let mut instruments = client.universe("").await.context("native universe")?;
    for dex in dexs.iter().filter(|d| !d.dex.is_empty()) {
        match client.universe(&dex.dex).await {
            Ok(mut rows) => instruments.append(&mut rows),
            Err(error) => tracing::warn!(dex = %dex.dex, %error, "hip3 universe failed"),
        }
    }

    let now = Utc::now();
    let mut first_seen = Vec::new();
    for inst in &instruments {
        let deployer = deployer_by_dex.get(&inst.dex).cloned();
        let class = match inst.class {
            HipClass::Native => "native",
            HipClass::Hip3 => "hip3",
            HipClass::Unknown => "unknown",
        };
        let first = store::upsert_instrument(
            stores,
            Venue::Hyperliquid,
            &inst.coin,
            &inst.display_name,
            "perpetual",
            Some(class),
            Some(&inst.dex),
            deployer.as_deref(),
            inst.delisted,
            None,
            serde_json::json!({
                "hip1_split": if inst.class == HipClass::Native { "UNKNOWN" } else { "n/a" },
                "growth_mode": inst.growth_mode,
                "deployer_fee_scale": inst.deployer_fee_scale.as_f64(),
                "max_leverage": inst.max_leverage,
                "sz_decimals": inst.sz_decimals,
                "age_basis": "first_observed_by_this_collector_not_listing_time"
            }),
        )
        .await?;
        first_seen.push((inst, first));
    }

    let mut l2_targets = Vec::new();
    for (inst, first) in &first_seen {
        let age = now.signed_duration_since(*first).num_seconds().max(0);
        let spread = match (inst.mark.as_f64(), inst.mid.as_f64()) {
            (Some(m), Some(mid)) if mid > 0.0 => OptionalNumber::observed((m - mid).abs() / mid),
            _ => OptionalNumber::unknown(),
        };
        let life = classify_lifecycle(&LifecycleInput {
            first_observed_at: *first,
            now,
            delisted: inst.delisted,
            listing_known: false,
            day_notional_volume: inst.day_notional_volume,
            depth_25k_available: None,
            spread,
        });
        let life_s = serde_json::to_value(life)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "DISCOVERED".into());
        if inst.class == HipClass::Hip3 || n % 3 == 0 {
            store::insert_hl_quote(
                stores,
                &inst.coin,
                None,
                inst.mark,
                inst.mid,
                inst.oracle,
                inst.funding,
                inst.open_interest,
                inst.premium,
                inst.day_notional_volume,
                spread,
                inst.impact_bid,
                inst.impact_ask,
                &life_s,
            )
            .await?;
        }
        store::insert_lifecycle(
            stores,
            &inst.coin,
            &life_s,
            serde_json::json!({
                "age_seconds": age,
                "volume": inst.day_notional_volume.as_f64(),
                "first_observed_at": first.to_rfc3339()
            }),
        )
        .await?;
        for target in AGE_TARGETS_SECONDS {
            if age >= target {
                let snap = age_snapshot(*first, now, target);
                store::upsert_age_snapshot(
                    stores,
                    &inst.coin,
                    snap.target_age_seconds,
                    snap.actual_observed_at,
                    snap.observation_delay_seconds,
                    serde_json::json!({
                        "mark": inst.mark.as_f64(),
                        "mid": inst.mid.as_f64(),
                        "oi": inst.open_interest.as_f64(),
                        "funding": inst.funding.as_f64(),
                        "volume": inst.day_notional_volume.as_f64(),
                        "lifecycle": life_s
                    }),
                )
                .await?;
            }
        }
        if inst.coin == "BTC" || inst.coin == "ETH" || inst.class == HipClass::Hip3 && age < 3600 {
            l2_targets.push(inst.coin.clone());
        } else if inst.class == HipClass::Hip3 {
            if inst.day_notional_volume.as_f64().unwrap_or(0.0) > 0.0 {
                l2_targets.push(inst.coin.clone());
            }
        }
    }
    l2_targets.sort();
    l2_targets.dedup();
    let rotate = (n as usize) % 32;
    let rest: Vec<String> = l2_targets
        .iter()
        .filter(|c| *c != "BTC" && *c != "ETH")
        .cloned()
        .collect();
    let rotated = if rest.is_empty() {
        Vec::new()
    } else {
        rest.iter()
            .cycle()
            .skip(rotate % rest.len())
            .take(14)
            .cloned()
            .collect()
    };
    let sampled: Vec<String> = l2_targets
        .iter()
        .filter(|c| *c == "BTC" || *c == "ETH")
        .cloned()
        .chain(rotated)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .take(16)
        .collect();

    let mut books = 0u32;
    let mut last_book = None;
    for coin in &sampled {
        match client.l2_book(coin).await {
            Ok(book) => {
                let inst = instruments.iter().find(|i| i.coin == *coin);
                let mid = inst.map(|i| i.mid).unwrap_or_else(OptionalNumber::unknown);
                let depth = depth_notional(&book, mid, true);
                let best_bid = book.bids.first().map(|l| l.px).unwrap_or_else(OptionalNumber::unknown);
                let best_ask = book.asks.first().map(|l| l.px).unwrap_or_else(OptionalNumber::unknown);
                let top = serde_json::json!({
                    "bids": book.bids.iter().take(5).map(|l| serde_json::json!({"px": l.px.as_f64(), "sz": l.sz.as_f64(), "n": l.n})).collect::<Vec<_>>(),
                    "asks": book.asks.iter().take(5).map(|l| serde_json::json!({"px": l.px.as_f64(), "sz": l.sz.as_f64(), "n": l.n})).collect::<Vec<_>>(),
                });
                store::insert_hl_l2(
                    stores,
                    coin,
                    book.source_ts,
                    best_bid,
                    best_ask,
                    mid,
                    top,
                    serde_json::to_value(&depth).unwrap_or_default(),
                )
                .await?;
                let _ = store::append_clickhouse(
                    stores,
                    "hyperliquid",
                    coin,
                    "l2",
                    book.source_ts,
                    &serde_json::json!({"best_bid": best_bid.as_f64(), "best_ask": best_ask.as_f64()}),
                )
                .await;
                books += 1;
                last_book = Some((coin.clone(), book, mid, inst.cloned()));
            }
            Err(error) => tracing::warn!(coin, %error, "l2Book failed"),
        }
        if n % 3 == 0 && coin.contains(':') {
            if let Ok(trades) = client.recent_trades(coin).await {
                for trade in trades.into_iter().take(20) {
                    if let Some(ts) = trade.source_ts {
                        store::insert_hl_trade(
                            stores,
                            coin,
                            ts,
                            trade.px,
                            trade.sz,
                            trade.side.as_deref(),
                        )
                        .await?;
                    }
                }
            }
        }
    }

    if n % 3 == 0 {
        if let Ok(pred) = client.predicted_fundings().await {
            for row in pred.into_iter().take(400) {
                store::insert_predicted_funding(
                    stores,
                    &row.coin,
                    &row.venue,
                    row.funding_rate,
                    row.next_funding_time,
                    row.interval_hours.map(|v| v as i32),
                )
                .await?;
            }
        }
    }

    let mut universe = Vec::new();
    for (inst, first) in &first_seen {
        if let Some(row) = eligible(now, inst, *first) {
            universe.push(row);
        }
    }
    let salt = now.format("%Y-%m-%d-%H").to_string();
    let random = random_pick(&universe, &salt).map(|r| r.coin.clone());
    let mut scored: Vec<_> = universe
        .iter()
        .filter_map(|row| microstructure_score(row).as_f64().map(|s| (s, row.coin.clone())))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let micro = scored.first().map(|(_, c)| c.clone());

    let last_paper = LAST_PAPER.load(Ordering::Relaxed);
    if n.saturating_sub(last_paper) >= 2 {
        if let (Some((coin, book, mid, Some(inst))), Some(target)) =
            (last_book, micro.clone().or(random.clone()))
        {
            if coin == target || inst.class == HipClass::Hip3 {
                let fees = fee_quote(&FeeInputs {
                    class: inst.class,
                    deployer_fee_scale: inst.deployer_fee_scale,
                    growth_mode: inst.growth_mode,
                    referral_discount: OptionalNumber::unknown(),
                });
                let fill = simulate_taker(
                    &book,
                    mid,
                    &OrderIntent {
                        coin: coin.clone(),
                        side: Side::Buy,
                        order_type: OrderType::Market,
                        notional_usd: 1_000.0,
                        limit_px: OptionalNumber::unknown(),
                        leverage: OptionalNumber::observed(2.0),
                        decision_ts: now,
                        latency_ms: 80,
                    },
                    &fees,
                    Some(0),
                    5_000,
                    inst.funding,
                    OptionalNumber::observed(1.0),
                );
                store::insert_paper_fill(
                    stores,
                    &coin,
                    Book::MicrostructureBaseline.as_str(),
                    serde_json::to_value(&fill).unwrap_or_default(),
                )
                .await?;
                LAST_PAPER.store(n, Ordering::Relaxed);
            }
        }
    }

    store::upsert_health(
        stores,
        Venue::Hyperliquid,
        "info",
        HealthState::Healthy,
        None,
        first_seen.len() as i64,
        instruments.len() as i32,
        serde_json::json!({
            "native": instruments.iter().filter(|i| i.class == HipClass::Native).count(),
            "hip3": instruments.iter().filter(|i| i.class == HipClass::Hip3).count(),
            "l2_sampled": books,
            "eligible_new_or_hip3": universe.len(),
            "random_pick": random,
            "microstructure_pick": micro,
            "hip1_split": "UNKNOWN"
        }),
    )
    .await?;
    Ok(())
}
