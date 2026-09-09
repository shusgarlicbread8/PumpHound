//! PumpExecutionBuilder — constructs the exact Pump / PumpSwap instruction set a real executor
//! would submit, compiled into a legacy Solana message, for SIMULATION ONLY.
//!
//! * Account lists and discriminators come from the vendored IDLs (pump-public-docs @9c82f61).
//! * The fee payer is a SYNTHETIC identity with no private key anywhere in this repository.
//!   Nothing here can sign; nothing here can be submitted.
//! * The recent blockhash is a deterministic placeholder derived from the decision slot; the
//!   Surfpool/Mollusk harness substitutes a real one at verification time.

use crate::contract::TransactionKind;
use crate::fee_config::schedules;
use crate::pda::{self, associated_token_address, pk};
use pumphound_domain::{sha256_hex, Pubkey, Slot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const BUILDER_VERSION: &str = "pump-execution-builder-v1";

/// Deterministic, unsigned, never-funded identity used as fee payer in simulated messages.
pub fn synthetic_identity() -> Pubkey {
    let h = Sha256::digest(b"pumphound:paper-only:synthetic-execution-identity:v1");
    Pubkey(h.into())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMeta {
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
    pub role: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction {
    pub program_id: String,
    pub name: &'static str,
    pub accounts: Vec<AccountMeta>,
    pub data_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltTransaction {
    pub builder_version: &'static str,
    pub kind: TransactionKind,
    pub fee_payer: String,
    pub synthetic_identity: bool,
    pub recent_blockhash_placeholder: String,
    pub instructions: Vec<Instruction>,
    /// Legacy wire-format message bytes (no signatures).
    pub message_hex: String,
    pub message_sha256: String,
    pub creates_ata: bool,
    pub num_required_signatures: u8,
}

/// Identity facts the builder needs beyond the market snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketIdentity {
    pub mint: Pubkey,
    pub market: Pubkey,
    pub quote_mint: Pubkey,
    /// Curve creator (Pump) or pool coin_creator (PumpSwap). Required for the creator vault.
    pub creator: Option<Pubkey>,
    /// Token program owning the base mint (Token or Token-2022).
    pub base_token_program: Pubkey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapArgs {
    pub kind: TransactionKind,
    /// Buy: token amount (curve `buy`) or spendable quote (pumpswap exact-in). Sell: base in.
    pub amount: u64,
    /// Buy: max quote cost (curve) or min base out (pumpswap). Sell: min quote out.
    pub limit: u64,
    pub compute_unit_limit: u32,
    pub compute_unit_price_micro_lamports: u64,
    pub create_user_ata: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BuildError {
    #[error("creator unknown: cannot derive creator vault")]
    CreatorUnknown,
}

fn meta(pubkey: Pubkey, is_signer: bool, is_writable: bool, role: &'static str) -> AccountMeta {
    AccountMeta {
        pubkey: pubkey.to_base58(),
        is_signer,
        is_writable,
        role,
    }
}

fn compute_budget_ixs(limit: u32, price: u64) -> Vec<Instruction> {
    let cb = pda::COMPUTE_BUDGET_PROGRAM.to_string();
    let mut set_limit = vec![2u8];
    set_limit.extend_from_slice(&limit.to_le_bytes());
    let mut set_price = vec![3u8];
    set_price.extend_from_slice(&price.to_le_bytes());
    vec![
        Instruction {
            program_id: cb.clone(),
            name: "SetComputeUnitLimit",
            accounts: vec![],
            data_hex: hex(&set_limit),
        },
        Instruction {
            program_id: cb,
            name: "SetComputeUnitPrice",
            accounts: vec![],
            data_hex: hex(&set_price),
        },
    ]
}

fn create_ata_idempotent(
    payer: Pubkey,
    owner: Pubkey,
    mint: Pubkey,
    token_program: Pubkey,
) -> Instruction {
    let ata = associated_token_address(&owner, &mint, &token_program);
    Instruction {
        program_id: pda::ASSOCIATED_TOKEN_PROGRAM.into(),
        name: "CreateIdempotent",
        accounts: vec![
            meta(payer, true, true, "payer"),
            meta(ata, false, true, "associated_token_account"),
            meta(owner, false, false, "owner"),
            meta(mint, false, false, "mint"),
            meta(pk(pda::SYSTEM_PROGRAM), false, false, "system_program"),
            meta(token_program, false, false, "token_program"),
        ],
        data_hex: "01".into(),
    }
}

fn anchor_data(disc: [u8; 8], args: &[u64], track_volume: Option<bool>) -> Vec<u8> {
    let mut d = disc.to_vec();
    for a in args {
        d.extend_from_slice(&a.to_le_bytes());
    }
    // OptionBool: 0 = None, 1 = Some(false), 2 = Some(true)  (pump IDL `OptionBool`)
    if let Some(tv) = track_volume {
        d.push(if tv { 2 } else { 1 });
    }
    d
}

const PUMP_BUY: [u8; 8] = [0x66, 0x06, 0x3d, 0x12, 0x01, 0xda, 0xeb, 0xea];
const PUMP_SELL: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];
const AMM_BUY_EXACT_QUOTE_IN: [u8; 8] = [0xc6, 0x2e, 0x15, 0x52, 0xb4, 0xd9, 0xe8, 0x70];
const AMM_SELL: [u8; 8] = [0x33, 0xe6, 0x85, 0xa4, 0x01, 0x7f, 0x83, 0xad];

fn pump_swap_ix(
    user: Pubkey,
    id: &MarketIdentity,
    args: &SwapArgs,
) -> Result<Instruction, BuildError> {
    let creator = id.creator.ok_or(BuildError::CreatorUnknown)?;
    let sched = schedules();
    let program = pda::pump::program();
    let bonding_curve = id.market;
    let abc = associated_token_address(&bonding_curve, &id.mint, &id.base_token_program);
    let user_ata = associated_token_address(&user, &id.mint, &id.base_token_program);
    let fee_recipient = pk(&sched.pump_fee_recipient);
    let is_buy = matches!(args.kind, TransactionKind::PumpBuyExactSolIn);
    let mut accounts = vec![
        meta(pda::pump::global(), false, false, "global"),
        meta(fee_recipient, false, true, "fee_recipient"),
        meta(id.mint, false, false, "mint"),
        meta(bonding_curve, false, true, "bonding_curve"),
        meta(abc, false, true, "associated_bonding_curve"),
        meta(user_ata, false, true, "associated_user"),
        meta(user, true, true, "user"),
        meta(pk(pda::SYSTEM_PROGRAM), false, false, "system_program"),
    ];
    if is_buy {
        accounts.push(meta(id.base_token_program, false, false, "token_program"));
        accounts.push(meta(
            pda::pump::creator_vault(&creator),
            false,
            true,
            "creator_vault",
        ));
    } else {
        accounts.push(meta(
            pda::pump::creator_vault(&creator),
            false,
            true,
            "creator_vault",
        ));
        accounts.push(meta(id.base_token_program, false, false, "token_program"));
    }
    accounts.push(meta(
        pda::pump::event_authority(),
        false,
        false,
        "event_authority",
    ));
    accounts.push(meta(program, false, false, "program"));
    if is_buy {
        accounts.push(meta(
            pda::pump::global_volume_accumulator(),
            false,
            false,
            "global_volume_accumulator",
        ));
        accounts.push(meta(
            pda::pump::user_volume_accumulator(&user),
            false,
            true,
            "user_volume_accumulator",
        ));
    }
    accounts.push(meta(pda::pump::fee_config(), false, false, "fee_config"));
    accounts.push(meta(pk(pda::PUMP_FEE_PROGRAM), false, false, "fee_program"));
    let (name, data) = if is_buy {
        // buy(amount: tokens, max_sol_cost, track_volume)
        (
            "buy",
            anchor_data(PUMP_BUY, &[args.amount, args.limit], Some(false)),
        )
    } else {
        (
            "sell",
            anchor_data(PUMP_SELL, &[args.amount, args.limit], None),
        )
    };
    Ok(Instruction {
        program_id: program.to_base58(),
        name,
        accounts,
        data_hex: hex(&data),
    })
}

fn pump_amm_ix(
    user: Pubkey,
    id: &MarketIdentity,
    args: &SwapArgs,
) -> Result<Instruction, BuildError> {
    let coin_creator = id.creator.ok_or(BuildError::CreatorUnknown)?;
    let sched = schedules();
    let program = pda::pump_amm::program();
    let pool = id.market;
    let quote_token_program = pk(pda::TOKEN_PROGRAM);
    let protocol_fee_recipient = pk(sched
        .amm_protocol_fee_recipients
        .first()
        .expect("vendored AMM global config has fee recipients"));
    let vault_authority = pda::pump_amm::coin_creator_vault_authority(&coin_creator);
    let is_buy = matches!(args.kind, TransactionKind::PumpSwapBuyExactQuoteIn);
    let mut accounts = vec![
        meta(pool, false, true, "pool"),
        meta(user, true, true, "user"),
        meta(
            pda::pump_amm::global_config(),
            false,
            false,
            "global_config",
        ),
        meta(id.mint, false, false, "base_mint"),
        meta(id.quote_mint, false, false, "quote_mint"),
        meta(
            associated_token_address(&user, &id.mint, &id.base_token_program),
            false,
            true,
            "user_base_token_account",
        ),
        meta(
            associated_token_address(&user, &id.quote_mint, &quote_token_program),
            false,
            true,
            "user_quote_token_account",
        ),
        meta(
            associated_token_address(&pool, &id.mint, &id.base_token_program),
            false,
            true,
            "pool_base_token_account",
        ),
        meta(
            associated_token_address(&pool, &id.quote_mint, &quote_token_program),
            false,
            true,
            "pool_quote_token_account",
        ),
        meta(
            protocol_fee_recipient,
            false,
            false,
            "protocol_fee_recipient",
        ),
        meta(
            associated_token_address(
                &protocol_fee_recipient,
                &id.quote_mint,
                &quote_token_program,
            ),
            false,
            true,
            "protocol_fee_recipient_token_account",
        ),
        meta(id.base_token_program, false, false, "base_token_program"),
        meta(quote_token_program, false, false, "quote_token_program"),
        meta(pk(pda::SYSTEM_PROGRAM), false, false, "system_program"),
        meta(
            pk(pda::ASSOCIATED_TOKEN_PROGRAM),
            false,
            false,
            "associated_token_program",
        ),
        meta(
            pda::pump_amm::event_authority(),
            false,
            false,
            "event_authority",
        ),
        meta(program, false, false, "program"),
        meta(
            associated_token_address(&vault_authority, &id.quote_mint, &quote_token_program),
            false,
            true,
            "coin_creator_vault_ata",
        ),
        meta(
            vault_authority,
            false,
            false,
            "coin_creator_vault_authority",
        ),
    ];
    if is_buy {
        accounts.push(meta(
            pda::pump_amm::global_volume_accumulator(),
            false,
            false,
            "global_volume_accumulator",
        ));
        accounts.push(meta(
            pda::pump_amm::user_volume_accumulator(&user),
            false,
            true,
            "user_volume_accumulator",
        ));
    }
    accounts.push(meta(
        pda::pump_amm::fee_config(),
        false,
        false,
        "fee_config",
    ));
    accounts.push(meta(pk(pda::PUMP_FEE_PROGRAM), false, false, "fee_program"));
    let (name, data) = if is_buy {
        // buy_exact_quote_in(spendable_quote_in, min_base_amount_out, track_volume)
        (
            "buy_exact_quote_in",
            anchor_data(
                AMM_BUY_EXACT_QUOTE_IN,
                &[args.amount, args.limit],
                Some(false),
            ),
        )
    } else {
        (
            "sell",
            anchor_data(AMM_SELL, &[args.amount, args.limit], None),
        )
    };
    Ok(Instruction {
        program_id: program.to_base58(),
        name,
        accounts,
        data_hex: hex(&data),
    })
}

/// Build the full instruction list and compile a legacy message.
pub fn build(
    id: &MarketIdentity,
    args: &SwapArgs,
    decision_slot: Slot,
) -> Result<BuiltTransaction, BuildError> {
    let user = synthetic_identity();
    let mut ixs = compute_budget_ixs(
        args.compute_unit_limit,
        args.compute_unit_price_micro_lamports,
    );
    let is_pumpswap = matches!(
        args.kind,
        TransactionKind::PumpSwapBuyExactQuoteIn | TransactionKind::PumpSwapSell
    );
    if args.create_user_ata {
        ixs.push(create_ata_idempotent(
            user,
            user,
            id.mint,
            id.base_token_program,
        ));
    }
    if is_pumpswap {
        // Quote-side (WSOL) ATA is required by PumpSwap; idempotent create is cheap insurance.
        ixs.push(create_ata_idempotent(
            user,
            user,
            id.quote_mint,
            pk(pda::TOKEN_PROGRAM),
        ));
        ixs.push(pump_amm_ix(user, id, args)?);
    } else {
        ixs.push(pump_swap_ix(user, id, args)?);
    }
    let blockhash = Sha256::digest(
        format!("pumphound:synthetic-blockhash:slot:{}", decision_slot.0).as_bytes(),
    );
    let message = compile_legacy_message(user, &ixs, blockhash.into());
    Ok(BuiltTransaction {
        builder_version: BUILDER_VERSION,
        kind: args.kind,
        fee_payer: user.to_base58(),
        synthetic_identity: true,
        recent_blockhash_placeholder: bs58::encode(blockhash).into_string(),
        message_sha256: sha256_hex(&message),
        message_hex: hex(&message),
        instructions: ixs,
        creates_ata: args.create_user_ata,
        num_required_signatures: 1,
    })
}

/// Legacy message wire format: header, account keys, blockhash, compiled instructions.
fn compile_legacy_message(payer: Pubkey, ixs: &[Instruction], blockhash: [u8; 32]) -> Vec<u8> {
    #[derive(Default, Clone, Copy)]
    struct Flags {
        signer: bool,
        writable: bool,
    }
    let mut keys: Vec<(Pubkey, Flags)> = vec![(
        payer,
        Flags {
            signer: true,
            writable: true,
        },
    )];
    let mut upsert = |k: Pubkey, f: Flags| {
        if let Some(e) = keys.iter_mut().find(|(p, _)| *p == k) {
            e.1.signer |= f.signer;
            e.1.writable |= f.writable;
        } else {
            keys.push((k, f));
        }
    };
    for ix in ixs {
        for a in &ix.accounts {
            upsert(
                pk(&a.pubkey),
                Flags {
                    signer: a.is_signer,
                    writable: a.is_writable,
                },
            );
        }
        upsert(pk(&ix.program_id), Flags::default());
    }
    // Order: signer+writable, signer+readonly, nonsigner+writable, nonsigner+readonly; stable.
    let rank = |f: &Flags| match (f.signer, f.writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    };
    let mut ordered: Vec<(Pubkey, Flags)> = keys.clone();
    ordered.sort_by_key(|(_, f)| rank(f));
    let n_sig = ordered.iter().filter(|(_, f)| f.signer).count() as u8;
    let n_sig_ro = ordered
        .iter()
        .filter(|(_, f)| f.signer && !f.writable)
        .count() as u8;
    let n_ro = ordered
        .iter()
        .filter(|(_, f)| !f.signer && !f.writable)
        .count() as u8;
    let index_of = |k: &Pubkey| ordered.iter().position(|(p, _)| p == k).unwrap() as u8;
    let mut out = vec![n_sig, n_sig_ro, n_ro];
    shortvec(&mut out, ordered.len());
    for (k, _) in &ordered {
        out.extend_from_slice(&k.0);
    }
    out.extend_from_slice(&blockhash);
    shortvec(&mut out, ixs.len());
    for ix in ixs {
        out.push(index_of(&pk(&ix.program_id)));
        shortvec(&mut out, ix.accounts.len());
        for a in &ix.accounts {
            out.push(index_of(&pk(&a.pubkey)));
        }
        let data = unhex(&ix.data_hex);
        shortvec(&mut out, data.len());
        out.extend_from_slice(&data);
    }
    out
}

fn shortvec(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> MarketIdentity {
        let mint = pk("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        MarketIdentity {
            mint,
            market: pda::pump::bonding_curve(&mint),
            quote_mint: pk(pumphound_domain::WSOL_MINT),
            creator: Some(pk("62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV")),
            base_token_program: pk(pda::TOKEN_PROGRAM),
        }
    }

    fn args(kind: TransactionKind) -> SwapArgs {
        SwapArgs {
            kind,
            amount: 1_000_000,
            limit: 900_000,
            compute_unit_limit: 80_000,
            compute_unit_price_micro_lamports: 50_000,
            create_user_ata: true,
        }
    }

    #[test]
    fn pump_buy_matches_idl_account_layout() {
        let tx = build(
            &identity(),
            &args(TransactionKind::PumpBuyExactSolIn),
            Slot(1),
        )
        .unwrap();
        let swap = tx.instructions.last().unwrap();
        assert_eq!(swap.name, "buy");
        let roles: Vec<_> = swap.accounts.iter().map(|a| a.role).collect();
        assert_eq!(
            roles,
            vec![
                "global",
                "fee_recipient",
                "mint",
                "bonding_curve",
                "associated_bonding_curve",
                "associated_user",
                "user",
                "system_program",
                "token_program",
                "creator_vault",
                "event_authority",
                "program",
                "global_volume_accumulator",
                "user_volume_accumulator",
                "fee_config",
                "fee_program"
            ]
        );
        assert!(swap.data_hex.starts_with("66063d1201daebea"));
        assert_eq!(tx.num_required_signatures, 1);
        assert!(tx.synthetic_identity);
        assert_eq!(tx.instructions[0].name, "SetComputeUnitLimit");
    }

    #[test]
    fn pump_sell_has_creator_vault_before_token_program() {
        let tx = build(&identity(), &args(TransactionKind::PumpSell), Slot(1)).unwrap();
        let swap = tx.instructions.last().unwrap();
        let roles: Vec<_> = swap.accounts.iter().map(|a| a.role).collect();
        assert_eq!(roles[8], "creator_vault");
        assert_eq!(roles[9], "token_program");
        assert_eq!(roles.len(), 14);
        assert!(swap.data_hex.starts_with("33e685a4017f83ad"));
    }

    #[test]
    fn pumpswap_buy_exact_quote_in_layout() {
        let mut id = identity();
        id.market = pda::pump_amm::canonical_pool(&id.mint, &id.quote_mint);
        let tx = build(
            &id,
            &args(TransactionKind::PumpSwapBuyExactQuoteIn),
            Slot(1),
        )
        .unwrap();
        let swap = tx.instructions.last().unwrap();
        assert_eq!(swap.name, "buy_exact_quote_in");
        assert_eq!(swap.accounts.len(), 23);
        assert!(swap.data_hex.starts_with("c62e1552b4d9e870"));
        let sell = build(&id, &args(TransactionKind::PumpSwapSell), Slot(1)).unwrap();
        assert_eq!(sell.instructions.last().unwrap().accounts.len(), 21);
    }

    #[test]
    fn message_is_deterministic_and_decodable_header() {
        let a = build(
            &identity(),
            &args(TransactionKind::PumpBuyExactSolIn),
            Slot(7),
        )
        .unwrap();
        let b = build(
            &identity(),
            &args(TransactionKind::PumpBuyExactSolIn),
            Slot(7),
        )
        .unwrap();
        assert_eq!(a.message_sha256, b.message_sha256);
        let c = build(
            &identity(),
            &args(TransactionKind::PumpBuyExactSolIn),
            Slot(8),
        )
        .unwrap();
        assert_ne!(
            a.message_sha256, c.message_sha256,
            "blockhash placeholder must vary by slot"
        );
        let bytes = unhex(&a.message_hex);
        assert_eq!(bytes[0], 1, "exactly one required signature");
        assert_eq!(bytes[1], 0);
    }

    #[test]
    fn unknown_creator_refuses_to_construct() {
        let mut id = identity();
        id.creator = None;
        assert_eq!(
            build(&id, &args(TransactionKind::PumpSell), Slot(1)),
            Err(BuildError::CreatorUnknown)
        );
    }
}
