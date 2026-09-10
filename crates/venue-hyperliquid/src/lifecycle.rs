use chrono::{DateTime, Utc};
use pumphound_venue_core::OptionalNumber;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Lifecycle {
    Discovered,
    New,
    Forming,
    Liquid,
    Mature,
    Stalled,
    Delisted,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LifecycleInput {
    pub first_observed_at: DateTime<Utc>,
    pub now: DateTime<Utc>,
    pub delisted: bool,
    pub listing_known: bool,
    pub day_notional_volume: OptionalNumber,
    pub depth_25k_available: Option<bool>,
    pub spread: OptionalNumber,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgeSnapshot {
    pub target_age_seconds: i64,
    pub actual_observed_at: DateTime<Utc>,
    pub observation_delay_seconds: i64,
}

pub fn classify_lifecycle(input: &LifecycleInput) -> Lifecycle {
    if input.delisted {
        return Lifecycle::Delisted;
    }
    if !input.listing_known {
        return Lifecycle::Discovered;
    }
    let age = input
        .now
        .signed_duration_since(input.first_observed_at)
        .num_seconds()
        .max(0);
    let volume = input.day_notional_volume.as_f64();
    let stalled = matches!(volume, Some(v) if v == 0.0) && age > 6 * 3600;
    if stalled {
        return Lifecycle::Stalled;
    }
    let liquid = input.depth_25k_available == Some(true)
        && input
            .spread
            .as_f64()
            .map(|s| s < 0.005)
            .unwrap_or(false);
    if age >= 7 * 24 * 3600 && liquid {
        return Lifecycle::Mature;
    }
    if liquid {
        return Lifecycle::Liquid;
    }
    if age < 3600 {
        return Lifecycle::New;
    }
    if age < 24 * 3600 {
        return Lifecycle::Forming;
    }
    Lifecycle::Discovered
}

pub fn age_snapshot(
    first_observed_at: DateTime<Utc>,
    now: DateTime<Utc>,
    target_age_seconds: i64,
) -> AgeSnapshot {
    let actual_age = now
        .signed_duration_since(first_observed_at)
        .num_seconds()
        .max(0);
    AgeSnapshot {
        target_age_seconds,
        actual_observed_at: now,
        observation_delay_seconds: (actual_age - target_age_seconds).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn new_market_is_not_mature() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap();
        let now = t0 + chrono::Duration::seconds(30);
        let state = classify_lifecycle(&LifecycleInput {
            first_observed_at: t0,
            now,
            delisted: false,
            listing_known: true,
            day_notional_volume: OptionalNumber::observed(100.0),
            depth_25k_available: Some(false),
            spread: OptionalNumber::observed(0.02),
        });
        assert_eq!(state, Lifecycle::New);
    }

    #[test]
    fn collector_start_is_not_new_without_listing_time() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap();
        let now = t0 + chrono::Duration::seconds(30);
        let state = classify_lifecycle(&LifecycleInput {
            first_observed_at: t0,
            now,
            delisted: false,
            listing_known: false,
            day_notional_volume: OptionalNumber::observed(1_000_000.0),
            depth_25k_available: Some(true),
            spread: OptionalNumber::observed(0.0001),
        });
        assert_eq!(state, Lifecycle::Discovered);
    }

    #[test]
    fn age_snapshot_records_delay_instead_of_rewriting_t0() {
        let t0 = Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap();
        let now = t0 + chrono::Duration::seconds(45);
        let snap = age_snapshot(t0, now, 0);
        assert_eq!(snap.target_age_seconds, 0);
        assert_eq!(snap.observation_delay_seconds, 45);
        assert_eq!(snap.actual_observed_at, now);
    }
}
