use crate::{Canonicality, Commitment, Slot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Timestamp = DateTime<Utc>;

pub fn now_utc() -> Timestamp {
    Utc::now()
}

pub fn format_rfc3339_micros(ts: Timestamp) -> String {
    ts.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationPoint {
    pub slot: Slot,
    pub tx_index: Option<u32>,
    pub event_index: Option<u32>,
    pub event_time: Option<Timestamp>,
    pub observed_at: Timestamp,
    pub knowledge_cutoff: Timestamp,
    pub commitment: Commitment,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub source: String,
    pub source_record_id: String,
    pub payload_sha256: String,
    pub decoder_version: String,
    pub idl_sha256: Option<String>,
    pub raw_partition_uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeContext {
    pub slot: Slot,
    pub block_time: Option<Timestamp>,
    pub observed_at: Timestamp,
    pub computed_at: Timestamp,
    pub available_from_slot: Slot,
    pub knowledge_cutoff: Timestamp,
    pub commitment: Commitment,
    pub canonicality: Canonicality,
}

/// Pump.fun mainnet era starts in January 2024. Timestamps before this, or more
/// than two hours in the future, are not usable as chain time.
pub const PUMP_ERA_START_UNIX: i64 = 1_704_067_200;

pub fn is_sane_chain_unix(ts: i64) -> bool {
    let max = Utc::now().timestamp().saturating_add(7_200);
    ts >= PUMP_ERA_START_UNIX && ts <= max
}

/// Prefer a sane RPC `blockTime`; otherwise a sane program-event unix timestamp.
/// Impossible values (year-2299, pre-Pump era, clock-skewed futures) are dropped
/// rather than stored as launch or event time.
pub fn chain_time_from_parts(
    block_time: Option<DateTime<Utc>>,
    event_unix: Option<i64>,
) -> Option<DateTime<Utc>> {
    if let Some(block_time) = block_time.filter(|time| is_sane_chain_unix(time.timestamp())) {
        return Some(block_time);
    }
    event_unix
        .filter(|ts| is_sane_chain_unix(*ts))
        .and_then(|ts| DateTime::from_timestamp(ts, 0))
}

/// Linear interpolation between two slot/time anchors. Used only to recover
/// missing `creation_time` when a nearby launch already has chain time.
pub fn interpolate_slot_time(
    slot: i64,
    left_slot: i64,
    left_time: DateTime<Utc>,
    right_slot: i64,
    right_time: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if left_slot == right_slot {
        return chain_time_from_parts(Some(left_time), None);
    }
    let span = right_slot.checked_sub(left_slot)?;
    let delta = slot.checked_sub(left_slot)?;
    let left_ts = left_time.timestamp();
    let right_ts = right_time.timestamp();
    let interpolated = left_ts.saturating_add(
        ((right_ts.saturating_sub(left_ts) as i128) * delta as i128 / span as i128) as i64,
    );
    chain_time_from_parts(DateTime::from_timestamp(interpolated, 0), None)
}

/// Filter predicate for bitemporal historical reads.
pub fn visible_at(
    event_slot: Slot,
    cutoff_slot: Slot,
    known_at: Timestamp,
    knowledge_cutoff: Timestamp,
    superseded_at: Option<Timestamp>,
) -> bool {
    event_slot.0 <= cutoff_slot.0
        && known_at <= knowledge_cutoff
        && superseded_at.map(|s| s > knowledge_cutoff).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_year_2299_and_pre_pump_timestamps() {
        assert!(!is_sane_chain_unix(10_409_356_800)); // 2299-01-01
        assert!(!is_sane_chain_unix(0));
        assert!(is_sane_chain_unix(1_720_000_000)); // 2024-07
        assert!(
            chain_time_from_parts(
                DateTime::from_timestamp(10_409_356_800, 0),
                Some(1_720_000_000)
            )
            .unwrap()
            .timestamp()
                == 1_720_000_000
        );
    }

    #[test]
    fn interpolates_between_nearby_slots() {
        let left = DateTime::from_timestamp(1_720_000_000, 0).unwrap();
        let right = DateTime::from_timestamp(1_720_000_400, 0).unwrap();
        let mid = interpolate_slot_time(150, 100, left, 200, right).unwrap();
        assert_eq!(mid.timestamp(), 1_720_000_200);
    }
}
