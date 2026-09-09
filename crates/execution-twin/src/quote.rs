//! Exact executable quote from a `MarketSnapshot` (arrival or decision state).
//!
//! This is the single quoting path of the twin. It refuses to quote rather than guess: missing
//! protocol fields (e.g. PumpSwap `base_supply`, needed to select the fee tier) yield
//! `UNSUPPORTED_MARKET_STATE`, stale/incomplete data yields `DATA_STALE`.

use crate::contract::{FailureClass, ProtocolFees, TransactionKind};
use crate::fee_config::schedules;
use crate::pump_math::{
    self, curve_buy_cost_for_tokens, curve_buy_tokens_for_sol, curve_sell, mid_price_e9,
    pool_buy_exact_quote_in, pool_sell, CurveState, PoolState, ONE_BILLION_SUPPLY,
};
use pumphound_domain::{Coverage, MarketSnapshot, Protocol, QualityGrade, TradeSide};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteRejection {
    pub class: FailureClass,
    pub detail: String,
}

impl QuoteRejection {
    fn unsupported(detail: impl Into<String>) -> Self {
        QuoteRejection {
            class: FailureClass::UnsupportedMarketState,
            detail: detail.into(),
        }
    }
    fn stale(detail: impl Into<String>) -> Self {
        QuoteRejection {
            class: FailureClass::DataStale,
            detail: detail.into(),
        }
    }
}

impl From<pump_math::MathError> for QuoteRejection {
    fn from(e: pump_math::MathError) -> Self {
        QuoteRejection::unsupported(format!("protocol math: {e}"))
    }
}

/// Flags that change how a quote should be interpreted or risk-managed. Every `assumed_*`
/// flag marks a place where the snapshot did not carry the field and a conservative default
/// (higher cost) was applied; they are surfaced in evidence, never hidden.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketFlags {
    pub is_mayhem_mode: bool,
    pub boost_pool: bool,
    pub cashback_coin: bool,
    pub creator_fee_assumed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactQuote {
    pub venue: Protocol,
    pub side: TradeSide,
    pub transaction_kind: TransactionKind,
    /// Buy: fee-inclusive lamport budget. Sell: base atomic offered.
    pub input_atomic: u64,
    /// Quote that moves through the curve/pool before fees (`sol_amount` / `quote_amount_in|out`).
    pub swap_quote: u64,
    /// Buy: total quote the trader pays (swap + fees). Sell: net quote the trader receives.
    pub filled_quote: u64,
    pub filled_base: u64,
    pub protocol_fees: ProtocolFees,
    /// Instruction-level protection: min base out (buy) or min quote out (sell).
    pub minimum_output: u64,
    pub mid_price_before_e9: u128,
    pub execution_price_e9: u128,
    /// Execution price vs pre-trade mid, fees excluded (impact only).
    pub price_impact_bps: u32,
    pub flags: MarketFlags,
    pub market_snapshot_id: String,
}

fn check_freshness(s: &MarketSnapshot) -> Result<(), QuoteRejection> {
    if s.quality.grade == QualityGrade::Unavailable
        || s.quality.coverage != Coverage::CompleteForScope
    {
        return Err(QuoteRejection::stale(
            "market state is incomplete for executable quoting",
        ));
    }
    if s.effective_base_atomic.is_zero() || s.effective_quote_atomic.is_zero() {
        return Err(QuoteRejection::unsupported("unknown or zero reserves"));
    }
    Ok(())
}

fn curve_state(s: &MarketSnapshot, flags: &mut MarketFlags) -> Result<CurveState, QuoteRejection> {
    if s.complete {
        return Err(QuoteRejection::unsupported(
            "bonding curve complete; trade the PumpSwap market",
        ));
    }
    let creator_is_set = match s.creator_fee_applies {
        Some(v) => v,
        None => {
            flags.creator_fee_assumed = true;
            true
        }
    };
    let is_mayhem_mode = s.is_mayhem_mode.unwrap_or(false);
    flags.is_mayhem_mode = is_mayhem_mode;
    let cashback = s.cashback_fee_bps.unwrap_or(0);
    flags.cashback_coin = cashback > 0;
    Ok(CurveState {
        virtual_token_reserves: s.virtual_base_atomic.0,
        virtual_quote_reserves: s.virtual_quote_atomic.0,
        real_token_reserves: s.real_base_atomic.0,
        real_quote_reserves: s.real_quote_atomic.0,
        creator_is_set,
        is_mayhem_mode,
        // Mayhem coins tier by actual mint supply; PumpHound does not track it yet. The current
        // on-chain Pump FeeConfig has a single tier so the value cannot change the fee today,
        // but this stays visible via `flags.is_mayhem_mode`.
        mint_supply: ONE_BILLION_SUPPLY,
        cashback_fee_bps: cashback,
    })
}

fn pool_state(s: &MarketSnapshot, flags: &mut MarketFlags) -> Result<PoolState, QuoteRejection> {
    let base_mint_supply = s
        .base_supply_atomic
        .ok_or_else(|| {
            QuoteRejection::unsupported(
                "pumpswap base_supply unknown; fee tier cannot be determined from this snapshot",
            )
        })?
        .0;
    let coin_creator_is_set = match s.creator_fee_applies {
        Some(v) => v,
        None => {
            flags.creator_fee_assumed = true;
            true
        }
    };
    flags.boost_pool = !s.virtual_quote_atomic.is_zero();
    let cashback = s.cashback_fee_bps.unwrap_or(0);
    flags.cashback_coin = cashback > 0;
    Ok(PoolState {
        base_reserve: s.real_base_atomic.0,
        quote_reserve: s.real_quote_atomic.0,
        virtual_quote_reserves: s.virtual_quote_atomic.0,
        base_mint_supply,
        coin_creator_is_set,
        // PumpHound only tracks canonical (migration-created) PumpSwap pools, whose creator is
        // the Pump pool-authority PDA; tiered fees apply. Verified on 26/26 golden fixtures.
        is_pump_pool: true,
        cashback_fee_bps: cashback,
    })
}

fn min_out(amount: u64, slippage_bps: u32) -> u64 {
    let bps = slippage_bps.min(10_000) as u128;
    ((amount as u128) * (10_000 - bps) / 10_000) as u64
}

fn u64_of(v: u128, what: &str) -> Result<u64, QuoteRejection> {
    u64::try_from(v).map_err(|_| QuoteRejection::unsupported(format!("{what} exceeds u64")))
}

/// Quote `side` for `input_atomic` against `snapshot`. Buys interpret `input_atomic` as a
/// fee-inclusive quote budget; sells as base atomic to dispose of.
pub fn exact_quote(
    snapshot: &MarketSnapshot,
    side: TradeSide,
    input_atomic: u64,
    slippage_bps: u32,
) -> Result<ExactQuote, QuoteRejection> {
    check_freshness(snapshot)?;
    if input_atomic == 0 {
        return Err(QuoteRejection::unsupported("zero input"));
    }
    let sched = schedules();
    let mut flags = MarketFlags::default();
    let mid_before = mid_price_e9(
        snapshot.effective_quote_atomic.0,
        snapshot.effective_base_atomic.0,
    )
    .ok_or_else(|| QuoteRejection::unsupported("mid price undefined"))?;

    let (kind, swap_quote, filled_quote, filled_base, fees) = match (snapshot.protocol, side) {
        (Protocol::PumpCurve, TradeSide::Buy) => {
            let curve = curve_state(snapshot, &mut flags)?;
            let tokens = curve_buy_tokens_for_sol(&curve, &sched.curve, input_atomic as u128)?;
            if tokens == 0 {
                return Err(QuoteRejection::unsupported("budget buys zero tokens"));
            }
            let cost = curve_buy_cost_for_tokens(&curve, &sched.curve, tokens)?;
            if cost.total_sol > input_atomic as u128 {
                return Err(QuoteRejection::unsupported(
                    "exact cost exceeds budget (protocol rounding)",
                ));
            }
            (
                TransactionKind::PumpBuyExactSolIn,
                u64_of(cost.sol_cost, "sol_cost")?,
                u64_of(cost.total_sol, "total_sol")?,
                u64_of(cost.token_amount, "tokens")?,
                cost.fees,
            )
        }
        (Protocol::PumpCurve, TradeSide::Sell) => {
            let curve = curve_state(snapshot, &mut flags)?;
            let sell = curve_sell(&curve, &sched.curve, input_atomic as u128)?;
            if sell.sol_out_gross > curve.real_quote_reserves {
                return Err(QuoteRejection::unsupported(
                    "sell output exceeds real SOL reserves",
                ));
            }
            (
                TransactionKind::PumpSell,
                u64_of(sell.sol_out_gross, "gross")?,
                u64_of(sell.sol_out_net, "net")?,
                input_atomic,
                sell.fees,
            )
        }
        (Protocol::PumpSwap, TradeSide::Buy) => {
            let pool = pool_state(snapshot, &mut flags)?;
            let buy = pool_buy_exact_quote_in(&pool, &sched.pool, input_atomic as u128)?;
            if buy.base_amount_out == 0 {
                return Err(QuoteRejection::unsupported("budget buys zero base"));
            }
            (
                TransactionKind::PumpSwapBuyExactQuoteIn,
                u64_of(buy.quote_amount_in, "quote_in")?,
                u64_of(buy.total_quote, "total_quote")?,
                u64_of(buy.base_amount_out, "base_out")?,
                buy.fees,
            )
        }
        (Protocol::PumpSwap, TradeSide::Sell) => {
            let pool = pool_state(snapshot, &mut flags)?;
            let sell = pool_sell(&pool, &sched.pool, input_atomic as u128)?;
            (
                TransactionKind::PumpSwapSell,
                u64_of(sell.quote_out_gross, "gross")?,
                u64_of(sell.quote_out_net, "net")?,
                input_atomic,
                sell.fees,
            )
        }
    };
    if filled_quote == 0 || filled_base == 0 {
        return Err(QuoteRejection::unsupported("zero-sized fill"));
    }
    let execution_price = pump_math::execution_price_e9(swap_quote as u128, filled_base as u128)
        .ok_or_else(|| QuoteRejection::unsupported("execution price undefined"))?;
    // Impact is always a cost: buys execute above mid, sells below.
    let impact = pump_math::relative_bps(mid_before, execution_price);
    let price_impact_bps = match side {
        TradeSide::Buy => impact.max(0) as u32,
        TradeSide::Sell => (-impact).max(0) as u32,
    };
    let minimum_output = match side {
        TradeSide::Buy => min_out(filled_base, slippage_bps),
        TradeSide::Sell => min_out(filled_quote, slippage_bps),
    };
    Ok(ExactQuote {
        venue: snapshot.protocol,
        side,
        transaction_kind: kind,
        input_atomic,
        swap_quote,
        filled_quote,
        filled_base,
        protocol_fees: fees,
        minimum_output,
        mid_price_before_e9: mid_before,
        execution_price_e9: execution_price,
        price_impact_bps,
        flags,
        market_snapshot_id: snapshot.snapshot_id.clone(),
    })
}
