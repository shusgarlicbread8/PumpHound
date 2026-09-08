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
