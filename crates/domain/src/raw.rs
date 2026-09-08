use crate::{Commitment, Slot, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainCursor {
    pub source: String,
    pub subscription_id: String,
    pub slot: Option<Slot>,
    pub signature: Option<String>,
    pub provider_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawChainEnvelope {
    pub sighting_id: String,
    pub source: String,
    pub source_session: String,
    pub source_cursor: String,
    pub slot: Slot,
    pub blockhash: Option<String>,
    pub parent_slot: Option<Slot>,
    pub commitment: Commitment,
    pub observed_at: Timestamp,
    pub payload_sha256: String,
    pub kind: RawKind,
    pub payload: Value,
    pub transport: TransportMeta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawKind {
    Transaction,
    Account,
    Slot,
    BlockMeta,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransportMeta {
    pub source_lag_ms: Option<i64>,
    pub received_bytes: u64,
    pub reconnect_generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DurableBatch {
    pub batch_id: uuid::Uuid,
    pub content_sha256: String,
    pub rows: Vec<RawChainEnvelope>,
    pub cursor: ChainCursor,
    pub journal_uri: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawTransaction {
    pub signature: String,
    pub slot: Slot,
    pub block_time: Option<Timestamp>,
    pub transaction_index: Option<u32>,
    pub err: Option<Value>,
    pub fee_payer: Option<String>,
    pub account_keys: Vec<String>,
    pub loaded_writable_addresses: Vec<String>,
    pub loaded_readonly_addresses: Vec<String>,
    pub instructions: Vec<RawInstruction>,
    pub inner_instructions: Vec<RawInnerInstructions>,
    pub log_messages: Vec<String>,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    pub pre_token_balances: Vec<Value>,
    pub post_token_balances: Vec<Value>,
    pub compute_units_consumed: Option<u64>,
    pub raw_json: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawInstruction {
    pub program_id: String,
    pub accounts: Vec<String>,
    pub data_base64: String,
    pub stack_height: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawInnerInstructions {
    pub index: u32,
    pub instructions: Vec<RawInstruction>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChainSourceError {
    #[error("source disconnected: {0}")]
    Disconnected(String),
    #[error("source configuration: {0}")]
    Config(String),
    #[error("decode transport: {0}")]
    Transport(String),
    #[error("io: {0}")]
    Io(String),
}
