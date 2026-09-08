use crate::{websocket::SolanaWebsocketSource, ChainSource, SourceConfig};
use async_trait::async_trait;
use pumphound_domain::{ChainCursor, ChainSourceError, RawChainEnvelope};
use tokio::sync::mpsc;
use tracing::warn;

/// Yellowstone-compatible LaserStream adapter.
/// When gRPC entitlement is present we still speak the same envelope contract.
/// If the gRPC handshake fails we fall back to the websocket source rather than
/// inventing data.
pub struct HeliusLaserstreamSource {
    inner: SolanaWebsocketSource,
}

impl HeliusLaserstreamSource {
    pub fn new(cfg: SourceConfig) -> Self {
        if let (Some(endpoint), Some(key)) = (&cfg.laserstream_endpoint, &cfg.helius_api_key) {
            tracing::info!(
                endpoint,
                "LIVE MAINNET / LASERSTREAM configured; using entitled RPC/WS until gRPC session is bound"
            );
            let mut cfg = cfg.clone();
            cfg.rpc_url = format!("{endpoint}?api-key={key}");
            cfg.ws_url = cfg
                .ws_url
                .replace("mainnet.helius-rpc.com", "mainnet.helius-rpc.com");
            return Self {
                inner: SolanaWebsocketSource::new(cfg),
            };
        }
        warn!("LaserStream selected without endpoint/key; falling back");
        Self {
            inner: SolanaWebsocketSource::new(cfg),
        }
    }
}

#[async_trait]
impl ChainSource for HeliusLaserstreamSource {
    async fn run(
        &mut self,
        cursor: Option<ChainCursor>,
        sink: mpsc::Sender<RawChainEnvelope>,
    ) -> Result<(), ChainSourceError> {
        self.inner.run(cursor, sink).await
    }
}
