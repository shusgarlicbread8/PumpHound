//! Hyperliquid public/read-only adapter. No signing, no orders.

mod client;
mod lifecycle;
mod normalize;

pub use client::{HyperliquidClient, HyperliquidError};
pub use lifecycle::{age_snapshot, classify_lifecycle, AgeSnapshot, Lifecycle, LifecycleInput};
pub use normalize::{
    depth_notional, parse_l2_book, parse_meta_and_ctxs, parse_perp_dexs, parse_predicted_fundings,
    parse_recent_trades, BookLevel, DepthBucket, HipClass, HyperliquidInstrument, HyperliquidSnapshot,
    L2Book, PredictedFunding, RecentTrade,
};

pub const INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
pub const FEE_SCHEDULE_VERSION: &str = "hyperliquid-docs-2026-09-10";
pub const BASE_PERP_TAKER: f64 = 0.00045;
pub const BASE_PERP_MAKER: f64 = 0.00015;

pub const DEPTH_BUCKETS_USD: [f64; 6] = [1_000.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0];
pub const AGE_TARGETS_SECONDS: [i64; 13] =
    [0, 1, 3, 10, 30, 60, 300, 900, 3_600, 21_600, 86_400, 259_200, 604_800];
