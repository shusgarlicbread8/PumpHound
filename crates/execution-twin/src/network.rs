//! Solana network economics: base fee, compute budget, priority fee, Jito tip scenarios, rent.
//!
//! Solana charges the priority fee on the REQUESTED compute-unit limit, not on consumption, and
//! charges base + priority fee on every transaction that lands — including ones whose program
//! instruction fails. Models here are versioned; parameters with no live observation behind
//! them are labelled `fallback` in the produced evidence so research can discount them.

use crate::contract::{JitoScenario, NetworkCosts, TransactionKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const COMPUTE_BUDGET_MODEL_VERSION: &str = "compute-budget-v1-static";
pub const JITO_MODEL_VERSION: &str = "jito-auction-v1-percentile";
pub const LAMPORTS_PER_SIGNATURE: u64 = 5_000;
/// Rent-exempt minimum for a 165-byte SPL token account (mainnet, 2026).
pub const TOKEN_ACCOUNT_RENT_LAMPORTS: u64 = 2_039_280;
/// Documented Jito minimum tip. A FLOOR, not a landing price.
pub const JITO_MIN_TIP_LAMPORTS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CongestionClass {
    Low,
    #[default]
    Normal,
    High,
    Extreme,
}

impl CongestionClass {
    /// Coarse classification from the priority-fee p50 (micro-lamports/CU). Thresholds are
    /// operator-visible constants, not learned.
    pub fn from_cu_price_p50(p50: u64) -> Self {
        match p50 {
            0..=2_000 => CongestionClass::Low,
            2_001..=50_000 => CongestionClass::Normal,
            50_001..=500_000 => CongestionClass::High,
            _ => CongestionClass::Extreme,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            CongestionClass::Low => "LOW",
            CongestionClass::Normal => "NORMAL",
            CongestionClass::High => "HIGH",
            CongestionClass::Extreme => "EXTREME",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Percentiles {
    pub p25: u64,
    pub p50: u64,
    pub p75: u64,
    pub p95: u64,
}

/// A point-in-time observation of the fee environment. Produced by the core-api oracle
/// (Helius getPriorityFeeEstimate / getRecentPrioritizationFees / Jito tip-floor) or by the
/// documented fallback. Immutable once attached to an execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeEnvironment {
    pub observed_at: DateTime<Utc>,
    pub priority_fee_provider: String,
    /// micro-lamports per CU distribution for the relevant account-lock set / venue.
    pub cu_price_micro_lamports: Percentiles,
    pub jito_tip_provider: String,
    pub jito_tip_lamports: Percentiles,
    pub congestion: CongestionClass,
    /// True when any component came from static fallback values rather than observation.
    pub is_fallback: bool,
    /// Uncertainty penalty (bps of notional) research should add when `is_fallback`.
    pub uncertainty_penalty_bps: u32,
}

impl FeeEnvironment {
    /// Static fallback used when no oracle observation is available. Values are deliberately
    /// conservative (higher than typical calm-market observations) and clearly labelled.
    pub fn fallback(now: DateTime<Utc>) -> Self {
        FeeEnvironment {
            observed_at: now,
            priority_fee_provider: "fallback:static-v1".into(),
            cu_price_micro_lamports: Percentiles {
                p25: 10_000,
                p50: 50_000,
                p75: 200_000,
                p95: 1_000_000,
            },
            jito_tip_provider: "fallback:static-v1".into(),
            jito_tip_lamports: Percentiles {
                p25: 10_000,
                p50: 100_000,
                p75: 500_000,
                p95: 2_000_000,
            },
            congestion: CongestionClass::Normal,
            is_fallback: true,
            uncertainty_penalty_bps: 25,
        }
    }
}

/// Which priority-fee percentile a book pays. Part of the frozen book manifest.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PriorityFeePolicy {
    P25,
    #[default]
    P50,
    P75,
    P95,
}

impl PriorityFeePolicy {
    pub fn pick(self, p: &Percentiles) -> u64 {
        match self {
            PriorityFeePolicy::P25 => p.p25,
            PriorityFeePolicy::P50 => p.p50,
            PriorityFeePolicy::P75 => p.p75,
            PriorityFeePolicy::P95 => p.p95,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PriorityFeePolicy::P25 => "P25",
            PriorityFeePolicy::P50 => "P50",
            PriorityFeePolicy::P75 => "P75",
            PriorityFeePolicy::P95 => "P95",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "P25" => Some(Self::P25),
            "P50" => Some(Self::P50),
            "P75" => Some(Self::P75),
            "P95" => Some(Self::P95),
            _ => None,
        }
    }
}

/// Estimated CU consumption per transaction kind and the requested limit after a safety margin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeBudgetModel {
    pub safety_margin_bps: u32,
}

impl Default for ComputeBudgetModel {
    fn default() -> Self {
        // Solana guidance: simulate, then add a margin instead of over-requesting.
        ComputeBudgetModel {
            safety_margin_bps: 2_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeEstimate {
    pub estimated_cu: u32,
    pub requested_limit: u32,
    pub includes_ata_create: bool,
    pub model_version: &'static str,
}

impl ComputeBudgetModel {
    /// Static v1 estimates from observed mainnet Pump/PumpSwap transactions (swap instruction
    /// only). Shadow observations recalibrate these; until then they are visible constants.
    pub fn swap_cu(kind: TransactionKind) -> u32 {
        match kind {
            TransactionKind::PumpBuyExactSolIn => 60_000,
            TransactionKind::PumpSell => 50_000,
            TransactionKind::PumpSwapBuyExactQuoteIn => 90_000,
            TransactionKind::PumpSwapSell => 85_000,
        }
    }
    pub const ATA_CREATE_CU: u32 = 25_000;
    pub const COMPUTE_BUDGET_IX_CU: u32 = 300;

    pub fn estimate(&self, kind: TransactionKind, create_ata: bool) -> ComputeEstimate {
        let mut cu = Self::swap_cu(kind) + Self::COMPUTE_BUDGET_IX_CU;
        if create_ata {
            cu += Self::ATA_CREATE_CU;
        }
        let limit = (cu as u64 * (10_000 + self.safety_margin_bps as u64) / 10_000) as u32;
        ComputeEstimate {
            estimated_cu: cu,
            requested_limit: limit.min(1_400_000),
            includes_ata_create: create_ata,
            model_version: COMPUTE_BUDGET_MODEL_VERSION,
        }
    }
}

/// priority fee = ceil(limit × price / 1e6), on the REQUESTED limit.
pub fn priority_fee_lamports(cu_limit: u32, cu_price_micro_lamports: u64) -> u64 {
    let num = cu_limit as u128 * cu_price_micro_lamports as u128;
    ((num + 999_999) / 1_000_000) as u64
}

/// Tip scenarios read from observed percentiles; never assume the minimum tip lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JitoAuctionModel;

impl JitoAuctionModel {
    pub fn tip(&self, scenario: JitoScenario, env: &FeeEnvironment) -> u64 {
        match scenario {
            JitoScenario::NoJito => 0,
            JitoScenario::Conservative => env.jito_tip_lamports.p25.max(JITO_MIN_TIP_LAMPORTS),
            JitoScenario::Median => env.jito_tip_lamports.p50.max(JITO_MIN_TIP_LAMPORTS),
            JitoScenario::Aggressive => env.jito_tip_lamports.p75.max(JITO_MIN_TIP_LAMPORTS),
        }
    }
}

/// Full network cost decomposition for one transaction.
pub fn network_costs(
    kind: TransactionKind,
    create_ata: bool,
    compute: &ComputeBudgetModel,
    fee_policy: PriorityFeePolicy,
    scenario: JitoScenario,
    env: &FeeEnvironment,
) -> NetworkCosts {
    let est = compute.estimate(kind, create_ata);
    let cu_price = fee_policy.pick(&env.cu_price_micro_lamports);
    NetworkCosts {
        base_fee_lamports: LAMPORTS_PER_SIGNATURE,
        compute_unit_limit: est.requested_limit,
        compute_units_consumed: est.estimated_cu,
        compute_unit_price_micro_lamports: cu_price,
        priority_fee_lamports: priority_fee_lamports(est.requested_limit, cu_price),
        estimated_jito_tip_lamports: JitoAuctionModel.tip(scenario, env),
        rent_lamports: if create_ata {
            TOKEN_ACCOUNT_RENT_LAMPORTS
        } else {
            0
        },
        priority_fee_provider: format!("{}@{}", env.priority_fee_provider, fee_policy.as_str()),
        jito_scenario: scenario,
        jito_model_version: format!("{JITO_MODEL_VERSION}/{}", env.jito_tip_provider),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_fee_uses_requested_limit_not_consumption() {
        // 100k CU limit at 50k micro-lamports/CU = 5_000 lamports regardless of consumption.
        assert_eq!(priority_fee_lamports(100_000, 50_000), 5_000);
        assert_eq!(priority_fee_lamports(1, 1), 1); // ceil
    }

    #[test]
    fn compute_estimate_applies_margin_and_ata() {
        let m = ComputeBudgetModel::default();
        let a = m.estimate(TransactionKind::PumpBuyExactSolIn, false);
        let b = m.estimate(TransactionKind::PumpBuyExactSolIn, true);
        assert!(b.estimated_cu > a.estimated_cu);
        assert_eq!(a.requested_limit, a.estimated_cu * 12 / 10);
    }

    #[test]
    fn jito_scenarios_are_ordered_and_never_below_floor() {
        let env = FeeEnvironment::fallback(Utc::now());
        let m = JitoAuctionModel;
        assert_eq!(m.tip(JitoScenario::NoJito, &env), 0);
        let c = m.tip(JitoScenario::Conservative, &env);
        let md = m.tip(JitoScenario::Median, &env);
        let ag = m.tip(JitoScenario::Aggressive, &env);
        assert!(JITO_MIN_TIP_LAMPORTS <= c && c <= md && md <= ag);
    }

    #[test]
    fn fallback_environment_is_labelled() {
        let env = FeeEnvironment::fallback(Utc::now());
        assert!(env.is_fallback);
        assert!(env.priority_fee_provider.starts_with("fallback"));
        let costs = network_costs(
            TransactionKind::PumpSell,
            false,
            &ComputeBudgetModel::default(),
            PriorityFeePolicy::P50,
            JitoScenario::NoJito,
            &env,
        );
        assert!(costs.priority_fee_provider.contains("fallback"));
        assert_eq!(costs.rent_lamports, 0);
        assert_eq!(costs.base_fee_lamports, 5_000);
    }
}
