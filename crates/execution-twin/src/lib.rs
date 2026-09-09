//! PumpHound Execution Twin — PAPER ONLY.
//!
//! Turns a strategy decision into an exact, reproducible simulated Solana execution:
//! intent → canonical Pump/PumpSwap transaction → fee decomposition → landing sample →
//! arrival-state protocol-exact fill or classified failure → cost attribution.
//!
//! There is deliberately no signer, no keypair, no RPC submission path in this crate.

pub mod contract;
pub mod fee_config;
pub mod pda;
pub mod pump_math;

pub use contract::*;
