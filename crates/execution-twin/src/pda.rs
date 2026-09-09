//! Program-derived addresses without pulling the Solana SDK into the workspace.
//!
//! `find_program_address` follows the runtime exactly: sha256(seeds ‖ program_id ‖
//! "ProgramDerivedAddress") must NOT be a valid Ed25519 point. Verified in tests against the
//! well-known Pump `global` and `__event_authority` PDAs.

use curve25519_dalek::edwards::CompressedEdwardsY;
use pumphound_domain::Pubkey;
use sha2::{Digest, Sha256};

pub const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const PUMP_AMM_PROGRAM: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
pub const PUMP_FEE_PROGRAM: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
pub const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";
/// Canonical PumpSwap pool index used by `canonicalPumpPoolPda` in the official SDK.
pub const CANONICAL_POOL_INDEX: u16 = 0;

pub fn pk(s: &str) -> Pubkey {
    Pubkey::from_base58(s).expect("static pubkey")
}

fn is_on_curve(bytes: &[u8; 32]) -> bool {
    CompressedEdwardsY(*bytes).decompress().is_some()
}

pub fn create_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> Option<Pubkey> {
    if seeds.len() > 16 || seeds.iter().any(|s| s.len() > 32) {
        return None;
    }
    let mut hasher = Sha256::new();
    for seed in seeds {
        hasher.update(seed);
    }
    hasher.update(program_id.0);
    hasher.update(b"ProgramDerivedAddress");
    let hash: [u8; 32] = hasher.finalize().into();
    if is_on_curve(&hash) {
        None
    } else {
        Some(Pubkey(hash))
    }
}

pub fn find_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> (Pubkey, u8) {
    for bump in (0..=255u8).rev() {
        let bump_seed = [bump];
        let mut all: Vec<&[u8]> = seeds.to_vec();
        all.push(&bump_seed);
        if let Some(address) = create_program_address(&all, program_id) {
            return (address, bump);
        }
    }
    unreachable!("no viable bump seed (probability ~2^-256)")
}

pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    find_program_address(
        &[&owner.0, &token_program.0, &mint.0],
        &pk(ASSOCIATED_TOKEN_PROGRAM),
    )
    .0
}

pub mod pump {
    use super::*;
    pub fn program() -> Pubkey {
        pk(PUMP_PROGRAM)
    }
    pub fn global() -> Pubkey {
        find_program_address(&[b"global"], &program()).0
    }
    pub fn event_authority() -> Pubkey {
        find_program_address(&[b"__event_authority"], &program()).0
    }
    pub fn bonding_curve(mint: &Pubkey) -> Pubkey {
        find_program_address(&[b"bonding-curve", &mint.0], &program()).0
    }
    pub fn creator_vault(creator: &Pubkey) -> Pubkey {
        find_program_address(&[b"creator-vault", &creator.0], &program()).0
    }
    pub fn global_volume_accumulator() -> Pubkey {
        find_program_address(&[b"global_volume_accumulator"], &program()).0
    }
    pub fn user_volume_accumulator(user: &Pubkey) -> Pubkey {
        find_program_address(&[b"user_volume_accumulator", &user.0], &program()).0
    }
    pub fn pool_authority(mint: &Pubkey) -> Pubkey {
        find_program_address(&[b"pool-authority", &mint.0], &program()).0
    }
    pub fn fee_config() -> Pubkey {
        find_program_address(&[b"fee_config", &program().0], &pk(PUMP_FEE_PROGRAM)).0
    }
}

pub mod pump_amm {
    use super::*;
    pub fn program() -> Pubkey {
        pk(PUMP_AMM_PROGRAM)
    }
    pub fn global_config() -> Pubkey {
        find_program_address(&[b"global_config"], &program()).0
    }
    pub fn event_authority() -> Pubkey {
        find_program_address(&[b"__event_authority"], &program()).0
    }
    pub fn global_volume_accumulator() -> Pubkey {
        find_program_address(&[b"global_volume_accumulator"], &program()).0
    }
    pub fn user_volume_accumulator(user: &Pubkey) -> Pubkey {
        find_program_address(&[b"user_volume_accumulator", &user.0], &program()).0
    }
    pub fn coin_creator_vault_authority(coin_creator: &Pubkey) -> Pubkey {
        find_program_address(&[b"creator_vault", &coin_creator.0], &program()).0
    }
    pub fn fee_config() -> Pubkey {
        find_program_address(&[b"fee_config", &program().0], &pk(PUMP_FEE_PROGRAM)).0
    }
    pub fn pool(index: u16, owner: &Pubkey, base_mint: &Pubkey, quote_mint: &Pubkey) -> Pubkey {
        find_program_address(
            &[
                b"pool",
                &index.to_le_bytes(),
                &owner.0,
                &base_mint.0,
                &quote_mint.0,
            ],
            &program(),
        )
        .0
    }
    /// The canonical (graduated-from-Pump) pool for a mint.
    pub fn canonical_pool(mint: &Pubkey, quote_mint: &Pubkey) -> Pubkey {
        pool(
            CANONICAL_POOL_INDEX,
            &pump::pool_authority(mint),
            mint,
            quote_mint,
        )
    }
    pub fn is_pump_pool(base_mint: &Pubkey, pool_creator: &Pubkey) -> bool {
        pump::pool_authority(base_mint) == *pool_creator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pump_global_pda_matches_mainnet_constant() {
        assert_eq!(
            pump::global().to_base58(),
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"
        );
    }

    #[test]
    fn pump_event_authority_matches_mainnet_constant() {
        assert_eq!(
            pump::event_authority().to_base58(),
            "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"
        );
    }

    #[test]
    fn pump_amm_global_config_matches_mainnet_constant() {
        assert_eq!(
            pump_amm::global_config().to_base58(),
            "ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw"
        );
    }

    #[test]
    fn derived_addresses_are_off_curve() {
        let (addr, _) = find_program_address(&[b"anything"], &pump::program());
        assert!(!is_on_curve(&addr.0));
    }
}
