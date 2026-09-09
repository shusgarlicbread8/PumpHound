//! Landing model v1: an explicit, versioned prior over "when (if ever) does this transaction
//! land", sampled deterministically from the trace so replay reproduces the same outcome.
//!
//! Status: PRIOR, NOT CALIBRATED. The probabilities below are operator-visible assumptions
//! conditioned only on congestion class and whether a tip is paid. Shadow landing observations
//! (`landing_observations`) exist to replace them; until then every result carries
//! `landing_model_version = landing-v1-prior` so research can weight it accordingly.

use crate::contract::{FailureClass, JitoScenario, LandingSample};
use crate::network::CongestionClass;
use chrono::{DateTime, Duration, Utc};
use pumphound_domain::Slot;
use serde::{Deserialize, Serialize};

pub const LANDING_MODEL_VERSION: &str = "landing-v1-prior";
/// Mainnet slot time used to convert slots into wall-clock delay.
pub const SLOT_MS: u64 = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingProbabilities {
    pub p_land_current_slot_bps: u32,
    pub p_land_plus_1_bps: u32,
    pub p_land_plus_2_bps: u32,
    pub p_expire_bps: u32,
}

impl LandingProbabilities {
    fn total(&self) -> u32 {
        self.p_land_current_slot_bps
            + self.p_land_plus_1_bps
            + self.p_land_plus_2_bps
            + self.p_expire_bps
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingModel {
    /// Build + sign + submit path latency before the transaction can enter a slot.
    pub submit_latency_ms: u64,
}

impl Default for LandingModel {
    fn default() -> Self {
        LandingModel {
            submit_latency_ms: 350,
        }
    }
}

impl LandingModel {
    pub fn probabilities(
        congestion: CongestionClass,
        scenario: JitoScenario,
    ) -> LandingProbabilities {
        let tipped = scenario != JitoScenario::NoJito;
        match (congestion, tipped) {
            (CongestionClass::Low, false) => lp(5_500, 3_200, 900, 400),
            (CongestionClass::Low, true) => lp(7_000, 2_400, 400, 200),
            (CongestionClass::Normal, false) => lp(4_000, 3_500, 1_500, 1_000),
            (CongestionClass::Normal, true) => lp(6_000, 2_800, 800, 400),
            (CongestionClass::High, false) => lp(2_000, 3_000, 2_500, 2_500),
            (CongestionClass::High, true) => lp(4_500, 3_000, 1_500, 1_000),
            (CongestionClass::Extreme, false) => lp(800, 1_700, 2_500, 5_000),
            (CongestionClass::Extreme, true) => lp(2_500, 2_500, 2_500, 2_500),
        }
    }

    /// Deterministic sample. `seed` must be derived from immutable trace identity.
    pub fn sample(
        &self,
        seed: u64,
        decision_at: DateTime<Utc>,
        decision_slot: Slot,
        congestion: CongestionClass,
        scenario: JitoScenario,
    ) -> (LandingSample, Option<FailureClass>) {
        let p = Self::probabilities(congestion, scenario);
        debug_assert_eq!(p.total(), 10_000);
        let roll = (splitmix64(seed) % 10_000) as u32;
        let attempted_slot = Slot(decision_slot.0 + 1);
        let (landing_slots, failure) = if roll < p.p_land_current_slot_bps {
            (Some(0u32), None)
        } else if roll < p.p_land_current_slot_bps + p.p_land_plus_1_bps {
            (Some(1), None)
        } else if roll < p.p_land_current_slot_bps + p.p_land_plus_1_bps + p.p_land_plus_2_bps {
            (Some(2), None)
        } else {
            (None, Some(FailureClass::LandingTimeout))
        };
        // Expired transactions are only known to have failed after the blockhash window; for
        // paper accounting we resolve them at +3 slots.
        let elapsed_slots = landing_slots.unwrap_or(3) as u64 + 1;
        let arrival_delay_ms = self.submit_latency_ms + elapsed_slots * SLOT_MS;
        let arrival_slot = Slot(attempted_slot.0 + landing_slots.unwrap_or(3) as u64);
        (
            LandingSample {
                landing_model_version: LANDING_MODEL_VERSION.into(),
                arrival_timestamp: decision_at + Duration::milliseconds(arrival_delay_ms as i64),
                arrival_slot,
                attempted_slot,
                landed_slot: landing_slots.map(|_| arrival_slot),
                landing_slots,
                arrival_delay_ms,
                p_land_current_slot_bps: p.p_land_current_slot_bps,
                p_land_plus_1_bps: p.p_land_plus_1_bps,
                p_land_plus_2_bps: p.p_land_plus_2_bps,
                p_expire_bps: p.p_expire_bps,
                seed,
            },
            failure,
        )
    }
}

fn lp(a: u32, b: u32, c: u32, d: u32) -> LandingProbabilities {
    LandingProbabilities {
        p_land_current_slot_bps: a,
        p_land_plus_1_bps: b,
        p_land_plus_2_bps: c,
        p_expire_bps: d,
    }
}

/// Seed from immutable identifiers (uuid bytes) — stable across processes and restarts.
pub fn seed_from(parts: &[&[u8]]) -> u64 {
    let mut h = sha2::Sha256::new();
    use sha2::Digest;
    for p in parts {
        h.update(p);
    }
    let out = h.finalize();
    u64::from_le_bytes(out[..8].try_into().unwrap())
}

fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_priors_sum_to_one() {
        for c in [
            CongestionClass::Low,
            CongestionClass::Normal,
            CongestionClass::High,
            CongestionClass::Extreme,
        ] {
            for s in [JitoScenario::NoJito, JitoScenario::Median] {
                assert_eq!(LandingModel::probabilities(c, s).total(), 10_000);
            }
        }
    }

    #[test]
    fn sampling_is_deterministic_for_a_seed() {
        let m = LandingModel::default();
        let now = Utc::now();
        let a = m.sample(
            42,
            now,
            Slot(100),
            CongestionClass::Normal,
            JitoScenario::NoJito,
        );
        let b = m.sample(
            42,
            now,
            Slot(100),
            CongestionClass::Normal,
            JitoScenario::NoJito,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn not_every_transaction_lands() {
        let m = LandingModel::default();
        let now = Utc::now();
        let mut expired = 0;
        let mut delays = std::collections::BTreeSet::new();
        for seed in 0..2_000u64 {
            let (s, f) = m.sample(
                seed,
                now,
                Slot(1),
                CongestionClass::Normal,
                JitoScenario::NoJito,
            );
            if f.is_some() {
                expired += 1;
                assert!(s.landed_slot.is_none());
            }
            delays.insert(s.arrival_delay_ms);
        }
        // prior says 10% expire; allow generous tolerance on 2k samples
        assert!((120..=280).contains(&expired), "expired={expired}");
        assert!(
            delays.len() >= 3,
            "landing delays must vary, got {delays:?}"
        );
    }
}
