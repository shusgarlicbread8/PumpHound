//! Isolated non-Solana market collector. Does not use Helius.

mod hyperliquid;
mod ostium;
mod polymarket;
mod store;

use anyhow::Context;
use pumphound_storage::Stores;
use pumphound_venue_core::{HealthState, Venue};
use std::time::Duration;
use tracing::{error, info};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let stores = Stores::connect().await.context("stores")?;
    store::seed_static_health(&stores).await?;
    info!("market-collector starting (hyperliquid/ostium/polymarket; no Helius)");

    let hl = stores.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = hyperliquid::tick(&hl).await {
                error!(?error, "hyperliquid tick failed");
                let _ = store::upsert_health(
                    &hl,
                    Venue::Hyperliquid,
                    "info",
                    HealthState::Degraded,
                    Some(error.to_string()),
                    0,
                    0,
                    serde_json::json!({"isolated": true}),
                )
                .await;
            }
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });

    let ostium = stores.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = ostium::tick(&ostium).await {
                error!(?error, "ostium tick failed");
                let _ = store::upsert_health(
                    &ostium,
                    Venue::Ostium,
                    "pairs",
                    HealthState::Degraded,
                    Some(error.to_string()),
                    0,
                    0,
                    serde_json::json!({"isolated": true}),
                )
                .await;
            }
            tokio::time::sleep(Duration::from_secs(45)).await;
        }
    });

    let poly = stores.clone();
    tokio::spawn(async move {
        loop {
            if let Err(error) = polymarket::tick(&poly).await {
                error!(?error, "polymarket tick failed");
                let _ = store::upsert_health(
                    &poly,
                    Venue::Polymarket,
                    "gamma",
                    HealthState::Degraded,
                    Some(error.to_string()),
                    0,
                    0,
                    serde_json::json!({"isolated": true}),
                )
                .await;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    loop {
        if let Err(error) = store::refresh_storage_stats(&stores).await {
            error!(?error, "storage stats");
        }
        tokio::time::sleep(Duration::from_secs(120)).await;
    }
}
