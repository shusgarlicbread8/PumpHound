#![recursion_limit = "256"]

mod candidate_enrichment;
mod cockpit;
mod graph_view;
mod hot_set;
mod intelligence;
mod investigator;
mod launch_age;
mod ledger_metrics;
mod metadata;
mod model_client;
mod model_truth;
mod paper_ledger;
mod paper_loop;
mod readiness;
mod routes;
mod state;
mod stream;
mod truth;

use anyhow::Context;
use futures::StreamExt;
use pumphound_chain_source::SourceConfig;
use pumphound_domain::ChainEvent;
use pumphound_storage::Stores;
use state::AppState;
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let source = SourceConfig::from_env();
    println!("{}", source.label());
    let stores = Stores::connect().await.context("stores")?;
    paper_ledger::seed(&stores)
        .await
        .context("seed paper ledger")?;
    let state = Arc::new(AppState::new(stores.clone(), source.label().into()));
    *state.paper.write().await = paper_ledger::hydrate(&stores)
        .await
        .context("recover paper portfolios")?;
    consume_normalized(state.clone());
    paper_loop::spawn(state.clone());
    intelligence::spawn(state.clone());

    let bind = std::env::var("PUMPHOUND_API_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let app = routes::router(state);
    info!(%bind, source = %source.label(), "core-api listening");
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn consume_normalized(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            match state.stores.nats.subscribe("chain.normalized.v1").await {
                Ok(mut sub) => {
                    {
                        let mut h = state.health.write().await;
                        h.chain = "healthy".into();
                    }
                    while let Some(msg) = sub.next().await {
                        match serde_json::from_slice::<Vec<ChainEvent>>(&msg.payload) {
                            Ok(events) => {
                                let mut snapshots = Vec::new();
                                for ev in events {
                                    snapshots.extend(state.apply_event(&ev).await);
                                    // Sequence is assigned once here, at broadcast time, so every
                                    // subscriber sees the same number for the same event. Assigning
                                    // it per-connection (as stream.rs used to) meant two clients
                                    // watching at once — e.g. the desktop app plus a browser tab —
                                    // each stole sequence slots the other never saw, so every client
                                    // perceived a permanent gap and force-resynced on nearly every
                                    // message.
                                    let seq = state.next_seq().await;
                                    let _ = state.radar_tx.send(serde_json::json!({
                                        "topic": "radar",
                                        "event_id": ev.event_id,
                                        "kind": format!("{:?}", ev.payload).split('(').next(),
                                        "seq": seq,
                                    }));
                                }
                                if !snapshots.is_empty() {
                                    match state.stores.persist_market_snapshots(&snapshots).await {
                                        Ok(()) => {
                                            let mut health = state.health.write().await;
                                            health.last_market_snapshot_persisted_at =
                                                Some(pumphound_domain::now_utc());
                                            health.market_snapshot_persist_error = None;
                                        }
                                        Err(error) => {
                                            let mut health = state.health.write().await;
                                            health.market_snapshot_persist_error =
                                                Some(error.to_string());
                                            warn!(
                                                error = %error,
                                                count = snapshots.len(),
                                                "market snapshot persistence failed"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => warn!(error = %e, "bad canonical batch"),
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "nats subscribe failed");
                    let mut h = state.health.write().await;
                    h.chain = "disconnected".into();
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    });
}
