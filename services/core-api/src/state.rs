use pumphound_domain::{ChainEvent, MarketReducer, MarketSnapshot};
use pumphound_feature_core::{TradeTick, WindowAggregator};
use pumphound_market_state::InMemoryMarketReducer;
use pumphound_paper_core::{PortfolioRisk, RiskGates};
use pumphound_storage::Stores;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub stores: Stores,
    pub started: Instant,
    pub mode: String,
    pub source_label: String,
    pub markets: Arc<RwLock<InMemoryMarketReducer>>,
    pub windows: Arc<RwLock<HashMap<String, WindowAggregator>>>,
    pub radar_tx: broadcast::Sender<serde_json::Value>,
    pub stream_epoch: String,
    pub sequence: Arc<RwLock<u64>>,
    pub paper: Arc<RwLock<PaperState>>,
    pub watchlists: Arc<RwLock<Vec<Watchlist>>>,
    pub settings: Arc<RwLock<serde_json::Value>>,
    pub health: Arc<RwLock<HealthBits>>,
    pub tape: Arc<RwLock<HashMap<String, Vec<TapeTrade>>>>,
    /// Latest fee-oracle observation (priority fee percentiles + Jito tip floor). `None` until the
    /// poller has run; consumers fall back to a labelled static environment.
    pub fee_env: Arc<RwLock<Option<pumphound_execution_twin::network::FeeEnvironment>>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TapeTrade {
    pub event_id: String,
    pub signature: String,
    pub slot: String,
    pub side: String,
    pub trader: String,
    pub base_amount_atomic: String,
    pub quote_amount_atomic: String,
    pub quote_mint: String,
    pub instruction_name: String,
    pub observed_at: String,
    pub block_time: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct HealthBits {
    pub chain: String,
    pub last_slot: u64,
    pub last_event_at: Option<chrono::DateTime<chrono::Utc>>,
    pub lag_ms: i64,
    pub decoded: u64,
    pub quarantined: u64,
    pub last_market_snapshot_persisted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub market_snapshot_persist_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Watchlist {
    pub watchlist_id: Uuid,
    pub name: String,
    pub items: Vec<String>,
}

#[derive(Default)]
pub struct PaperState {
    pub portfolios: HashMap<Uuid, PaperPortfolio>,
    #[allow(dead_code)] // hydrated at startup so FIFO state is crash-visible even before an exit
    pub lots: Vec<PaperLot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperPortfolio {
    pub portfolio_id: Uuid,
    pub simulation_run_id: Uuid,
    pub name: String,
    pub mode: String,
    pub operating_mode: String,
    pub sol_atomic: i128,
    pub usdc_atomic: i128,
    pub reserved_sol: u128,
    pub positions: Vec<Position>,
    pub orders: Vec<serde_json::Value>,
    pub risk: PortfolioRisk,
    pub gates: RiskGates,
    pub policy_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub mint: String,
    pub market: String,
    pub quote_mint: String,
    pub base_atomic: u128,
    pub cost_quote_atomic: u128,
    pub realized_pnl_quote_atomic: i128,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperLot {
    pub lot_id: Uuid,
    pub portfolio_id: Uuid,
    pub simulation_run_id: Uuid,
    pub base_mint: String,
    pub quote_mint: String,
    pub remaining_base_atomic: u128,
    pub remaining_cost_atomic: u128,
}

impl AppState {
    pub fn new(stores: Stores, source_label: String) -> Self {
        let (radar_tx, _) = broadcast::channel(1024);
        Self {
            stores,
            started: Instant::now(),
            mode: std::env::var("PUMPHOUND_MODE").unwrap_or_else(|_| "LIVE_WATCH".into()),
            source_label,
            markets: Arc::new(RwLock::new(InMemoryMarketReducer::new())),
            windows: Arc::new(RwLock::new(HashMap::new())),
            radar_tx,
            stream_epoch: Uuid::now_v7().to_string(),
            sequence: Arc::new(RwLock::new(0)),
            paper: Arc::new(RwLock::new(PaperState::default())),
            watchlists: Arc::new(RwLock::new(vec![Watchlist {
                watchlist_id: Uuid::now_v7(),
                name: "Default".into(),
                items: vec![],
            }])),
            settings: Arc::new(RwLock::new(serde_json::json!({
                "revision": 1,
                "secrets_present": {
                    "helius": std::env::var("PUMPHOUND_HELIUS_API_KEY").ok().filter(|s| !s.is_empty()).is_some()
                }
            }))),
            health: Arc::new(RwLock::new(HealthBits {
                chain: "disconnected".into(),
                ..Default::default()
            })),
            tape: Arc::new(RwLock::new(HashMap::new())),
            fee_env: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn apply_event(&self, ev: &ChainEvent) -> Vec<MarketSnapshot> {
        let snapshots = {
            let mut markets = self.markets.write().await;
            markets
                .apply(ev)
                .unwrap_or_default()
                .into_iter()
                .map(|delta| delta.snapshot)
                .collect()
        };
        if let pumphound_domain::ChainEventPayload::TradeExecuted(t) = &ev.payload {
            let key = t.mint.to_string();
            {
                let mut w = self.windows.write().await;
                w.entry(key.clone()).or_default().push(TradeTick {
                    at: ev.block_time.unwrap_or(ev.observed_at).timestamp(),
                    side_buy: t.side == pumphound_domain::TradeSide::Buy,
                    base_atomic: t.base_amount_atomic.0,
                    quote_atomic: t.quote_amount_atomic.0,
                    trader: t.trader.to_string(),
                    price_ratio: if t.base_amount_atomic.0 == 0 {
                        None
                    } else {
                        Some(t.quote_amount_atomic.0 as f64 / t.base_amount_atomic.0 as f64)
                    },
                });
            }
            let row = TapeTrade {
                event_id: ev.event_id.clone(),
                signature: ev.signature.clone(),
                slot: ev.slot.to_string(),
                side: match t.side {
                    pumphound_domain::TradeSide::Buy => "buy".into(),
                    pumphound_domain::TradeSide::Sell => "sell".into(),
                },
                trader: t.trader.to_string(),
                base_amount_atomic: t.base_amount_atomic.to_string(),
                quote_amount_atomic: t.quote_amount_atomic.to_string(),
                quote_mint: t.quote_mint.to_string(),
                instruction_name: t.instruction_name.clone(),
                observed_at: ev.observed_at.to_rfc3339(),
                block_time: ev.block_time.map(|t| t.to_rfc3339()),
            };
            let mut tape = self.tape.write().await;
            let list = tape.entry(key).or_default();
            list.insert(0, row);
            list.truncate(2000);
        }
        let mut h = self.health.write().await;
        h.chain = "healthy".into();
        h.last_slot = ev.slot.0;
        h.last_event_at = Some(ev.observed_at);
        h.lag_ms = (pumphound_domain::now_utc() - ev.observed_at)
            .num_milliseconds()
            .max(0);
        h.decoded += 1;
        if matches!(
            ev.payload,
            pumphound_domain::ChainEventPayload::DecodeQuarantined(_)
        ) {
            h.quarantined += 1;
        }
        snapshots
    }

    pub async fn next_seq(&self) -> u64 {
        let mut s = self.sequence.write().await;
        *s += 1;
        *s
    }

    /// Reads the current position in the shared delta sequence without
    /// consuming a slot. Per-connection messages that aren't part of the
    /// ordered broadcast log (heartbeats, subscribe acks) report this instead
    /// of calling next_seq(), so they can't create a gap that only this one
    /// connection would ever "recover" from.
    pub async fn current_seq(&self) -> u64 {
        *self.sequence.read().await
    }
}

pub fn tape_event_price(quote_atomic: &str, base_atomic: &str) -> Option<f64> {
    let quote: f64 = quote_atomic.parse().ok()?;
    let base: f64 = base_atomic.parse().ok()?;
    if !quote.is_finite() || !base.is_finite() || quote <= 0.0 || base <= 0.0 {
        return None;
    }
    let price = quote / base;
    if !price.is_finite() || price <= 0.0 {
        return None;
    }
    Some(price)
}

pub fn tape_event_ts(trade: &TapeTrade) -> Option<i64> {
    let raw = trade
        .block_time
        .as_deref()
        .unwrap_or(trade.observed_at.as_str());
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

/// Newest tape rows first. Returns chronological 15s OHLC for the chart contract.
pub fn candles_from_tape(items: &[TapeTrade]) -> Vec<serde_json::Value> {
    const BUCKET_SECS: i64 = 15;
    const MAX_BUCKETS: usize = 300;
    let mut points: Vec<(i64, f64)> = Vec::new();
    for trade in items {
        let Some(price) = tape_event_price(&trade.quote_amount_atomic, &trade.base_amount_atomic)
        else {
            continue;
        };
        let Some(ts) = tape_event_ts(trade) else {
            continue;
        };
        points.push((ts, price));
    }
    points.sort_by_key(|(ts, _)| *ts);
    let mut buckets = std::collections::BTreeMap::<i64, (f64, f64, f64, f64)>::new();
    for (ts, price) in points {
        let bucket = ts - ts.rem_euclid(BUCKET_SECS);
        buckets
            .entry(bucket)
            .and_modify(|ohlc| {
                ohlc.1 = ohlc.1.max(price);
                ohlc.2 = ohlc.2.min(price);
                ohlc.3 = price;
            })
            .or_insert((price, price, price, price));
    }
    let all: Vec<_> = buckets.into_iter().collect();
    let start = all.len().saturating_sub(MAX_BUCKETS);
    all[start..]
        .iter()
        .map(|(time, (open, high, low, close))| {
            serde_json::json!({
                "time": time,
                "open": open,
                "high": high,
                "low": low,
                "close": close,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trade(ts: &str, quote: &str, base: &str) -> TapeTrade {
        TapeTrade {
            event_id: "e".into(),
            signature: "s".into(),
            slot: "1".into(),
            side: "buy".into(),
            trader: "w".into(),
            base_amount_atomic: base.into(),
            quote_amount_atomic: quote.into(),
            quote_mint: "So11111111111111111111111111111111111111112".into(),
            instruction_name: "buy".into(),
            observed_at: ts.into(),
            block_time: Some(ts.into()),
        }
    }

    #[test]
    fn candles_skip_invalid_and_emit_ohlc() {
        let items = vec![
            trade("2026-09-08T01:00:14Z", "2000000000", "1000000"),
            trade("2026-09-08T01:00:10Z", "1000000000", "1000000"),
            trade("2026-09-08T01:00:12Z", "0", "1000000"),
            trade("2026-09-08T01:00:40Z", "3000000000", "1000000"),
        ];
        let candles = candles_from_tape(&items);
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0]["time"], 1_788_829_200);
        assert_eq!(candles[0]["open"], 1000.0);
        assert_eq!(candles[0]["high"], 2000.0);
        assert_eq!(candles[0]["low"], 1000.0);
        assert_eq!(candles[0]["close"], 2000.0);
        assert!(candles[0]["open"].as_f64().unwrap().is_finite());
        assert_eq!(candles[1]["open"], 3000.0);
    }
}
