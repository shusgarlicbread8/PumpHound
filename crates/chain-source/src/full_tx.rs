use base64::Engine;
use pumphound_protocol_pump::{CREATE_DISCRIMINATOR, MIGRATE_DISCRIMINATOR};
use pumphound_protocol_pumpswap::CREATE_POOL_DISCRIMINATOR;

pub fn program_data_discriminator(line: &str) -> Option<[u8; 8]> {
    let b64 = line.strip_prefix("Program data: ")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&bytes[..8]);
    Some(disc)
}

/// Create / create-pool / migrate are low-volume and carry identity plus
/// chain time. Always fetch the full transaction for those log lines even
/// when the broad scout is logs-only.
pub fn logs_warrant_full_transaction(logs: &[String]) -> bool {
    logs.iter().any(|line| {
        program_data_discriminator(line).is_some_and(|disc| {
            disc == CREATE_DISCRIMINATOR
                || disc == MIGRATE_DISCRIMINATOR
                || disc == CREATE_POOL_DISCRIMINATOR
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumphound_domain::PUMP_PROGRAM_ID;

    #[test]
    fn create_log_warrants_a_full_transaction() {
        let line = format!(
            "Program data: {}",
            base64::engine::general_purpose::STANDARD.encode(CREATE_DISCRIMINATOR)
        );
        assert!(logs_warrant_full_transaction(&[
            format!("Program {PUMP_PROGRAM_ID} invoke [1]"),
            line,
        ]));
        assert!(!logs_warrant_full_transaction(&[
            "Program data: AAAAAAAAAAA=".into()
        ]));
    }
}
