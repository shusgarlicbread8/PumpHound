//! Paper-only Solana execution twin for Pump / PumpSwap.
//!
//! No signer. No wallet. No RPC submission. Real transaction construction exists solely so the
//! paper ledger charges what mainnet would have charged and fills what mainnet would have filled.

pub mod builder;
pub mod contract;
pub mod fee_config;
pub mod landing;
pub mod mark;
pub mod network;
pub mod pda;
pub mod pump_math;
pub mod quote;
pub mod twin;

pub use contract::*;
