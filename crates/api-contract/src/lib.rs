use pumphound_domain::{DataQuality, ObservationPoint};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RadarRow {
    pub launch_id: String,
    pub mint: String,
    pub symbol: Option<String>,
    pub creator: Option<String>,
    pub age_seconds: Option<f64>,
    #[serde(default)]
    pub age_basis: Option<String>,
    pub protocol: String,
    pub quote_mint: String,
    pub price_quote: Option<String>,
    pub executable_depth_quote: Option<String>,
    pub curve_progress: Option<f64>,
    pub buyers: u64,
    pub independent_clusters: Option<u64>,
    pub flow_60s_quote: Option<String>,
    pub graduation_6h: Option<f64>,
    pub collapse_1h: Option<f64>,
    pub heuristic_rank: Option<f64>,
    pub prediction_id: Option<String>,
    pub stance: String,
    pub paper_position_id: Option<String>,
    pub flags: Vec<Flag>,
    pub quality: DataQuality,
    pub as_of: ObservationPoint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flag {
    pub code: String,
    pub evidence_id: String,
    pub severity: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamMessage<T> {
    #[serde(rename = "type")]
    pub kind: String,
    pub schema_version: u32,
    pub epoch: String,
    pub sequence: String,
    pub topic: String,
    pub sent_at: String,
    pub as_of_slot: Option<String>,
    pub payload: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub error: ApiError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub request_id: String,
    pub details: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ready: bool,
    pub mode: String,
    pub uptime_seconds: u64,
    pub decoder_version: String,
    pub source_label: String,
    pub model_version: Option<String>,
}
