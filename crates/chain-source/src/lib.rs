mod backfill;
mod hot_mints;
mod laserstream;
mod replay;
mod websocket;

pub use backfill::backfill_since_checkpoint;
pub use hot_mints::{is_hot_mint, logs_mention_hot, replace_hot_mints};
pub use laserstream::HeliusLaserstreamSource;
pub use replay::ParquetReplaySource;
pub use websocket::{
    envelope_from_tx, fetch_transaction, parse_transaction, SolanaWebsocketSource,
};

use async_trait::async_trait;
use pumphound_domain::{ChainCursor, ChainSourceError, RawChainEnvelope};
use tokio::sync::mpsc;

#[async_trait]
pub trait ChainSource: Send {
    async fn run(
        &mut self,
        cursor: Option<ChainCursor>,
        sink: mpsc::Sender<RawChainEnvelope>,
    ) -> Result<(), ChainSourceError>;
}

#[derive(Clone, Debug)]
pub struct SourceConfig {
    pub rpc_url: String,
    pub ws_url: String,
    pub helius_api_key: Option<String>,
    pub laserstream_endpoint: Option<String>,
    pub laserstream_entitled: bool,
    pub program_ids: Vec<String>,
}

impl SourceConfig {
    pub fn from_env() -> Self {
        let helius = std::env::var("PUMPHOUND_HELIUS_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());
        let mut rpc = std::env::var("PUMPHOUND_SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".into());
        let mut ws = std::env::var("PUMPHOUND_SOLANA_WS_URL")
            .unwrap_or_else(|_| "wss://api.mainnet-beta.solana.com".into());
        if let Some(key) = &helius {
            if !rpc.contains(key) {
                rpc = format!("https://mainnet.helius-rpc.com/?api-key={key}");
                ws = format!("wss://mainnet.helius-rpc.com/?api-key={key}");
            }
        }
        Self {
            rpc_url: rpc,
            ws_url: ws,
            helius_api_key: helius,
            laserstream_endpoint: std::env::var("PUMPHOUND_LASERSTREAM_ENDPOINT").ok(),
            laserstream_entitled: std::env::var("PUMPHOUND_LASERSTREAM_ENTITLED")
                .ok()
                .as_deref()
                == Some("1"),
            program_ids: vec![
                pumphound_domain::PUMP_PROGRAM_ID.into(),
                pumphound_domain::PUMPSWAP_PROGRAM_ID.into(),
            ],
        }
    }

    pub fn label(&self) -> &'static str {
        let mode = std::env::var("PUMPHOUND_MODE").unwrap_or_default();
        if mode.eq_ignore_ascii_case("REPLAY") {
            return "REPLAY";
        }
        if self.laserstream_entitled && self.helius_api_key.is_some() {
            "LIVE MAINNET / LASERSTREAM"
        } else {
            "LIVE MAINNET / RPC FALLBACK"
        }
    }
}

pub async fn open_source(cfg: &SourceConfig) -> Box<dyn ChainSource> {
    let mode = std::env::var("PUMPHOUND_MODE").unwrap_or_default();
    if mode.eq_ignore_ascii_case("REPLAY") {
        let path = std::env::var("PUMPHOUND_REPLAY_PATH")
            .unwrap_or_else(|_| "fixtures/chain/sample_logs.json".into());
        let speed = std::env::var("PUMPHOUND_REPLAY_SPEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        return Box::new(ParquetReplaySource::new(path, speed));
    }
    if cfg.laserstream_entitled && cfg.helius_api_key.is_some() {
        Box::new(HeliusLaserstreamSource::new(cfg.clone()))
    } else {
        Box::new(SolanaWebsocketSource::new(cfg.clone()))
    }
}
