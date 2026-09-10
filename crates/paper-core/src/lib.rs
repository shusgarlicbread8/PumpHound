mod autonomous;
mod exec_select;
mod execution_edge;
mod fees;
mod learning;
mod policy;
mod quote;
mod research;
mod risk;
mod strategy_diff;
mod tail;

pub use autonomous::*;
pub use exec_select::*;
pub use execution_edge::*;
pub use learning::*;
pub use policy::*;
pub use research::*;
pub use strategy_diff::*;
pub use tail::*;

pub use fees::*;
pub use quote::*;
pub use risk::*;

use pumphound_domain::{
    now_utc, AcceptedPaperOrder, AssetAmount, Atomic, Canonicality, ChainEvent, MarketSnapshot,
    PaperError, PaperFill, PaperOrderRequest, PaperOrderResult, PaperOrderStatus, PaperQuote,
    PaperVenue, TradeSide,
};
use uuid::Uuid;

pub struct PumpPaperVenue {
    pub arrival_delay_ms: u64,
}

impl Default for PumpPaperVenue {
    fn default() -> Self {
        Self {
            arrival_delay_ms: 1000,
        }
    }
}

impl PaperVenue for PumpPaperVenue {
    fn quote(
        &self,
        order: &PaperOrderRequest,
        state: &MarketSnapshot,
    ) -> Result<PaperQuote, PaperError> {
        quote::quote_order(order, state)
    }

    fn execute(
        &mut self,
        accepted: AcceptedPaperOrder,
        _events: &[ChainEvent],
    ) -> PaperOrderResult {
        match quote::quote_order(&accepted.request, &placeholder_state(&accepted.quote)) {
            Ok(q) => {
                if q.expected_output.atomic.0 < accepted.quote.minimum_output.atomic.0 {
                    return PaperOrderResult {
                        order_id: accepted.order_id,
                        status: PaperOrderStatus::RejectedSlippage,
                        fill: None,
                        reason_code: Some("REJECTED_SLIPPAGE".into()),
                    };
                }
                PaperOrderResult {
                    order_id: accepted.order_id,
                    status: PaperOrderStatus::Filled,
                    fill: Some(PaperFill {
                        fill_id: Uuid::now_v7(),
                        order_id: accepted.order_id,
                        simulation_run_id: Uuid::now_v7(),
                        arrival_slot: accepted
                            .quote
                            .quality
                            .chain_lag_ms
                            .map(|_| pumphound_domain::Slot(0))
                            .unwrap_or(pumphound_domain::Slot(0)),
                        arrival_at: now_utc(),
                        input: accepted.request.input.clone(),
                        output: q.expected_output,
                        fee_components: q.fee_components,
                        market_snapshot_id: q.market_snapshot_id,
                        canonicality: Canonicality::Provisional,
                    }),
                    reason_code: None,
                }
            }
            Err(e) => PaperOrderResult {
                order_id: accepted.order_id,
                status: PaperOrderStatus::RejectedLiquidity,
                fill: None,
                reason_code: Some(e.to_string()),
            },
        }
    }
}

fn placeholder_state(q: &PaperQuote) -> MarketSnapshot {
    MarketSnapshot {
        snapshot_id: q.market_snapshot_id.clone(),
        market: q.input.mint,
        mint: q.expected_output.mint,
        quote_mint: q.input.mint,
        protocol: pumphound_domain::Protocol::PumpCurve,
        as_of: pumphound_domain::ObservationPoint {
            slot: pumphound_domain::Slot(0),
            tx_index: None,
            event_index: None,
            event_time: None,
            observed_at: now_utc(),
            knowledge_cutoff: now_utc(),
            commitment: pumphound_domain::Commitment::Processed,
        },
        canonicality: Canonicality::Provisional,
        virtual_base_atomic: Atomic(1_073_000_000_000_000),
        virtual_quote_atomic: Atomic(30_000_000_000),
        real_base_atomic: Atomic(800_000_000_000_000),
        real_quote_atomic: Atomic(0),
        effective_base_atomic: Atomic(1_073_000_000_000_000),
        effective_quote_atomic: Atomic(30_000_000_000),
        complete: false,
        migrated_market: None,
        config_snapshot_id: "default".into(),
        quality: q.quality.clone(),
        is_mayhem_mode: None,
        creator_fee_applies: None,
        base_supply_atomic: None,
        cashback_fee_bps: None,
    }
}

pub fn spendable(ledger_sum: i128, reserved: u128) -> Result<i128, PaperError> {
    let r = i128::try_from(reserved).map_err(|_| PaperError::Arithmetic("reserve".into()))?;
    Ok(ledger_sum - r)
}

#[allow(dead_code)]
fn _side(s: TradeSide) -> TradeSide {
    s
}

#[allow(dead_code)]
fn _aa(a: AssetAmount) -> AssetAmount {
    a
}
