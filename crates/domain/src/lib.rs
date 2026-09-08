//! Canonical PumpHound domain types. JSON, storage and TypeScript are generated
//! from these definitions — do not hand-edit parallel copies.

pub mod events;
pub mod ids;
pub mod market;
pub mod paper;
pub mod quality;
pub mod raw;
pub mod time;
pub mod units;

pub use events::*;
pub use ids::*;
pub use market::*;
pub use paper::*;
pub use quality::*;
pub use raw::*;
pub use time::*;
pub use units::*;

pub const SCHEMA_VERSION: u32 = 1;
pub const DECODER_VERSION: &str = "pump-events-identity-v2-2026-09-08";
pub const CHAIN: &str = "solana-mainnet";

pub const PUMP_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMPSWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const PUMP_FEES_PROGRAM_ID: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const NATIVE_SOL_MINT: &str = "So11111111111111111111111111111111111111112";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mode {
    LiveWatch,
    LivePaper,
    Replay,
    Research,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    PumpCurve,
    PumpSwap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Canonicality {
    Provisional,
    Canonical,
    Orphaned,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Healthy,
    Delayed,
    Backfilling,
    Degraded,
    Disconnected,
    Inconsistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("invalid public key: {0}")]
    InvalidPubkey(String),
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("invalid atomic quantity: {0}")]
    InvalidAtomic(String),
    #[error("arithmetic overflow: {0}")]
    Overflow(&'static str),
    #[error("contract violation: {0}")]
    Contract(&'static str),
}
