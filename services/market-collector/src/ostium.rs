use crate::store;
use pumphound_storage::Stores;
use pumphound_venue_core::{HealthState, Venue};
use pumphound_venue_ostium::OstiumClient;

pub async fn tick(stores: &Stores) -> anyhow::Result<()> {
    let client = OstiumClient::default();
    let markets = client.fetch_merged().await?;
    for market in &markets {
        store::upsert_instrument(
            stores,
            Venue::Ostium,
            &market.pair_id,
            &market.symbol,
            "perpetual",
            market.group.as_deref(),
            None,
            None,
            false,
            None,
            serde_json::json!({
                "from": market.from_asset,
                "to": market.to_asset,
                "sim_slippage_status": market.sim_slippage_status
            }),
        )
        .await?;
        store::upsert_ostium(
            stores,
            &market.pair_id,
            &market.symbol,
            market.from_asset.as_deref(),
            market.to_asset.as_deref(),
            market.price,
            market.funding_long,
            market.funding_short,
            market.last_funding_rate,
            market.rollover,
            market.rollover_long_acc,
            market.rollover_short_acc,
            market.broker_premium,
            market.is_negative_rollover_allowed,
            market.oi_long,
            market.oi_short,
            market.max_oi_usd,
            market.utilization,
            market.price_impact_k,
            &market.sim_slippage_status,
            market.raw.clone(),
            serde_json::json!({"group": market.group}),
        )
        .await?;
    }
    store::upsert_health(
        stores,
        Venue::Ostium,
        "pairs",
        HealthState::Healthy,
        None,
        markets.len() as i64,
        markets.len() as i32,
        serde_json::json!({
            "builder": "https://builder.prod.bedrock.ostium.io/v1/pairs",
            "app": "https://app.ostium.io/api/pairs",
            "funding_vs_rollover": "stored_separately",
            "sim_slippage": "UNSUPPORTED"
        }),
    )
    .await?;
    Ok(())
}
