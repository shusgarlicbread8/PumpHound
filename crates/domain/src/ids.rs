use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Immutable economic identity: one instruction event, one row.
pub fn economic_event_key(
    signature: &str,
    instruction_index: u32,
    inner_instruction_index: u32,
    event_kind: &str,
) -> String {
    format!("{signature}:{instruction_index}:{inner_instruction_index}:{event_kind}")
}

/// event_id = sha256(chain + signature + program_id + instruction_path + event_index)
pub fn event_id(
    chain: &str,
    signature: &str,
    program_id: &str,
    instruction_path: &[u32],
    event_index: u32,
) -> String {
    let path = instruction_path
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("/");
    sha256_hex(format!("{chain}|{signature}|{program_id}|{path}|{event_index}").as_bytes())
}

/// sighting_id = sha256(source + source_session + source_cursor + raw_payload_hash)
pub fn sighting_id(
    source: &str,
    source_session: &str,
    source_cursor: &str,
    payload_sha256: &str,
) -> String {
    sha256_hex(
        format!("{source}|{source_session}|{source_session}|{source_cursor}|{payload_sha256}")
            .as_bytes(),
    )
}

pub fn payload_sha256(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn economic_key_is_stable_and_unique() {
        let a = economic_event_key("sig", 0, 0, "BUY");
        let b = economic_event_key("sig", 0, 0, "BUY");
        let c = economic_event_key("sig", 1, 0, "BUY");
        assert_eq!(a, b);
        assert_eq!(a, "sig:0:0:BUY");
        assert_ne!(a, c);
    }
}
