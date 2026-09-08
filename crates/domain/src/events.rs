use crate::{
    AssetAmount, Atomic, Canonicality, Commitment, Protocol, Provenance, Pubkey, Slot, Timestamp,
    TradeSide,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventAction {
    Upsert,
    Retract,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub schema_version: u32,
    pub event_id: String,
    pub revision: u64,
    pub action: EventAction,
    pub chain: String,
    pub protocol: Protocol,
    pub program_id: Pubkey,
    pub signature: String,
    pub slot: Slot,
    pub blockhash: Option<String>,
    pub tx_index: Option<u32>,
    pub instruction_path: Vec<u32>,
    pub event_index: u32,
    pub block_time: Option<Timestamp>,
    pub observed_at: Timestamp,
    pub computed_at: Timestamp,
    pub available_from_slot: Slot,
    pub commitment: Commitment,
    pub canonicality: Canonicality,
    pub transaction_succeeded: bool,
    pub provenance: Provenance,
    pub payload: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ChainEventPayload {
    #[serde(rename = "launch.created")]
    LaunchCreated(LaunchCreated),
    #[serde(rename = "trade.executed")]
    TradeExecuted(TradeExecuted),
    #[serde(rename = "transfer.observed")]
    TransferObserved(TransferObserved),
    #[serde(rename = "launch.migrated")]
    LaunchMigrated(LaunchMigrated),
    #[serde(rename = "launch.completed")]
    LaunchCompleted(LaunchCompleted),
    #[serde(rename = "liquidity.changed")]
    LiquidityChanged(LiquidityChanged),
    #[serde(rename = "protocol.state_changed")]
    ProtocolStateChanged(ProtocolStateChanged),
    #[serde(rename = "decode.quarantined")]
    DecodeQuarantined(DecodeQuarantined),
}

pub type ChainEvent = EventEnvelope<ChainEventPayload>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchCreated {
    pub launch_id: String,
    pub mint: Pubkey,
    pub creator: Option<Pubkey>,
    pub market: Pubkey,
    pub quote_mint: Pubkey,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub supply_atomic: Option<Atomic>,
    pub metadata_uri: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub protocol_flags: serde_json::Value,
    pub virtual_base_atomic: Option<Atomic>,
    pub virtual_quote_atomic: Option<Atomic>,
    pub real_base_atomic: Option<Atomic>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TradeExecuted {
    pub launch_id: Option<String>,
    pub mint: Pubkey,
    pub market: Pubkey,
    pub trader: Pubkey,
    pub fee_payer: Pubkey,
    pub side: TradeSide,
    pub base_amount_atomic: Atomic,
    pub quote_amount_atomic: Atomic,
    pub quote_mint: Pubkey,
    pub fee_components: Vec<FeeComponent>,
    pub reserve_snapshot_id: Option<String>,
    pub instruction_name: String,
    pub virtual_base_atomic: Option<Atomic>,
    pub virtual_quote_atomic: Option<Atomic>,
    pub real_base_atomic: Option<Atomic>,
    pub real_quote_atomic: Option<Atomic>,
    pub raw_event: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferObserved {
    pub source_owner: Option<Pubkey>,
    pub destination_owner: Option<Pubkey>,
    pub source_account: Pubkey,
    pub destination_account: Pubkey,
    pub amount: AssetAmount,
    pub transfer_role: TransferRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferRole {
    External,
    TradeSettlement,
    Fee,
    Rent,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchMigrated {
    pub launch_id: String,
    pub mint: Pubkey,
    pub old_market: Pubkey,
    pub new_market: Pubkey,
    pub quote_mint: Pubkey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchCompleted {
    pub launch_id: String,
    pub mint: Pubkey,
    pub market: Pubkey,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiquidityChanged {
    pub mint: Pubkey,
    pub market: Pubkey,
    pub lp_provider: Option<Pubkey>,
    pub base_delta_atomic: Atomic,
    pub quote_delta_atomic: Atomic,
    pub direction: LiquidityDirection,
    pub instruction_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiquidityDirection {
    Deposit,
    Withdraw,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtocolStateChanged {
    pub account: Pubkey,
    pub config_snapshot_id: String,
    pub affected_markets: Vec<Pubkey>,
    pub decoded_config: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecodeQuarantined {
    pub discriminator_hex: String,
    pub program_id: String,
    pub reason: String,
    #[serde(default)]
    pub payload_length: Option<usize>,
    #[serde(default)]
    pub payload_sha256: Option<String>,
    #[serde(default)]
    pub decoder_version: String,
    #[serde(default)]
    pub supporting_signature: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeeComponent {
    pub kind: FeeKind,
    pub asset: AssetAmount,
    pub schedule_version: String,
    pub charged_on_failure: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeKind {
    Protocol,
    Creator,
    Lp,
    Network,
    Priority,
    Tip,
    Rent,
    Cashback,
    Buyback,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unknown discriminator {0}")]
    UnknownDiscriminator(String),
    #[error("borsh decode failed: {0}")]
    Borsh(String),
    #[error("missing program context")]
    MissingProgramContext,
    #[error("incompatible layout: {0}")]
    IncompatibleLayout(String),
}

pub trait LaunchpadDecoder: Send + Sync {
    fn protocol(&self) -> Protocol;
    fn program_ids(&self) -> &[String];
    fn decode(&self, tx: &crate::raw::RawTransaction) -> Result<Vec<ChainEvent>, DecodeError>;
}
