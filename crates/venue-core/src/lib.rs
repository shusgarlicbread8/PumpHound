//! Venue-neutral research contracts. Market-specific fields stay in venue crates.

mod health;
mod instrument;
mod observation;

pub use health::{DataHealth, HealthState, ProviderHealth};
pub use instrument::{InstrumentId, InstrumentKind, Venue};
pub use observation::{
    MarketObservationMeta, ObservationId, ObservationTime, OptionalNumber, ResearchStatus,
};

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn hyperliquid_instrument_has_no_pump_fields() {
        let id = InstrumentId::new(Venue::Hyperliquid, "xyz:NVDA");
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json["venue"], "hyperliquid");
        assert_eq!(json["native_id"], "xyz:NVDA");
        assert!(json.get("mint").is_none());
        assert!(json.get("creator").is_none());
        assert!(json.get("bonding_curve").is_none());
    }

    #[test]
    fn unknown_numeric_value_is_not_zero() {
        let mut obs = MarketObservationMeta::new(Venue::Hyperliquid, "xyz:NVDA", Some(10));
        assert_eq!(obs.sequence, Some(10));
        assert!(obs.source_timestamp.is_none());
        assert!(obs.mid.is_unknown());
        let encoded = serde_json::to_value(&obs).unwrap();
        assert!(encoded["mid"].is_null());
        obs.mid = OptionalNumber::observed(0.0);
        assert_eq!(obs.mid.as_f64(), Some(0.0));
    }

    #[test]
    fn health_states_cover_degraded_and_missing_key() {
        for state in [
            HealthState::Healthy,
            HealthState::Stale,
            HealthState::Degraded,
            HealthState::DisabledMissingKey,
            HealthState::Unsupported,
            HealthState::Unknown,
        ] {
            let text = serde_json::to_string(&state).unwrap();
            let back: HealthState = serde_json::from_str(&text).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn observation_time_keeps_source_and_received_separate() {
        let source = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 1).unwrap();
        let received = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 3).unwrap();
        let time = ObservationTime {
            source_ts: Some(source),
            received_ts: received,
        };
        assert_eq!(time.receive_delay_ms(), Some(2000));
        assert_ne!(time.source_ts, Some(time.received_ts));
    }
}
