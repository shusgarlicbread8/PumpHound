//! Fee schedules as observed on mainnet, vendored at `vendor/pump-idl/onchain-fee-config.snapshot.json`.
//!
//! The IDL does not carry tier VALUES — they live in the `FeeConfig` PDA owned by the Pump fee
//! program. The snapshot records address, owner, slot, sha256 of raw account data and the
//! decoded tiers so the twin's fee math is reproducible and auditable. Re-vendor (and bump
//! `FEE_SNAPSHOT_VERSION`) when Pump changes tiers; parity tests will fail loudly first.

use crate::pump_math::{CurveFeeSchedule, FeeTier, Fees, PoolFeeSchedule};
use serde::Deserialize;
use std::sync::OnceLock;

pub const FEE_SNAPSHOT_VERSION: &str = "onchain-fee-config-2026-09-09";
const SNAPSHOT_JSON: &str =
    include_str!("../../../vendor/pump-idl/onchain-fee-config.snapshot.json");

#[derive(Debug, Deserialize)]
struct Snapshot {
    retrieved_at: String,
    accounts: Accounts,
}

#[derive(Debug, Deserialize)]
struct Accounts {
    #[serde(rename = "PUMP_FEE_CONFIG")]
    pump_fee_config: Account<FeeConfigDecoded>,
    #[serde(rename = "AMM_FEE_CONFIG")]
    amm_fee_config: Account<FeeConfigDecoded>,
    #[serde(rename = "PUMP_GLOBAL")]
    pump_global: Account<PumpGlobalDecoded>,
    #[serde(rename = "AMM_GLOBAL_CONFIG")]
    amm_global_config: Account<AmmGlobalDecoded>,
}

#[derive(Debug, Deserialize)]
struct Account<T> {
    address: String,
    slot: u64,
    data_sha256: String,
    decoded: T,
}

#[derive(Debug, Deserialize)]
struct FeesDecoded {
    lp_fee_bps: u64,
    protocol_fee_bps: u64,
    creator_fee_bps: u64,
}

#[derive(Debug, Deserialize)]
struct FeeTierDecoded {
    market_cap_lamports_threshold: u128,
    fees: FeesDecoded,
}

#[derive(Debug, Deserialize)]
struct FeeConfigDecoded {
    flat_fees: FeesDecoded,
    fee_tiers: Vec<FeeTierDecoded>,
}

#[derive(Debug, Deserialize)]
struct PumpGlobalDecoded {
    fee_recipient: String,
    initial_virtual_token_reserves: u128,
    initial_virtual_sol_reserves: u128,
    initial_real_token_reserves: u128,
    token_total_supply: u128,
    fee_basis_points: u64,
    creator_fee_basis_points: u64,
}

#[derive(Debug, Deserialize)]
struct AmmGlobalDecoded {
    protocol_fee_recipients: Vec<String>,
    lp_fee_basis_points: u64,
    protocol_fee_basis_points: u64,
    coin_creator_fee_basis_points: u64,
}

/// Everything the twin needs to price a Pump or PumpSwap swap exactly.
#[derive(Clone, Debug)]
pub struct FeeSchedules {
    pub version: String,
    pub retrieved_at: String,
    pub pump_fee_config_address: String,
    pub pump_fee_config_slot: u64,
    pub pump_fee_config_sha256: String,
    pub amm_fee_config_address: String,
    pub amm_fee_config_slot: u64,
    pub amm_fee_config_sha256: String,
    pub curve: CurveFeeSchedule,
    /// Global flat fallback the program uses when no FeeConfig account is passed.
    pub curve_global_flat: CurveFeeSchedule,
    pub pool: PoolFeeSchedule,
    pub initial_virtual_token_reserves: u128,
    pub initial_virtual_sol_reserves: u128,
    pub initial_real_token_reserves: u128,
    pub token_total_supply: u128,
    /// PumpSwap GlobalConfig flat bps (program fallback when FeeConfig absent).
    pub amm_global_lp_fee_bps: u64,
    pub amm_global_protocol_fee_bps: u64,
    pub amm_global_creator_fee_bps: u64,
    /// `Global.fee_recipient` — the account the Pump `buy`/`sell` instructions must be given.
    pub pump_fee_recipient: String,
    /// `GlobalConfig.protocol_fee_recipients` — any one is valid for PumpSwap instructions.
    pub amm_protocol_fee_recipients: Vec<String>,
}

fn convert(t: &FeeTierDecoded) -> FeeTier {
    FeeTier {
        market_cap_lamports_threshold: t.market_cap_lamports_threshold,
        fees: Fees {
            lp_fee_bps: t.fees.lp_fee_bps,
            protocol_fee_bps: t.fees.protocol_fee_bps,
            creator_fee_bps: t.fees.creator_fee_bps,
        },
    }
}

fn load() -> FeeSchedules {
    let snap: Snapshot = serde_json::from_str(SNAPSHOT_JSON).expect("vendored fee snapshot parses");
    let pump = &snap.accounts.pump_fee_config;
    let amm = &snap.accounts.amm_fee_config;
    let global = &snap.accounts.pump_global.decoded;
    let amm_global = &snap.accounts.amm_global_config.decoded;
    FeeSchedules {
        version: FEE_SNAPSHOT_VERSION.into(),
        retrieved_at: snap.retrieved_at.clone(),
        pump_fee_config_address: pump.address.clone(),
        pump_fee_config_slot: pump.slot,
        pump_fee_config_sha256: pump.data_sha256.clone(),
        amm_fee_config_address: amm.address.clone(),
        amm_fee_config_slot: amm.slot,
        amm_fee_config_sha256: amm.data_sha256.clone(),
        curve: CurveFeeSchedule::Tiered {
            tiers: pump.decoded.fee_tiers.iter().map(convert).collect(),
        },
        curve_global_flat: CurveFeeSchedule::Flat {
            protocol_fee_bps: global.fee_basis_points,
            creator_fee_bps: global.creator_fee_basis_points,
        },
        pool: PoolFeeSchedule {
            tiers: amm.decoded.fee_tiers.iter().map(convert).collect(),
            flat_fees: Fees {
                lp_fee_bps: amm.decoded.flat_fees.lp_fee_bps,
                protocol_fee_bps: amm.decoded.flat_fees.protocol_fee_bps,
                creator_fee_bps: amm.decoded.flat_fees.creator_fee_bps,
            },
        },
        initial_virtual_token_reserves: global.initial_virtual_token_reserves,
        initial_virtual_sol_reserves: global.initial_virtual_sol_reserves,
        initial_real_token_reserves: global.initial_real_token_reserves,
        token_total_supply: global.token_total_supply,
        amm_global_lp_fee_bps: amm_global.lp_fee_basis_points,
        amm_global_protocol_fee_bps: amm_global.protocol_fee_basis_points,
        amm_global_creator_fee_bps: amm_global.coin_creator_fee_basis_points,
        pump_fee_recipient: global.fee_recipient.clone(),
        amm_protocol_fee_recipients: amm_global.protocol_fee_recipients.clone(),
    }
}

pub fn schedules() -> &'static FeeSchedules {
    static CELL: OnceLock<FeeSchedules> = OnceLock::new();
    CELL.get_or_init(load)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pda;

    #[test]
    fn snapshot_addresses_match_derived_pdas() {
        let s = schedules();
        assert_eq!(
            s.pump_fee_config_address,
            pda::pump::fee_config().to_base58()
        );
        assert_eq!(
            s.amm_fee_config_address,
            pda::pump_amm::fee_config().to_base58()
        );
    }

    #[test]
    fn curve_schedule_is_125bps_at_snapshot() {
        let s = schedules();
        match &s.curve {
            CurveFeeSchedule::Tiered { tiers } => {
                assert!(!tiers.is_empty());
                assert_eq!(tiers[0].fees.protocol_fee_bps, 95);
                assert_eq!(tiers[0].fees.creator_fee_bps, 30);
            }
            _ => panic!("expected tiered"),
        }
    }

    #[test]
    fn pool_schedule_has_market_cap_tiers() {
        let s = schedules();
        assert!(
            s.pool.tiers.len() >= 2,
            "PumpSwap tiers are market-cap dependent"
        );
        assert_eq!(s.pool.tiers[0].market_cap_lamports_threshold, 0);
        assert!(s
            .pool
            .tiers
            .windows(2)
            .all(|w| w[0].market_cap_lamports_threshold < w[1].market_cap_lamports_threshold));
    }
}
