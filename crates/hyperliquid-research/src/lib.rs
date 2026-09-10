//! HIP-NEW-MARKET-V1 research books. No LLM authority. No fake lift.

use chrono::{DateTime, Utc};
use pumphound_venue_core::{OptionalNumber, ResearchStatus};
use pumphound_venue_hyperliquid::{HipClass, HyperliquidInstrument};
use serde::{Deserialize, Serialize};

pub const EXPERIMENT_FAMILY: &str = "HIP-NEW-MARKET-V1";
pub const RULE_VERSION: &str = "hip-new-market-v1";
pub const MIN_LABELS_FOR_MODEL: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Book {
    RandomNewMarket,
    MicrostructureBaseline,
    MaturationRanker,
    StructuralEdge,
    FullQuant,
    FullQuantAiShadow,
}

impl Book {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RandomNewMarket => "RANDOM_NEW_MARKET",
            Self::MicrostructureBaseline => "MICROSTRUCTURE_BASELINE",
            Self::MaturationRanker => "MATURATION_RANKER",
            Self::StructuralEdge => "STRUCTURAL_EDGE",
            Self::FullQuant => "FULL_QUANT",
            Self::FullQuantAiShadow => "FULL_QUANT_AI_SHADOW",
        }
    }

    pub fn activation(self, valid_labels: usize) -> BookActivation {
        match self {
            Self::RandomNewMarket | Self::MicrostructureBaseline => BookActivation {
                active: true,
                status: ResearchStatus::SampleOnly,
                reason: "deterministic baseline; no model required".into(),
            },
            Self::MaturationRanker | Self::StructuralEdge | Self::FullQuant | Self::FullQuantAiShadow => {
                if valid_labels >= MIN_LABELS_FOR_MODEL {
                    BookActivation {
                        active: true,
                        status: ResearchStatus::Unproven,
                        reason: format!("{valid_labels} labels available; not promoted"),
                    }
                } else {
                    BookActivation {
                        active: false,
                        status: ResearchStatus::InsufficientSample,
                        reason: format!(
                            "{valid_labels} valid labels < {MIN_LABELS_FOR_MODEL}; book stays dark"
                        ),
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookActivation {
    pub active: bool,
    pub status: ResearchStatus,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EligibleMarket {
    pub coin: String,
    pub class: HipClass,
    pub first_observed_at: DateTime<Utc>,
    pub age_seconds: i64,
    pub spread: OptionalNumber,
    pub depth_1k: Option<bool>,
    pub volume: OptionalNumber,
    pub open_interest: OptionalNumber,
    pub funding: OptionalNumber,
}

pub fn eligible(now: DateTime<Utc>, instrument: &HyperliquidInstrument, first_observed_at: DateTime<Utc>) -> Option<EligibleMarket> {
    if instrument.delisted {
        return None;
    }
    let age = now
        .signed_duration_since(first_observed_at)
        .num_seconds()
        .max(0);
    // Collector start is not listing time. Native perps stay out until a real
    // listing/deployment timestamp exists. HIP-3 is identified by named dex.
    if instrument.class != HipClass::Hip3 {
        return None;
    }
    let spread = match (instrument.mark.as_f64(), instrument.mid.as_f64()) {
        (Some(mark), Some(mid)) if mid > 0.0 => OptionalNumber::observed((mark - mid).abs() / mid),
        _ => match (instrument.impact_bid.as_f64(), instrument.impact_ask.as_f64()) {
            (Some(b), Some(a)) if ((a + b) / 2.0) > 0.0 => {
                OptionalNumber::observed((a - b).abs() / ((a + b) / 2.0))
            }
            _ => OptionalNumber::unknown(),
        },
    };
    Some(EligibleMarket {
        coin: instrument.coin.clone(),
        class: instrument.class,
        first_observed_at,
        age_seconds: age,
        spread,
        depth_1k: None,
        volume: instrument.day_notional_volume,
        open_interest: instrument.open_interest,
        funding: instrument.funding,
    })
}

pub fn random_pick<'a>(universe: &'a [EligibleMarket], salt: &str) -> Option<&'a EligibleMarket> {
    if universe.is_empty() {
        return None;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in salt.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(&universe[h as usize % universe.len()])
}

pub fn microstructure_score(row: &EligibleMarket) -> OptionalNumber {
    let (Some(spread), Some(volume), Some(oi)) =
        (row.spread.as_f64(), row.volume.as_f64(), row.open_interest.as_f64())
    else {
        return OptionalNumber::unknown();
    };
    let age_pen = (row.age_seconds as f64 / 86_400.0).min(2.0);
    OptionalNumber::observed((volume.ln_1p() + oi.ln_1p()) / (1.0 + spread * 100.0 + age_pen))
}

pub fn compare_books(same_universe: bool, same_window: bool, same_sizing: bool, same_twin: bool, same_exit: bool) -> ResearchStatus {
    if same_universe && same_window && same_sizing && same_twin && same_exit {
        ResearchStatus::SampleOnly
    } else {
        ResearchStatus::Confounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumphound_venue_core::{InstrumentId, Venue};

    fn inst(coin: &str, class: HipClass) -> HyperliquidInstrument {
        HyperliquidInstrument {
            instrument_id: InstrumentId::new(Venue::Hyperliquid, coin),
            dex: if class == HipClass::Hip3 {
                "xyz".into()
            } else {
                String::new()
            },
            coin: coin.into(),
            display_name: coin.into(),
            class,
            sz_decimals: Some(2),
            max_leverage: Some(10),
            delisted: false,
            growth_mode: None,
            deployer_fee_scale: OptionalNumber::unknown(),
            deployer: None,
            mark: OptionalNumber::observed(10.0),
            mid: OptionalNumber::observed(10.0),
            oracle: OptionalNumber::observed(10.0),
            funding: OptionalNumber::observed(0.0),
            open_interest: OptionalNumber::observed(1.0),
            premium: OptionalNumber::unknown(),
            day_notional_volume: OptionalNumber::observed(100.0),
            impact_bid: OptionalNumber::observed(9.9),
            impact_ask: OptionalNumber::observed(10.1),
            prev_day_px: OptionalNumber::observed(10.0),
        }
    }

    #[test]
    fn native_btc_is_never_in_new_market_universe_without_listing_time() {
        let t0 = Utc::now();
        assert!(eligible(Utc::now(), &inst("BTC", HipClass::Native), t0).is_none());
        let old = Utc::now() - chrono::Duration::days(30);
        assert!(eligible(Utc::now(), &inst("BTC", HipClass::Native), old).is_none());
    }

    #[test]
    fn hip3_is_eligible_even_if_old_to_this_collector() {
        let t0 = Utc::now() - chrono::Duration::days(30);
        assert!(eligible(Utc::now(), &inst("xyz:NVDA", HipClass::Hip3), t0).is_some());
    }

    #[test]
    fn ranker_stays_dark_without_labels() {
        let act = Book::MaturationRanker.activation(12);
        assert!(!act.active);
        assert_eq!(act.status, ResearchStatus::InsufficientSample);
    }

    #[test]
    fn unequal_universes_are_confounded() {
        assert_eq!(
            compare_books(false, true, true, true, true),
            ResearchStatus::Confounded
        );
    }
}
