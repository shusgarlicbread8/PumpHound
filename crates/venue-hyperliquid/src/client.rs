use reqwest::Client;
use serde_json::{json, Value};
use thiserror::Error;

use crate::normalize::{
    parse_l2_book, parse_meta_and_ctxs, parse_perp_dexs, parse_predicted_fundings, parse_recent_trades,
    HyperliquidInstrument, L2Book, PerpDex, PredictedFunding, RecentTrade,
};
use crate::INFO_URL;

#[derive(Debug, Error)]
pub enum HyperliquidError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("decode: {0}")]
    Decode(String),
}

#[derive(Clone)]
pub struct HyperliquidClient {
    http: Client,
    info_url: String,
}

impl Default for HyperliquidClient {
    fn default() -> Self {
        Self::new(INFO_URL)
    }
}

impl HyperliquidClient {
    pub fn new(info_url: impl Into<String>) -> Self {
        Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("hyperliquid http"),
            info_url: info_url.into(),
        }
    }

    async fn post(&self, body: Value) -> Result<Value, HyperliquidError> {
        let response = self
            .http
            .post(&self.info_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| HyperliquidError::Transport(e.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| HyperliquidError::Transport(e.to_string()))?;
        if !status.is_success() {
            return Err(HyperliquidError::Transport(format!(
                "HTTP {status}: {}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(160)])
            )));
        }
        serde_json::from_slice(&bytes).map_err(|e| HyperliquidError::Decode(e.to_string()))
    }

    pub async fn perp_dexs(&self) -> Result<Vec<PerpDex>, HyperliquidError> {
        let value = self.post(json!({"type": "perpDexs"})).await?;
        parse_perp_dexs(&value).map_err(HyperliquidError::Decode)
    }

    pub async fn universe(&self, dex: &str) -> Result<Vec<HyperliquidInstrument>, HyperliquidError> {
        let mut body = json!({"type": "metaAndAssetCtxs"});
        if !dex.is_empty() {
            body["dex"] = json!(dex);
        }
        let value = self.post(body).await?;
        parse_meta_and_ctxs(dex, &value).map_err(HyperliquidError::Decode)
    }

    pub async fn l2_book(&self, coin: &str) -> Result<L2Book, HyperliquidError> {
        let value = self
            .post(json!({"type": "l2Book", "coin": coin}))
            .await?;
        parse_l2_book(&value).map_err(HyperliquidError::Decode)
    }

    pub async fn recent_trades(&self, coin: &str) -> Result<Vec<RecentTrade>, HyperliquidError> {
        let value = self
            .post(json!({"type": "recentTrades", "coin": coin}))
            .await?;
        parse_recent_trades(&value).map_err(HyperliquidError::Decode)
    }

    pub async fn predicted_fundings(&self) -> Result<Vec<PredictedFunding>, HyperliquidError> {
        let value = self.post(json!({"type": "predictedFundings"})).await?;
        parse_predicted_fundings(&value).map_err(HyperliquidError::Decode)
    }
}
