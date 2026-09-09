use crate::fees::{apply_curve_fees, DEFAULT_FEE_BPS};
use chrono::Utc;
use pumphound_domain::{
    now_utc, AssetAmount, Atomic, DataQuality, MarketSnapshot, PaperError, PaperOrderRequest,
    PaperQuote, Protocol, TradeSide,
};
use uuid::Uuid;

pub fn quote_order(
    order: &PaperOrderRequest,
    state: &MarketSnapshot,
) -> Result<PaperQuote, PaperError> {
    if state.quality.grade == pumphound_domain::QualityGrade::Unavailable
        || state.quality.coverage != pumphound_domain::Coverage::CompleteForScope
    {
        return Err(PaperError::QuoteUnavailable(
            "market state is incomplete for executable quoting".into(),
        ));
    }
    if state.effective_quote_atomic.is_zero() || state.effective_base_atomic.is_zero() {
        return Err(PaperError::QuoteUnavailable(
            "unknown or zero reserves".into(),
        ));
    }
    if state.complete && state.protocol == Protocol::PumpCurve {
        return Err(PaperError::QuoteUnavailable(
            "curve complete; use PumpSwap market".into(),
        ));
    }
    let (out, reserve_in, fees, impact) = match (state.protocol, order.side) {
        (Protocol::PumpCurve, TradeSide::Buy) => buy_curve(order.input.atomic.0, state)?,
        (Protocol::PumpCurve, TradeSide::Sell) => sell_curve(order.input.atomic.0, state)?,
        (Protocol::PumpSwap, TradeSide::Buy) => buy_amm(order.input.atomic.0, state)?,
        (Protocol::PumpSwap, TradeSide::Sell) => sell_amm(order.input.atomic.0, state)?,
    };
    let min = out.saturating_mul(10_000 - order.max_slippage_bps as u128) / 10_000;
    Ok(PaperQuote {
        quote_id: Uuid::now_v7(),
        market_snapshot_id: state.snapshot_id.clone(),
        input: order.input.clone(),
        expected_output: AssetAmount {
            mint: order.output_mint,
            atomic: Atomic(out),
            decimals: if order.side == TradeSide::Buy {
                6
            } else {
                order.input.decimals
            },
        },
        minimum_output: AssetAmount {
            mint: order.output_mint,
            atomic: Atomic(min),
            decimals: if order.side == TradeSide::Buy {
                6
            } else {
                order.input.decimals
            },
        },
        reserve_input: AssetAmount {
            mint: order.input.mint,
            atomic: Atomic(reserve_in),
            decimals: order.input.decimals,
        },
        fee_components: fees,
        price_impact_bps: impact,
        expires_at: Utc::now() + chrono::Duration::seconds(2),
        quality: DataQuality::live(
            state.quality.chain_lag_ms.unwrap_or(0),
            0,
            matches!(
                state.quality.ordering,
                pumphound_domain::OrderingQuality::Exact
            ),
        ),
    })
}

fn buy_curve(
    gross_in: u128,
    state: &MarketSnapshot,
) -> Result<(u128, u128, Vec<pumphound_domain::FeeComponent>, u32), PaperError> {
    let (reserve_in, fees) = apply_curve_fees(gross_in, order_mint(state), DEFAULT_FEE_BPS, 100)?;
    let vq = state.effective_quote_atomic.0;
    let vb = state.effective_base_atomic.0;
    let k = vq
        .checked_mul(vb)
        .ok_or(PaperError::Arithmetic("k".into()))?;
    let new_q = vq
        .checked_add(reserve_in)
        .ok_or(PaperError::Arithmetic("new_q".into()))?;
    let new_b = k
        .checked_div(new_q)
        .ok_or(PaperError::Arithmetic("div".into()))?;
    let out = vb.saturating_sub(new_b);
    if out == 0 {
        return Err(PaperError::QuoteUnavailable("zero output".into()));
    }
    let impact = impact_bps(vq, reserve_in);
    Ok((out, reserve_in, fees, impact))
}

fn sell_curve(
    base_in: u128,
    state: &MarketSnapshot,
) -> Result<(u128, u128, Vec<pumphound_domain::FeeComponent>, u32), PaperError> {
    let vq = state.effective_quote_atomic.0;
    let vb = state.effective_base_atomic.0;
    let k = vq
        .checked_mul(vb)
        .ok_or(PaperError::Arithmetic("k".into()))?;
    let new_b = vb
        .checked_add(base_in)
        .ok_or(PaperError::Arithmetic("new_b".into()))?;
    let new_q = k
        .checked_div(new_b)
        .ok_or(PaperError::Arithmetic("div".into()))?;
    let gross_out = vq.saturating_sub(new_q);
    let (net, fees) = apply_curve_fees(gross_out, order_mint(state), DEFAULT_FEE_BPS, 100)?;
    Ok((net, base_in, fees, impact_bps(vb, base_in)))
}

fn buy_amm(
    gross_in: u128,
    state: &MarketSnapshot,
) -> Result<(u128, u128, Vec<pumphound_domain::FeeComponent>, u32), PaperError> {
    // constant product on effective reserves (pool + virtual quote)
    buy_curve(gross_in, state)
}

fn sell_amm(
    base_in: u128,
    state: &MarketSnapshot,
) -> Result<(u128, u128, Vec<pumphound_domain::FeeComponent>, u32), PaperError> {
    sell_curve(base_in, state)
}

fn impact_bps(reserve: u128, input: u128) -> u32 {
    if reserve == 0 {
        return 0;
    }
    ((input.saturating_mul(10_000) / reserve) as u32).min(10_000)
}

fn order_mint(state: &MarketSnapshot) -> pumphound_domain::Pubkey {
    state.quote_mint
}

#[allow(dead_code)]
fn _now() {
    let _ = now_utc();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumphound_domain::{
        Canonicality, Commitment, ObservationPoint, PaperOrderRequest, Pubkey, Slot, WSOL_MINT,
    };

    fn snapshot(id: &str, quote_reserve: u128) -> MarketSnapshot {
        let mint = Pubkey::from_base58("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        let quote = Pubkey::from_base58(WSOL_MINT).unwrap();
        MarketSnapshot {
            snapshot_id: id.into(),
            market: mint,
            mint,
            quote_mint: quote,
            protocol: Protocol::PumpCurve,
            as_of: ObservationPoint {
                slot: Slot(if id == "decision" { 1 } else { 2 }),
                tx_index: None,
                event_index: None,
                event_time: Some(now_utc()),
                observed_at: now_utc(),
                knowledge_cutoff: now_utc(),
                commitment: Commitment::Confirmed,
            },
            canonicality: Canonicality::Provisional,
            virtual_base_atomic: Atomic(1_000_000_000_000),
            virtual_quote_atomic: Atomic(quote_reserve),
            real_base_atomic: Atomic(1_000_000_000_000),
            real_quote_atomic: Atomic(quote_reserve),
            effective_base_atomic: Atomic(1_000_000_000_000),
            effective_quote_atomic: Atomic(quote_reserve),
            complete: false,
            migrated_market: None,
            config_snapshot_id: "test".into(),
            quality: DataQuality::live(0, 0, true),
            is_mayhem_mode: None,
            creator_fee_applies: None,
            base_supply_atomic: None,
            cashback_fee_bps: None,
        }
    }

    #[test]
    fn arrival_state_reprices_decision_quote() {
        let input_mint = Pubkey::from_base58(WSOL_MINT).unwrap();
        let output_mint =
            Pubkey::from_base58("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        let request = PaperOrderRequest {
            client_request_id: Uuid::nil(),
            portfolio_id: Uuid::nil(),
            launch_id: None,
            market: output_mint,
            side: TradeSide::Buy,
            input: AssetAmount {
                mint: input_mint,
                atomic: Atomic(50_000_000),
                decimals: 9,
            },
            output_mint,
            max_slippage_bps: 100,
            quote_id: Uuid::nil(),
            policy_version: "test".into(),
            decision_id: Uuid::nil(),
        };
        let decision = quote_order(&request, &snapshot("decision", 30_000_000_000)).unwrap();
        let arrival = quote_order(&request, &snapshot("arrival", 40_000_000_000)).unwrap();
        assert_eq!(arrival.market_snapshot_id, "arrival");
        assert_ne!(
            decision.expected_output.atomic,
            arrival.expected_output.atomic
        );
    }

    #[test]
    fn incomplete_historical_state_cannot_produce_quote() {
        let input_mint = Pubkey::from_base58(WSOL_MINT).unwrap();
        let output_mint =
            Pubkey::from_base58("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
        let request = PaperOrderRequest {
            client_request_id: Uuid::nil(),
            portfolio_id: Uuid::nil(),
            launch_id: None,
            market: output_mint,
            side: TradeSide::Buy,
            input: AssetAmount {
                mint: input_mint,
                atomic: Atomic(1_000_000),
                decimals: 9,
            },
            output_mint,
            max_slippage_bps: 100,
            quote_id: Uuid::nil(),
            policy_version: "test".into(),
            decision_id: Uuid::nil(),
        };
        let mut incomplete = snapshot("historical", 30_000_000_000);
        incomplete.quality = DataQuality::unknown("legacy trade lacks fee inputs");

        assert!(matches!(
            quote_order(&request, &incomplete),
            Err(PaperError::QuoteUnavailable(_))
        ));
    }
}
