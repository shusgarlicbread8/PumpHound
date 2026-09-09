//! Executable liquidation mark: "if PumpHound sold this entire paper position now, after
//! protocol fees and estimated network costs, what would it receive?"

use crate::contract::JitoScenario;
use crate::network::{self, ComputeBudgetModel, FeeEnvironment, PriorityFeePolicy};
use crate::quote::{exact_quote, QuoteRejection};
use pumphound_domain::{MarketSnapshot, TradeSide};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutableMark {
    pub base_atomic: u64,
    /// mid × size, no fees, no impact — the "decorative" number.
    pub theoretical_quote: u64,
    /// Gross swap output at current curve/pool state (impact included, fees excluded).
    pub gross_quote: u64,
    pub protocol_fees_quote: u64,
    pub network_cost_lamports: u64,
    /// What the position would actually realize now.
    pub executable_quote: u64,
    pub liquidation_impact_bps: u32,
    /// theoretical − executable, in quote atomic.
    pub liquidation_cost_quote: u64,
    pub market_snapshot_id: String,
    pub simulator_version: &'static str,
}

pub fn executable_mark(
    snapshot: &MarketSnapshot,
    base_atomic: u64,
    fee_env: &FeeEnvironment,
    fee_policy: PriorityFeePolicy,
    scenario: JitoScenario,
) -> Result<ExecutableMark, QuoteRejection> {
    let q = exact_quote(snapshot, TradeSide::Sell, base_atomic, 0)?;
    let net = network::network_costs(
        q.transaction_kind,
        false,
        &ComputeBudgetModel::default(),
        fee_policy,
        scenario,
        fee_env,
    );
    let theoretical =
        (q.mid_price_before_e9 * base_atomic as u128 / 1_000_000 / 1_000_000_000) as u64;
    let executable = q.filled_quote.saturating_sub(net.landed_cost_lamports());
    Ok(ExecutableMark {
        base_atomic,
        theoretical_quote: theoretical,
        gross_quote: q.swap_quote,
        protocol_fees_quote: q.protocol_fees.total_quote(),
        network_cost_lamports: net.landed_cost_lamports(),
        executable_quote: executable,
        liquidation_impact_bps: q.price_impact_bps,
        liquidation_cost_quote: theoretical.saturating_sub(executable),
        market_snapshot_id: snapshot.snapshot_id.clone(),
        simulator_version: crate::contract::SIMULATOR_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pda::{self, pk};
    use chrono::Utc;
    use pumphound_domain::*;

    fn snap() -> MarketSnapshot {
        let mint = pk("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        MarketSnapshot {
            snapshot_id: "m".into(),
            market: pda::pump::bonding_curve(&mint),
            mint,
            quote_mint: pk(WSOL_MINT),
            protocol: Protocol::PumpCurve,
            as_of: ObservationPoint {
                slot: Slot(1),
                tx_index: None,
                event_index: None,
                event_time: Some(now_utc()),
                observed_at: now_utc(),
                knowledge_cutoff: now_utc(),
                commitment: Commitment::Confirmed,
            },
            canonicality: Canonicality::Provisional,
            virtual_base_atomic: Atomic(1_000_000_000_000_000),
            virtual_quote_atomic: Atomic(40_000_000_000),
            real_base_atomic: Atomic(720_000_000_000_000),
            real_quote_atomic: Atomic(10_000_000_000),
            effective_base_atomic: Atomic(1_000_000_000_000_000),
            effective_quote_atomic: Atomic(40_000_000_000),
            complete: false,
            migrated_market: None,
            config_snapshot_id: "t".into(),
            quality: DataQuality::live(0, 0, true),
            is_mayhem_mode: Some(false),
            creator_fee_applies: Some(true),
            base_supply_atomic: None,
            cashback_fee_bps: Some(0),
        }
    }

    #[test]
    fn executable_mark_degrades_with_position_size() {
        let env = FeeEnvironment::fallback(Utc::now());
        let small = executable_mark(
            &snap(),
            1_000_000_000_000,
            &env,
            PriorityFeePolicy::P50,
            JitoScenario::NoJito,
        )
        .unwrap();
        let large = executable_mark(
            &snap(),
            100_000_000_000_000,
            &env,
            PriorityFeePolicy::P50,
            JitoScenario::NoJito,
        )
        .unwrap();
        assert!(small.executable_quote < small.theoretical_quote);
        assert!(large.liquidation_impact_bps > small.liquidation_impact_bps);
        // per-token realized value must be worse for the larger position
        let small_px = small.executable_quote as u128 * 1_000_000 / small.base_atomic as u128;
        let large_px = large.executable_quote as u128 * 1_000_000 / large.base_atomic as u128;
        assert!(large_px < small_px, "{large_px} !< {small_px}");
        assert_eq!(
            small.liquidation_cost_quote,
            small.theoretical_quote - small.executable_quote
        );
    }
}
