//! Exact Pump bonding-curve and PumpSwap AMM math, transcribed from the official SDKs:
//!   @pump-fun/pump-sdk@1.36.0      src/bondingCurve.ts, src/fees.ts
//!   @pump-fun/pump-swap-sdk@1.19.0 src/sdk/buy.ts, src/sdk/sell.ts, src/sdk/fees.ts, util.ts
//! and pump-public-docs@9c82f61 docs/FEE_PROGRAM_README.md.
//!
//! Every function is integer-only. Rounding direction follows the SDK line by line:
//! fees are `ceil_div(amount * bps, 10_000)`; curve outputs are floor divisions.

use crate::contract::ProtocolFees;
use serde::{Deserialize, Serialize};

/// 1e9 tokens with 6 decimals. The SDK uses this constant for non-mayhem market-cap tiering.
pub const ONE_BILLION_SUPPLY: u128 = 1_000_000_000_000_000;
pub const BPS_DENOMINATOR: u128 = 10_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MathError {
    #[error("zero reserves")]
    ZeroReserves,
    #[error("curve migrated (virtual token reserves are zero)")]
    Migrated,
    #[error("pool would be depleted")]
    Depleted,
    #[error("insufficient real quote reserves to cover sell output")]
    InsufficientRealQuote,
    #[error("fees exceed output")]
    FeesExceedOutput,
    #[error("arithmetic overflow in {0}")]
    Overflow(&'static str),
    #[error("amount is zero")]
    ZeroAmount,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fees {
    pub lp_fee_bps: u64,
    pub protocol_fee_bps: u64,
    pub creator_fee_bps: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeTier {
    pub market_cap_lamports_threshold: u128,
    pub fees: Fees,
}

/// pump-fees-math::calculate_fee_tier(), exactly as the TS SDK mirrors it.
pub fn calculate_fee_tier(tiers: &[FeeTier], market_cap: u128) -> Fees {
    let first = match tiers.first() {
        Some(t) => t,
        None => return Fees::default(),
    };
    if market_cap < first.market_cap_lamports_threshold {
        return first.fees;
    }
    for tier in tiers.iter().rev() {
        if market_cap >= tier.market_cap_lamports_threshold {
            return tier.fees;
        }
    }
    first.fees
}

/// Returns the 0-based tier index selected for `market_cap` (for evidence/labels).
pub fn fee_tier_index(tiers: &[FeeTier], market_cap: u128) -> Option<usize> {
    let first = tiers.first()?;
    if market_cap < first.market_cap_lamports_threshold {
        return Some(0);
    }
    for (i, tier) in tiers.iter().enumerate().rev() {
        if market_cap >= tier.market_cap_lamports_threshold {
            return Some(i);
        }
    }
    Some(0)
}

pub fn ceil_div(a: u128, b: u128) -> Result<u128, MathError> {
    if b == 0 {
        return Err(MathError::ZeroReserves);
    }
    a.checked_add(b - 1)
        .map(|v| v / b)
        .ok_or(MathError::Overflow("ceil_div"))
}

pub fn fee(amount: u128, bps: u64) -> Result<u128, MathError> {
    ceil_div(
        amount
            .checked_mul(bps as u128)
            .ok_or(MathError::Overflow("fee"))?,
        BPS_DENOMINATOR,
    )
}

pub fn bonding_curve_market_cap(
    mint_supply: u128,
    virtual_quote: u128,
    virtual_token: u128,
) -> Result<u128, MathError> {
    if virtual_token == 0 {
        return Err(MathError::ZeroReserves);
    }
    Ok(virtual_quote
        .checked_mul(mint_supply)
        .ok_or(MathError::Overflow("market_cap"))?
        / virtual_token)
}

pub fn pool_market_cap(
    base_mint_supply: u128,
    base_reserve: u128,
    quote_reserve: u128,
) -> Result<u128, MathError> {
    if base_reserve == 0 {
        return Err(MathError::ZeroReserves);
    }
    Ok(quote_reserve
        .checked_mul(base_mint_supply)
        .ok_or(MathError::Overflow("pool_market_cap"))?
        / base_reserve)
}

/// Bonding-curve state as the program sees it at one slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveState {
    pub virtual_token_reserves: u128,
    pub virtual_quote_reserves: u128,
    pub real_token_reserves: u128,
    pub real_quote_reserves: u128,
    /// `bonding_curve.creator != Pubkey::default()` — creator fee only applies when true.
    pub creator_is_set: bool,
    pub is_mayhem_mode: bool,
    /// Actual mint supply (only used for mayhem coins; otherwise ONE_BILLION_SUPPLY).
    pub mint_supply: u128,
    /// Cashback coins: the program charges this instead of the creator fee (creator bps -> 0).
    /// Observed on mainnet TradeEvents: creator_fee_basis_points=0, cashback_fee_basis_points=30.
    #[serde(default)]
    pub cashback_fee_bps: u64,
}

impl CurveState {
    fn tier_supply(&self) -> u128 {
        if self.is_mayhem_mode {
            self.mint_supply
        } else {
            ONE_BILLION_SUPPLY
        }
    }
}

/// Fee schedule the Pump program applies to a curve: either tiered `FeeConfig` or global flat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveFeeSchedule {
    Tiered {
        tiers: Vec<FeeTier>,
    },
    Flat {
        protocol_fee_bps: u64,
        creator_fee_bps: u64,
    },
}

impl CurveFeeSchedule {
    pub fn fees_bps(&self, curve: &CurveState) -> Result<(Fees, String), MathError> {
        match self {
            CurveFeeSchedule::Tiered { tiers } => {
                let market_cap = bonding_curve_market_cap(
                    curve.tier_supply(),
                    curve.virtual_quote_reserves,
                    curve.virtual_token_reserves,
                )?;
                let idx = fee_tier_index(tiers, market_cap).unwrap_or(0);
                Ok((calculate_fee_tier(tiers, market_cap), format!("tier{idx}")))
            }
            CurveFeeSchedule::Flat {
                protocol_fee_bps,
                creator_fee_bps,
            } => Ok((
                Fees {
                    lp_fee_bps: 0,
                    protocol_fee_bps: *protocol_fee_bps,
                    creator_fee_bps: *creator_fee_bps,
                },
                "global_flat".into(),
            )),
        }
    }
}

/// Effective per-trade fee schedule after applying creator-set and cashback substitution rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EffectiveBps {
    lp: u64,
    protocol: u64,
    creator: u64,
    cashback: u64,
}

impl EffectiveBps {
    fn resolve(fees: Fees, creator_is_set: bool, cashback_fee_bps: u64) -> Self {
        let (creator, cashback) = if cashback_fee_bps > 0 {
            (0, cashback_fee_bps)
        } else if creator_is_set {
            (fees.creator_fee_bps, 0)
        } else {
            (0, 0)
        };
        EffectiveBps {
            lp: fees.lp_fee_bps,
            protocol: fees.protocol_fee_bps,
            creator,
            cashback,
        }
    }
    fn total(&self) -> u128 {
        self.lp as u128 + self.protocol as u128 + self.creator as u128 + self.cashback as u128
    }
    fn charge(&self, amount: u128, tier: String) -> Result<ProtocolFees, MathError> {
        Ok(ProtocolFees {
            protocol_fee_quote: as_u64(fee(amount, self.protocol)?)?,
            creator_fee_quote: as_u64(fee(amount, self.creator)?)?,
            lp_fee_quote: as_u64(fee(amount, self.lp)?)?,
            protocol_fee_bps: self.protocol,
            creator_fee_bps: self.creator,
            lp_fee_bps: self.lp,
            cashback_fee_quote: as_u64(fee(amount, self.cashback)?)?,
            cashback_fee_bps: self.cashback,
            fee_tier: tier,
        })
    }
}

fn curve_bps(
    curve: &CurveState,
    schedule: &CurveFeeSchedule,
) -> Result<(EffectiveBps, String), MathError> {
    let (fees, tier) = schedule.fees_bps(curve)?;
    Ok((
        EffectiveBps::resolve(fees, curve.creator_is_set, curve.cashback_fee_bps),
        tier,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveBuy {
    /// Tokens the `buy` instruction should request.
    pub token_amount: u128,
    /// SOL cost before fees (`sol_amount` in the on-chain TradeEvent).
    pub sol_cost: u128,
    pub fees: ProtocolFees,
    /// sol_cost + fees. Must be <= spendable input.
    pub total_sol: u128,
}

/// pump-sdk getBuyTokenAmountFromSolAmount: how many tokens does `spendable_sol` buy, net of fees.
pub fn curve_buy_tokens_for_sol(
    curve: &CurveState,
    schedule: &CurveFeeSchedule,
    spendable_sol: u128,
) -> Result<u128, MathError> {
    if spendable_sol == 0 {
        return Err(MathError::ZeroAmount);
    }
    if curve.virtual_token_reserves == 0 {
        return Err(MathError::Migrated);
    }
    let (bps, _) = curve_bps(curve, schedule)?;
    let input_amount = spendable_sol
        .saturating_sub(1)
        .checked_mul(BPS_DENOMINATOR)
        .ok_or(MathError::Overflow("input_amount"))?
        / (bps.total() + BPS_DENOMINATOR);
    let tokens = input_amount
        .checked_mul(curve.virtual_token_reserves)
        .ok_or(MathError::Overflow("tokens"))?
        / (curve
            .virtual_quote_reserves
            .checked_add(input_amount)
            .ok_or(MathError::Overflow("denominator"))?);
    Ok(tokens.min(curve.real_token_reserves))
}

/// pump-sdk getBuySolAmountFromTokenAmount: exact SOL the program charges for `token_amount`.
pub fn curve_buy_cost_for_tokens(
    curve: &CurveState,
    schedule: &CurveFeeSchedule,
    token_amount: u128,
) -> Result<CurveBuy, MathError> {
    if token_amount == 0 {
        return Err(MathError::ZeroAmount);
    }
    if curve.virtual_token_reserves == 0 {
        return Err(MathError::Migrated);
    }
    let min_amount = token_amount.min(curve.real_token_reserves);
    let denominator = curve
        .virtual_token_reserves
        .checked_sub(min_amount)
        .ok_or(MathError::Depleted)?;
    if denominator == 0 {
        return Err(MathError::Depleted);
    }
    let sol_cost = min_amount
        .checked_mul(curve.virtual_quote_reserves)
        .ok_or(MathError::Overflow("sol_cost"))?
        / denominator
        + 1;
    let (bps, tier) = curve_bps(curve, schedule)?;
    let fees = bps.charge(sol_cost, tier)?;
    let total = sol_cost
        .checked_add(fees.total_quote() as u128)
        .ok_or(MathError::Overflow("total"))?;
    Ok(CurveBuy {
        token_amount: min_amount,
        sol_cost,
        fees,
        total_sol: total,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveSell {
    /// Gross SOL out before fees (`sol_amount` in the on-chain TradeEvent).
    pub sol_out_gross: u128,
    pub fees: ProtocolFees,
    /// What the seller receives.
    pub sol_out_net: u128,
}

/// pump-sdk getSellSolAmountFromTokenAmount.
pub fn curve_sell(
    curve: &CurveState,
    schedule: &CurveFeeSchedule,
    token_amount: u128,
) -> Result<CurveSell, MathError> {
    if token_amount == 0 {
        return Err(MathError::ZeroAmount);
    }
    if curve.virtual_token_reserves == 0 {
        return Err(MathError::Migrated);
    }
    let gross = token_amount
        .checked_mul(curve.virtual_quote_reserves)
        .ok_or(MathError::Overflow("gross"))?
        / (curve
            .virtual_token_reserves
            .checked_add(token_amount)
            .ok_or(MathError::Overflow("denominator"))?);
    let (bps, tier) = curve_bps(curve, schedule)?;
    let fees = bps.charge(gross, tier)?;
    let net = gross
        .checked_sub(fees.total_quote() as u128)
        .ok_or(MathError::FeesExceedOutput)?;
    Ok(CurveSell {
        sol_out_gross: gross,
        fees,
        sol_out_net: net,
    })
}

// ---------------------------------------------------------------------------------------------
// PumpSwap (pump_amm) canonical pool
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolState {
    pub base_reserve: u128,
    pub quote_reserve: u128,
    /// Boost pools carry a virtual quote component; 0 for legacy pools.
    pub virtual_quote_reserves: u128,
    pub base_mint_supply: u128,
    /// `pool.coin_creator != Pubkey::default()`.
    pub coin_creator_is_set: bool,
    /// `pool.creator == pump_pool_authority_pda(base_mint)` — tiered fees apply only then.
    pub is_pump_pool: bool,
    #[serde(default)]
    pub cashback_fee_bps: u64,
}

impl PoolState {
    fn effective_quote(&self) -> Result<u128, MathError> {
        self.quote_reserve
            .checked_add(self.virtual_quote_reserves)
            .ok_or(MathError::Overflow("effective_quote"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolFeeSchedule {
    pub tiers: Vec<FeeTier>,
    pub flat_fees: Fees,
}

impl PoolFeeSchedule {
    pub fn fees_bps(&self, pool: &PoolState) -> Result<(Fees, String), MathError> {
        if !pool.is_pump_pool {
            return Ok((self.flat_fees, "flat_non_pump_pool".into()));
        }
        let market_cap = pool_market_cap(
            pool.base_mint_supply,
            pool.base_reserve,
            pool.effective_quote()?,
        )?;
        let idx = fee_tier_index(&self.tiers, market_cap).unwrap_or(0);
        Ok((
            calculate_fee_tier(&self.tiers, market_cap),
            format!("tier{idx}"),
        ))
    }
}

fn pool_bps(
    pool: &PoolState,
    schedule: &PoolFeeSchedule,
) -> Result<(EffectiveBps, String), MathError> {
    let (fees, tier) = schedule.fees_bps(pool)?;
    Ok((
        EffectiveBps::resolve(fees, pool.coin_creator_is_set, pool.cashback_fee_bps),
        tier,
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolBuy {
    pub base_amount_out: u128,
    /// Quote paid into the pool before fees (`quote_amount_in` of a `buy` BuyEvent, or
    /// `user_quote_amount_in` of a `buy_exact_quote_in` BuyEvent).
    pub quote_amount_in: u128,
    pub fees: ProtocolFees,
    /// quote_amount_in + fees: total the trader pays.
    pub total_quote: u128,
}

/// pump-swap-sdk buyQuoteInput / on-chain `buy_exact_quote_in`: exact decomposition for a
/// fee-inclusive quote budget. `total_quote` <= `quote` always.
pub fn pool_buy_exact_quote_in(
    pool: &PoolState,
    schedule: &PoolFeeSchedule,
    quote: u128,
) -> Result<PoolBuy, MathError> {
    if pool.base_reserve == 0 || pool.quote_reserve == 0 {
        return Err(MathError::ZeroReserves);
    }
    if quote == 0 {
        return Err(MathError::ZeroAmount);
    }
    let effective_quote_reserve = pool.effective_quote()?;
    let (bps, tier) = pool_bps(pool, schedule)?;
    let denominator = BPS_DENOMINATOR + bps.total();
    let mut effective_quote = quote
        .checked_mul(BPS_DENOMINATOR)
        .ok_or(MathError::Overflow("effective_quote"))?
        / denominator;
    let mut fees = bps.charge(effective_quote, tier.clone())?;
    let total_with_fees = effective_quote + fees.total_quote() as u128;
    if total_with_fees > quote {
        effective_quote = effective_quote.saturating_sub(total_with_fees - quote);
        fees = bps.charge(effective_quote, tier)?;
    }
    let input_amount = effective_quote.saturating_sub(1);
    let denominator_effective = effective_quote_reserve
        .checked_add(input_amount)
        .ok_or(MathError::Overflow("denominator_effective"))?;
    if denominator_effective == 0 {
        return Err(MathError::Depleted);
    }
    let base = pool
        .base_reserve
        .checked_mul(input_amount)
        .ok_or(MathError::Overflow("numerator"))?
        / denominator_effective;
    let total = effective_quote + fees.total_quote() as u128;
    Ok(PoolBuy {
        base_amount_out: base,
        quote_amount_in: effective_quote,
        fees,
        total_quote: total,
    })
}

/// pump-swap-sdk buyQuoteInput: base received for an exact quote budget (fees inclusive).
pub fn pool_buy_base_for_quote(
    pool: &PoolState,
    schedule: &PoolFeeSchedule,
    quote: u128,
) -> Result<u128, MathError> {
    Ok(pool_buy_exact_quote_in(pool, schedule, quote)?.base_amount_out)
}

/// pump-swap-sdk buyBaseInput / on-chain `buy`: exact quote the program charges for `base`.
pub fn pool_buy_cost_for_base(
    pool: &PoolState,
    schedule: &PoolFeeSchedule,
    base: u128,
) -> Result<PoolBuy, MathError> {
    if pool.base_reserve == 0 || pool.quote_reserve == 0 {
        return Err(MathError::ZeroReserves);
    }
    if base == 0 {
        return Err(MathError::ZeroAmount);
    }
    if base > pool.base_reserve {
        return Err(MathError::Depleted);
    }
    let effective_quote_reserve = pool.effective_quote()?;
    let numerator = effective_quote_reserve
        .checked_mul(base)
        .ok_or(MathError::Overflow("numerator"))?;
    let denominator = pool.base_reserve - base;
    if denominator == 0 {
        return Err(MathError::Depleted);
    }
    let quote_amount_in = ceil_div(numerator, denominator)?;
    let (bps, tier) = pool_bps(pool, schedule)?;
    let fees = bps.charge(quote_amount_in, tier)?;
    let total = quote_amount_in + fees.total_quote() as u128;
    Ok(PoolBuy {
        base_amount_out: base,
        quote_amount_in,
        fees,
        total_quote: total,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolSell {
    pub quote_out_gross: u128,
    pub fees: ProtocolFees,
    pub quote_out_net: u128,
}

/// pump-swap-sdk sellBaseInput.
pub fn pool_sell(
    pool: &PoolState,
    schedule: &PoolFeeSchedule,
    base: u128,
) -> Result<PoolSell, MathError> {
    if pool.base_reserve == 0 || pool.quote_reserve == 0 {
        return Err(MathError::ZeroReserves);
    }
    if base == 0 {
        return Err(MathError::ZeroAmount);
    }
    let effective_quote_reserve = pool.effective_quote()?;
    let gross = effective_quote_reserve
        .checked_mul(base)
        .ok_or(MathError::Overflow("gross"))?
        / (pool
            .base_reserve
            .checked_add(base)
            .ok_or(MathError::Overflow("denominator"))?);
    let (bps, tier) = pool_bps(pool, schedule)?;
    let fees = bps.charge(gross, tier)?;
    if pool.quote_reserve < gross.saturating_sub(fees.lp_fee_quote as u128) {
        return Err(MathError::InsufficientRealQuote);
    }
    let net = gross
        .checked_sub(fees.total_quote() as u128)
        .ok_or(MathError::FeesExceedOutput)?;
    Ok(PoolSell {
        quote_out_gross: gross,
        fees,
        quote_out_net: net,
    })
}

fn as_u64(v: u128) -> Result<u64, MathError> {
    u64::try_from(v).map_err(|_| MathError::Overflow("u64"))
}

/// Mid price in quote-atomic per 1e6 base-atomic (one whole 6-decimal token), scaled by 1e9.
pub fn mid_price_e9(quote_reserve: u128, base_reserve: u128) -> Option<u128> {
    if base_reserve == 0 {
        return None;
    }
    quote_reserve
        .checked_mul(1_000_000)?
        .checked_mul(1_000_000_000)?
        .checked_div(base_reserve)
}

/// Realized price for a fill, same units as `mid_price_e9`.
pub fn execution_price_e9(quote_atomic: u128, base_atomic: u128) -> Option<u128> {
    mid_price_e9(quote_atomic, base_atomic)
}

/// Signed basis points of `after` relative to `before`. Positive = after is higher.
pub fn relative_bps(before: u128, after: u128) -> i32 {
    if before == 0 {
        return 0;
    }
    let diff = after as i128 - before as i128;
    let bps = diff.saturating_mul(10_000) / before as i128;
    bps.clamp(i32::MIN as i128, i32::MAX as i128) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_curve() -> CurveState {
        // Global defaults published in pump-public-docs (initial virtual reserves).
        CurveState {
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_quote_reserves: 30_000_000_000,
            real_token_reserves: 793_100_000_000_000,
            real_quote_reserves: 0,
            creator_is_set: true,
            is_mayhem_mode: false,
            mint_supply: ONE_BILLION_SUPPLY,
            cashback_fee_bps: 0,
        }
    }

    fn flat_125() -> CurveFeeSchedule {
        CurveFeeSchedule::Flat {
            protocol_fee_bps: 95,
            creator_fee_bps: 30,
        }
    }

    #[test]
    fn fee_is_ceil_div() {
        assert_eq!(fee(10_000, 95).unwrap(), 95);
        assert_eq!(fee(10_001, 95).unwrap(), 96);
        assert_eq!(fee(1, 95).unwrap(), 1);
        assert_eq!(fee(0, 95).unwrap(), 0);
    }

    #[test]
    fn tier_selection_follows_sdk_semantics() {
        let tiers = vec![
            FeeTier {
                market_cap_lamports_threshold: 0,
                fees: Fees {
                    lp_fee_bps: 0,
                    protocol_fee_bps: 95,
                    creator_fee_bps: 30,
                },
            },
            FeeTier {
                market_cap_lamports_threshold: 100,
                fees: Fees {
                    lp_fee_bps: 0,
                    protocol_fee_bps: 50,
                    creator_fee_bps: 20,
                },
            },
            FeeTier {
                market_cap_lamports_threshold: 1_000,
                fees: Fees {
                    lp_fee_bps: 0,
                    protocol_fee_bps: 10,
                    creator_fee_bps: 5,
                },
            },
        ];
        assert_eq!(calculate_fee_tier(&tiers, 0).protocol_fee_bps, 95);
        assert_eq!(calculate_fee_tier(&tiers, 99).protocol_fee_bps, 95);
        assert_eq!(calculate_fee_tier(&tiers, 100).protocol_fee_bps, 50);
        assert_eq!(calculate_fee_tier(&tiers, 999).protocol_fee_bps, 50);
        assert_eq!(calculate_fee_tier(&tiers, 1_000).protocol_fee_bps, 10);
        assert_eq!(calculate_fee_tier(&tiers, u128::MAX).protocol_fee_bps, 10);
    }

    #[test]
    fn buy_round_trip_never_exceeds_budget() {
        let curve = fresh_curve();
        for spend in [1_000_000u128, 50_000_000, 1_000_000_000, 10_000_000_000] {
            let tokens = curve_buy_tokens_for_sol(&curve, &flat_125(), spend).unwrap();
            let cost = curve_buy_cost_for_tokens(&curve, &flat_125(), tokens).unwrap();
            assert!(
                cost.total_sol <= spend,
                "spend={spend} tokens={tokens} total={}",
                cost.total_sol
            );
            // and the budget is nearly fully used (SDK subtracts 1 lamport of slack).
            assert!(spend - cost.total_sol < spend / 1_000 + 3);
        }
    }

    #[test]
    fn creator_fee_absent_when_creator_default() {
        let mut curve = fresh_curve();
        curve.creator_is_set = false;
        let cost = curve_buy_cost_for_tokens(&curve, &flat_125(), 1_000_000_000).unwrap();
        assert_eq!(cost.fees.creator_fee_quote, 0);
        assert_eq!(cost.fees.creator_fee_bps, 0);
        assert!(cost.fees.protocol_fee_quote > 0);
    }

    #[test]
    fn sell_after_buy_loses_round_trip_fees() {
        let curve = fresh_curve();
        let tokens = curve_buy_tokens_for_sol(&curve, &flat_125(), 1_000_000_000).unwrap();
        let cost = curve_buy_cost_for_tokens(&curve, &flat_125(), tokens).unwrap();
        // apply the buy to the curve
        let after = CurveState {
            virtual_token_reserves: curve.virtual_token_reserves - tokens,
            virtual_quote_reserves: curve.virtual_quote_reserves + cost.sol_cost,
            real_token_reserves: curve.real_token_reserves - tokens,
            real_quote_reserves: cost.sol_cost,
            ..curve
        };
        let sell = curve_sell(&after, &flat_125(), tokens).unwrap();
        assert!(sell.sol_out_net < cost.total_sol);
        let drag_bps = relative_bps(cost.total_sol, sell.sol_out_net);
        // 1.25% each way ≈ 250 bps round trip on a fresh curve with negligible impact.
        assert!(drag_bps < -240 && drag_bps > -270, "drag_bps={drag_bps}");
    }

    #[test]
    fn migrated_curve_cannot_quote() {
        let mut curve = fresh_curve();
        curve.virtual_token_reserves = 0;
        assert_eq!(
            curve_buy_tokens_for_sol(&curve, &flat_125(), 1_000_000),
            Err(MathError::Migrated)
        );
    }

    #[test]
    fn pool_buy_and_sell_are_fee_consistent() {
        let pool = PoolState {
            base_reserve: 200_000_000_000_000,
            quote_reserve: 85_000_000_000,
            virtual_quote_reserves: 0,
            base_mint_supply: ONE_BILLION_SUPPLY,
            coin_creator_is_set: true,
            is_pump_pool: false,
            cashback_fee_bps: 0,
        };
        let schedule = PoolFeeSchedule {
            tiers: vec![],
            flat_fees: Fees {
                lp_fee_bps: 20,
                protocol_fee_bps: 5,
                creator_fee_bps: 5,
            },
        };
        let base = pool_buy_base_for_quote(&pool, &schedule, 1_000_000_000).unwrap();
        let cost = pool_buy_cost_for_base(&pool, &schedule, base).unwrap();
        assert!(cost.total_quote <= 1_000_000_000);
        assert_eq!(cost.fees.lp_fee_bps, 20);
        let sell = pool_sell(&pool, &schedule, base).unwrap();
        assert!(sell.quote_out_net < cost.total_quote);
    }

    #[test]
    fn mid_price_units_are_lamports_per_token_e9() {
        // 30 SOL virtual over 1.073e9 tokens => ~27.96 lamports per token
        let p = mid_price_e9(30_000_000_000, 1_073_000_000_000_000).unwrap();
        assert_eq!(p / 1_000_000_000, 27);
    }
}
