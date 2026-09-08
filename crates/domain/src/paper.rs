use crate::{
    AssetAmount, Atomic, Canonicality, DataQuality, FeeComponent, Mode, Pubkey, Slot, Timestamp,
    TradeSide,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperOrderRequest {
    pub client_request_id: Uuid,
    pub portfolio_id: Uuid,
    pub launch_id: Option<Uuid>,
    pub market: Pubkey,
    pub side: TradeSide,
    pub input: AssetAmount,
    pub output_mint: Pubkey,
    pub max_slippage_bps: u32,
    pub quote_id: Uuid,
    pub policy_version: String,
    pub decision_id: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperQuote {
    pub quote_id: Uuid,
    pub market_snapshot_id: String,
    pub input: AssetAmount,
    pub expected_output: AssetAmount,
    pub minimum_output: AssetAmount,
    pub reserve_input: AssetAmount,
    pub fee_components: Vec<FeeComponent>,
    pub price_impact_bps: u32,
    pub expires_at: Timestamp,
    pub quality: DataQuality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaperOrderStatus {
    Accepted,
    Queued,
    Filled,
    Cancelled,
    RejectedSlippage,
    RejectedLiquidity,
    RejectedStaleState,
    SimulatedTxFailure,
    RiskRejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionAction {
    Watch,
    Abstain,
    PaperBuy,
    PaperSell,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub decision_id: Uuid,
    pub mode: Mode,
    pub launch_id: Uuid,
    pub prediction_id: Option<String>,
    pub heuristic_version: Option<String>,
    pub policy_version: String,
    pub feature_snapshot_id: String,
    pub decision_at: Timestamp,
    pub knowledge_cutoff: Timestamp,
    pub action: DecisionAction,
    pub reason_codes: Vec<String>,
    pub risk_snapshot_id: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperFill {
    pub fill_id: Uuid,
    pub order_id: Uuid,
    pub simulation_run_id: Uuid,
    pub arrival_slot: Slot,
    pub arrival_at: Timestamp,
    pub input: AssetAmount,
    pub output: AssetAmount,
    pub fee_components: Vec<FeeComponent>,
    pub market_snapshot_id: String,
    pub canonicality: Canonicality,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcceptedPaperOrder {
    pub order_id: Uuid,
    pub request: PaperOrderRequest,
    pub quote: PaperQuote,
    pub accepted_at: Timestamp,
    pub arrival_at: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperOrderResult {
    pub order_id: Uuid,
    pub status: PaperOrderStatus,
    pub fill: Option<PaperFill>,
    pub reason_code: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum PaperError {
    #[error("quote unavailable: {0}")]
    QuoteUnavailable(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("risk rejected: {0}")]
    Risk(String),
    #[error("stale quote")]
    Stale,
    #[error("arithmetic: {0}")]
    Arithmetic(String),
}

pub trait PaperVenue {
    fn quote(
        &self,
        order: &PaperOrderRequest,
        state: &crate::MarketSnapshot,
    ) -> Result<PaperQuote, PaperError>;
    fn execute(
        &mut self,
        accepted: AcceptedPaperOrder,
        events: &[crate::ChainEvent],
    ) -> PaperOrderResult;
}

/// spendable(asset) = SUM(ledger signed_atomic) - SUM(active reservations)
pub fn spendable(ledger_sum: crate::AtomicSigned, reserved: Atomic) -> Result<i128, PaperError> {
    let reserved_i = i128::try_from(reserved.0)
        .map_err(|_| PaperError::Arithmetic("reservation overflow".into()))?;
    Ok(ledger_sum.0 - reserved_i)
}
