use pumphound_domain::{
    now_utc, Atomic, Canonicality, ChainEvent, ChainEventPayload, Commitment, DataQuality,
    MarketDelta, MarketReducer, MarketSnapshot, ObservationPoint, Protocol, Pubkey, StateError,
};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Market {
    snapshot: MarketSnapshot,
    buyers: std::collections::HashSet<String>,
    volume_quote: u128,
    last_signature: String,
}

#[derive(Default)]
pub struct InMemoryMarketReducer {
    by_market: HashMap<String, Market>,
    mint_to_market: HashMap<String, String>,
}

impl InMemoryMarketReducer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, market_or_mint: &str) -> Option<&MarketSnapshot> {
        self.by_market
            .get(market_or_mint)
            .or_else(|| {
                self.mint_to_market
                    .get(market_or_mint)
                    .and_then(|m| self.by_market.get(m))
            })
            .map(|m| &m.snapshot)
    }

    pub fn all(&self) -> impl Iterator<Item = &MarketSnapshot> {
        self.by_market.values().map(|m| &m.snapshot)
    }

    pub fn buyer_count(&self, market_or_mint: &str) -> usize {
        self.by_market
            .get(market_or_mint)
            .or_else(|| {
                self.mint_to_market
                    .get(market_or_mint)
                    .and_then(|m| self.by_market.get(m))
            })
            .map(|m| m.buyers.len())
            .unwrap_or(0)
    }

    pub fn volume_quote(&self, market_or_mint: &str) -> u128 {
        self.by_market
            .get(market_or_mint)
            .or_else(|| {
                self.mint_to_market
                    .get(market_or_mint)
                    .and_then(|m| self.by_market.get(m))
            })
            .map(|m| m.volume_quote)
            .unwrap_or(0)
    }
}

impl MarketReducer for InMemoryMarketReducer {
    fn apply(&mut self, event: &ChainEvent) -> Result<Vec<MarketDelta>, StateError> {
        match &event.payload {
            ChainEventPayload::LaunchCreated(c) => {
                let virtual_base = c.virtual_base_atomic.unwrap_or(Atomic(0));
                let virtual_quote = c.virtual_quote_atomic.unwrap_or(Atomic(0));
                let real_base = c.real_base_atomic.unwrap_or(Atomic(0));
                let reserves_complete = c.virtual_base_atomic.is_some()
                    && c.virtual_quote_atomic.is_some()
                    && c.real_base_atomic.is_some();
                let snap = MarketSnapshot {
                    snapshot_id: uuid::Uuid::now_v7().to_string(),
                    market: c.market,
                    mint: c.mint,
                    quote_mint: c.quote_mint,
                    protocol: event.protocol,
                    as_of: as_of(event),
                    canonicality: event.canonicality,
                    virtual_base_atomic: virtual_base,
                    virtual_quote_atomic: virtual_quote,
                    real_base_atomic: real_base,
                    real_quote_atomic: Atomic(0),
                    effective_base_atomic: virtual_base,
                    effective_quote_atomic: virtual_quote,
                    complete: false,
                    migrated_market: None,
                    config_snapshot_id: "genesis".into(),
                    quality: if reserves_complete {
                        live_quality(event, event.tx_index.is_some())
                    } else {
                        incomplete_reserve_quality(&[
                            "virtual_base_atomic",
                            "virtual_quote_atomic",
                            "real_base_atomic",
                        ])
                    },
                };
                self.mint_to_market
                    .insert(c.mint.to_string(), c.market.to_string());
                self.by_market.insert(
                    c.market.to_string(),
                    Market {
                        snapshot: snap.clone(),
                        buyers: Default::default(),
                        volume_quote: 0,
                        last_signature: event.signature.clone(),
                    },
                );
                Ok(vec![MarketDelta {
                    market: c.market,
                    snapshot: snap,
                }])
            }
            ChainEventPayload::TradeExecuted(t) => {
                let mk = t.market.to_string();
                if !self.by_market.contains_key(&mk)
                    && !self.mint_to_market.contains_key(&t.mint.to_string())
                {
                    let vb = t.virtual_base_atomic.unwrap_or(Atomic(0));
                    let vq = t.virtual_quote_atomic.unwrap_or(Atomic(0));
                    let rb = t.real_base_atomic.unwrap_or(Atomic(0));
                    let rq = t.real_quote_atomic.unwrap_or(Atomic(0));
                    let snap = MarketSnapshot {
                        snapshot_id: uuid::Uuid::now_v7().to_string(),
                        market: t.market,
                        mint: t.mint,
                        quote_mint: t.quote_mint,
                        protocol: event.protocol,
                        as_of: as_of(event),
                        canonicality: event.canonicality,
                        virtual_base_atomic: vb,
                        virtual_quote_atomic: vq,
                        real_base_atomic: rb,
                        real_quote_atomic: rq,
                        effective_base_atomic: match event.protocol {
                            Protocol::PumpSwap => rb,
                            Protocol::PumpCurve => vb,
                        },
                        effective_quote_atomic: match event.protocol {
                            Protocol::PumpSwap => rq.checked_add(vq).unwrap_or(rq),
                            Protocol::PumpCurve => vq,
                        },
                        complete: false,
                        migrated_market: None,
                        config_snapshot_id: "observed_from_trade".into(),
                        quality: trade_quality(t, event, event.tx_index.is_some()),
                    };
                    self.mint_to_market.insert(t.mint.to_string(), mk.clone());
                    self.by_market.insert(
                        mk,
                        Market {
                            snapshot: snap,
                            buyers: Default::default(),
                            volume_quote: 0,
                            last_signature: String::new(),
                        },
                    );
                }
                let key = resolve_key(self, &t.market, &t.mint);
                let Some(m) = self.by_market.get_mut(&key) else {
                    return Ok(vec![]);
                };
                if event.signature == m.last_signature
                    && event.event_index == 0
                    && m.snapshot.as_of.slot == event.slot
                {
                    return Ok(vec![]);
                }
                if let Some(v) = t.virtual_base_atomic {
                    m.snapshot.virtual_base_atomic = v;
                }
                if let Some(v) = t.virtual_quote_atomic {
                    m.snapshot.virtual_quote_atomic = v;
                }
                if let Some(v) = t.real_base_atomic {
                    m.snapshot.real_base_atomic = v;
                }
                if let Some(v) = t.real_quote_atomic {
                    m.snapshot.real_quote_atomic = v;
                }
                m.snapshot.quality = trade_quality(t, event, event.tx_index.is_some());
                m.snapshot.effective_base_atomic = match m.snapshot.protocol {
                    Protocol::PumpSwap => m.snapshot.real_base_atomic,
                    Protocol::PumpCurve => m.snapshot.virtual_base_atomic,
                };
                m.snapshot.effective_quote_atomic = match m.snapshot.protocol {
                    Protocol::PumpSwap => m
                        .snapshot
                        .real_quote_atomic
                        .checked_add(m.snapshot.virtual_quote_atomic)
                        .unwrap_or(m.snapshot.real_quote_atomic),
                    Protocol::PumpCurve => m.snapshot.virtual_quote_atomic,
                };
                m.snapshot.as_of = as_of(event);
                m.snapshot.snapshot_id = uuid::Uuid::now_v7().to_string();
                if t.side == pumphound_domain::TradeSide::Buy {
                    m.buyers.insert(t.trader.to_string());
                }
                m.volume_quote = m.volume_quote.saturating_add(t.quote_amount_atomic.0);
                m.last_signature = event.signature.clone();
                Ok(vec![MarketDelta {
                    market: m.snapshot.market,
                    snapshot: m.snapshot.clone(),
                }])
            }
            ChainEventPayload::LaunchCompleted(c) => {
                if let Some(m) = self.by_market.get_mut(&c.market.to_string()) {
                    m.snapshot.complete = true;
                    m.snapshot.as_of = as_of(event);
                    m.snapshot.snapshot_id = uuid::Uuid::now_v7().to_string();
                    return Ok(vec![MarketDelta {
                        market: c.market,
                        snapshot: m.snapshot.clone(),
                    }]);
                }
                Ok(vec![])
            }
            ChainEventPayload::LaunchMigrated(c) => {
                if let Some(m) = self.by_market.get_mut(&c.old_market.to_string()) {
                    m.snapshot.complete = true;
                    m.snapshot.migrated_market = Some(c.new_market);
                    m.snapshot.as_of = as_of(event);
                    m.snapshot.snapshot_id = uuid::Uuid::now_v7().to_string();
                    let old = m.snapshot.clone();
                    self.mint_to_market
                        .insert(c.mint.to_string(), c.new_market.to_string());
                    return Ok(vec![MarketDelta {
                        market: c.old_market,
                        snapshot: old,
                    }]);
                }
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }

    fn snapshot(&self, _as_of: ObservationPoint) -> Option<MarketSnapshot> {
        self.by_market.values().next().map(|m| m.snapshot.clone())
    }
}

fn trade_supports_executable_quote(trade: &pumphound_domain::TradeExecuted) -> bool {
    trade
        .raw_event
        .get("executable_quote_supported")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

fn trade_quality(
    trade: &pumphound_domain::TradeExecuted,
    event: &ChainEvent,
    exact_order: bool,
) -> DataQuality {
    if trade_supports_executable_quote(trade) {
        live_quality(event, exact_order)
    } else {
        let mut quality = DataQuality::unknown("historical event lacks executable quote inputs");
        quality.coverage = pumphound_domain::Coverage::Partial;
        quality.missing_fields = trade
            .raw_event
            .get("missing_fields")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string)
            .collect();
        quality
    }
}

fn live_quality(event: &ChainEvent, exact_order: bool) -> DataQuality {
    let chain_lag_ms = event
        .block_time
        .map(|block_time| (event.observed_at - block_time).num_milliseconds().max(0))
        .unwrap_or(0);
    let market_lag_ms = (now_utc() - event.observed_at).num_milliseconds().max(0);
    DataQuality::live(chain_lag_ms, market_lag_ms, exact_order)
}

fn incomplete_reserve_quality(missing_fields: &[&str]) -> DataQuality {
    let mut quality = DataQuality::unknown("historical create event lacks initial reserves");
    quality.coverage = pumphound_domain::Coverage::Partial;
    quality.missing_fields = missing_fields.iter().map(|field| (*field).into()).collect();
    quality
}

fn resolve_key(state: &InMemoryMarketReducer, market: &Pubkey, mint: &Pubkey) -> String {
    let mk = market.to_string();
    if state.by_market.contains_key(&mk) {
        return mk;
    }
    state
        .mint_to_market
        .get(&mint.to_string())
        .cloned()
        .unwrap_or(mk)
}

fn as_of(event: &ChainEvent) -> ObservationPoint {
    ObservationPoint {
        slot: event.slot,
        tx_index: event.tx_index,
        event_index: Some(event.event_index),
        event_time: event.block_time.or(Some(event.observed_at)),
        observed_at: event.observed_at,
        knowledge_cutoff: now_utc(),
        commitment: Commitment::Processed,
    }
}

pub fn curve_progress(snap: &MarketSnapshot) -> Option<f64> {
    if snap.complete {
        return Some(1.0);
    }
    if snap.virtual_base_atomic.is_zero() {
        return None;
    }
    let initial = snap.virtual_base_atomic.0;
    let remaining = snap.real_base_atomic.0.min(initial);
    Some(((initial - remaining) as f64 / initial as f64).clamp(0.0, 1.0))
}

pub fn executable_price_quote(snap: &MarketSnapshot) -> Option<String> {
    if snap.effective_base_atomic.is_zero() || snap.effective_quote_atomic.is_zero() {
        return None;
    }
    // Display-only ratio; not an executable fill price.
    let q = snap.effective_quote_atomic.0 as f64;
    let b = snap.effective_base_atomic.0 as f64;
    Some(format!("{:.12}", q / b))
}

#[allow(dead_code)]
fn _canon(c: Canonicality) -> Canonicality {
    c
}
