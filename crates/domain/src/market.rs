use crate::{Atomic, Canonicality, DataQuality, ObservationPoint, Protocol, Pubkey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketSnapshot {
    pub snapshot_id: String,
    pub market: Pubkey,
    pub mint: Pubkey,
    pub quote_mint: Pubkey,
    pub protocol: Protocol,
    pub as_of: ObservationPoint,
    pub canonicality: Canonicality,
    pub virtual_base_atomic: Atomic,
    pub virtual_quote_atomic: Atomic,
    pub real_base_atomic: Atomic,
    pub real_quote_atomic: Atomic,
    pub effective_base_atomic: Atomic,
    pub effective_quote_atomic: Atomic,
    pub complete: bool,
    pub migrated_market: Option<Pubkey>,
    pub config_snapshot_id: String,
    pub quality: DataQuality,
    /// Protocol flags needed for exact quoting. `None` = unknown (not false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_mayhem_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator_fee_applies: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_supply_atomic: Option<Atomic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cashback_fee_bps: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketDelta {
    pub market: Pubkey,
    pub snapshot: MarketSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeatureVector {
    pub feature_snapshot_id: String,
    pub launch_id: String,
    pub observation_window_seconds: u32,
    pub as_of: ObservationPoint,
    pub schema_version: String,
    pub values: serde_json::Map<String, serde_json::Value>,
    pub dependency_ids: Vec<String>,
    pub quality: DataQuality,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictionOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p10: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PredictionBundle {
    pub prediction_id: String,
    pub feature_snapshot_id: String,
    pub model_version: String,
    pub calibrated: bool,
    pub computed_at: crate::Timestamp,
    pub outputs: serde_json::Map<String, serde_json::Value>,
    pub explanation_id: Option<String>,
    pub abstention_reason: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("unknown market")]
    UnknownMarket,
    #[error("inconsistent state: {0}")]
    Inconsistent(String),
    #[error("frozen quotes: {0}")]
    Frozen(String),
    #[error("overflow")]
    Overflow,
}

pub trait MarketReducer {
    fn apply(&mut self, event: &crate::ChainEvent) -> Result<Vec<MarketDelta>, StateError>;
    fn snapshot(&self, as_of: ObservationPoint) -> Option<MarketSnapshot>;
}
