use crate::cursor::BorshCursor;
use crate::{DECODER_VERSION, IDL_SHA256, PROGRAM_ID};
use pumphound_domain::{
    event_id, AssetAmount, Atomic, Canonicality, ChainEvent, ChainEventPayload, Commitment,
    DecodeError, DecodeQuarantined, EventAction, EventEnvelope, FeeComponent, FeeKind,
    LaunchCompleted, LaunchCreated, LaunchMigrated, LaunchpadDecoder, Protocol, Provenance, Pubkey,
    RawTransaction, TradeExecuted, TradeSide, TransferObserved, TransferRole, CHAIN,
    SCHEMA_VERSION,
};
use serde_json::json;
use uuid::Uuid;

pub const CREATE_DISCRIMINATOR: [u8; 8] = [27, 114, 169, 77, 222, 235, 99, 118];
const CREATE: [u8; 8] = CREATE_DISCRIMINATOR;
const TRADE: [u8; 8] = [189, 219, 127, 211, 78, 230, 97, 238];
const COMPLETE: [u8; 8] = [95, 114, 97, 156, 212, 46, 152, 8];
pub const MIGRATE_DISCRIMINATOR: [u8; 8] = [189, 233, 93, 185, 92, 148, 234, 148];
const MIGRATE: [u8; 8] = MIGRATE_DISCRIMINATOR;
// Non-market events from official @pump-fun/pump-sdk 1.36.0. They remain in
// raw sightings but do not belong in canonical market or decoder-failure data.
const NON_MARKET_EVENTS: [[u8; 8]; 19] = [
    [64, 69, 192, 104, 29, 30, 25, 107],      // AdminSetCreatorEvent
    [245, 59, 70, 34, 75, 185, 109, 92],      // AdminSetIdlAuthorityEvent
    [147, 250, 108, 120, 247, 29, 67, 222],   // AdminUpdateTokenIncentivesEvent
    [226, 214, 246, 33, 7, 242, 147, 229],    // ClaimCashbackEvent
    [79, 172, 246, 49, 205, 91, 206, 232],    // ClaimTokenIncentivesEvent
    [146, 159, 189, 172, 146, 88, 56, 244],   // CloseUserVolumeAccumulatorEvent
    [122, 2, 127, 1, 14, 191, 12, 175],       // CollectCreatorFeeEvent
    [165, 55, 129, 112, 4, 179, 202, 40],     // DistributeCreatorFeesEvent
    [97, 97, 215, 144, 93, 146, 22, 124],     // ExtendAccountEvent
    [134, 36, 13, 72, 232, 101, 130, 216],    // InitUserVolumeAccumulatorEvent
    [155, 167, 104, 220, 213, 108, 243, 3],   // MigrateBondingCurveCreatorEvent
    [168, 216, 132, 239, 235, 182, 49, 52],   // MinimumDistributableFeeEvent
    [43, 188, 250, 18, 221, 75, 187, 95],     // ReservedFeeRecipientsEvent
    [237, 52, 123, 37, 245, 251, 72, 210],    // SetCreatorEvent
    [142, 203, 6, 32, 127, 105, 191, 162],    // SetMetaplexCreatorEvent
    [223, 195, 159, 246, 62, 48, 143, 131],   // SetParamsEvent
    [197, 122, 167, 124, 116, 81, 91, 255],   // SyncUserVolumeAccumulatorEvent
    [182, 195, 137, 42, 35, 206, 207, 247],   // UpdateGlobalAuthorityEvent
    [117, 123, 228, 182, 161, 168, 220, 214], // UpdateMayhemVirtualParamsEvent
];

pub struct PumpBondingCurveDecoder {
    program_ids: Vec<String>,
}

impl Default for PumpBondingCurveDecoder {
    fn default() -> Self {
        Self {
            program_ids: vec![PROGRAM_ID.to_string()],
        }
    }
}

impl LaunchpadDecoder for PumpBondingCurveDecoder {
    fn protocol(&self) -> Protocol {
        Protocol::PumpCurve
    }

    fn program_ids(&self) -> &[String] {
        &self.program_ids
    }

    fn decode(&self, tx: &RawTransaction) -> Result<Vec<ChainEvent>, DecodeError> {
        if !tx.account_keys.iter().any(|k| k == PROGRAM_ID)
            && !tx.instructions.iter().any(|i| i.program_id == PROGRAM_ID)
        {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for (event_index, (disc, payload)) in
            collect_program_data_logs(&tx.log_messages, PROGRAM_ID)
                .into_iter()
                .enumerate()
        {
            let event_index = event_index as u32;
            match decode_one(tx, disc, &payload, event_index) {
                Ok(Some(ev)) => out.push(ev),
                Ok(None) => {}
                Err(DecodeError::UnknownDiscriminator(hex)) => {
                    out.push(quarantine(
                        tx,
                        event_index,
                        &hex,
                        &payload,
                        "unknown discriminator; raw capture retained".into(),
                    ));
                }
                Err(error) => {
                    out.push(quarantine(
                        tx,
                        event_index,
                        &hex::encode(disc),
                        &payload,
                        format!(
                            "known discriminator with unsupported or malformed {}-byte payload: {error}",
                            payload.len()
                        ),
                    ));
                }
            }
        }
        Ok(out)
    }
}

fn collect_program_data_logs(logs: &[String], program: &str) -> Vec<([u8; 8], Vec<u8>)> {
    let mut stack: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for line in logs {
        if let Some(rest) = line.strip_prefix("Program ") {
            if rest.contains(" invoke [") {
                if let Some(id) = rest.split_whitespace().next() {
                    stack.push(id.to_string());
                }
            } else if rest.contains(" success") || rest.contains(" failed") {
                let _ = stack.pop();
            }
        }
        if let Some(b64) = line.strip_prefix("Program data: ") {
            if stack.last().map(|s| s.as_str()) != Some(program) {
                continue;
            }
            if let Ok(bytes) =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64.trim())
            {
                if bytes.len() >= 8 {
                    let mut disc = [0u8; 8];
                    disc.copy_from_slice(&bytes[..8]);
                    out.push((disc, bytes[8..].to_vec()));
                }
            }
        }
    }
    out
}

fn decode_one(
    tx: &RawTransaction,
    disc: [u8; 8],
    payload: &[u8],
    event_index: u32,
) -> Result<Option<ChainEvent>, DecodeError> {
    let succeeded = tx.err.is_none();
    match disc {
        CREATE => decode_create(tx, payload, event_index).map(Some),
        TRADE => {
            if !succeeded {
                return Ok(None);
            }
            decode_trade(tx, payload, event_index).map(Some)
        }
        COMPLETE => {
            if !succeeded {
                return Ok(None);
            }
            let mut c = BorshCursor::new(payload);
            let _user = c.pubkey()?;
            let mint = c.pubkey()?;
            let bonding_curve = c.pubkey()?;
            let ts = c.i64()?;
            match c.remaining() {
                0 => {}
                32 => {
                    let _quote = c.pubkey()?;
                }
                remaining => {
                    return Err(DecodeError::IncompatibleLayout(format!(
                        "complete event has {remaining} trailing bytes"
                    )));
                }
            }
            Ok(Some(wrap_at(
                tx,
                event_index,
                ChainEventPayload::LaunchCompleted(LaunchCompleted {
                    launch_id: String::new(),
                    mint,
                    market: bonding_curve,
                }),
                Some(ts),
            )))
        }
        MIGRATE => {
            if !succeeded {
                return Ok(None);
            }
            let mut c = BorshCursor::new(payload);
            let _user = c.pubkey()?;
            let mint = c.pubkey()?;
            let _mint_amount = c.u64()?;
            let _sol_amount = c.u64()?;
            let _fee = c.u64()?;
            let bonding_curve = c.pubkey()?;
            let ts = c.i64()?;
            let pool = c.pubkey()?;
            let quote_mint = normalize_quote_mint(c.pubkey()?);
            require_consumed(&c, "migration event")?;
            Ok(Some(wrap_at(
                tx,
                event_index,
                ChainEventPayload::LaunchMigrated(LaunchMigrated {
                    launch_id: String::new(),
                    mint,
                    old_market: bonding_curve,
                    new_market: pool,
                    quote_mint,
                }),
                Some(ts),
            )))
        }
        other if NON_MARKET_EVENTS.contains(&other) => Ok(None),
        other => Err(DecodeError::UnknownDiscriminator(hex::encode(other))),
    }
}

fn decode_create(
    tx: &RawTransaction,
    payload: &[u8],
    event_index: u32,
) -> Result<ChainEvent, DecodeError> {
    let mut c = BorshCursor::new(payload);
    let name = c.string()?;
    let symbol = c.string()?;
    let uri = c.string()?;
    let mint = c.pubkey()?;
    let bonding_curve = c.pubkey()?;
    let user = c.pubkey()?;

    let mut event_ts = None;
    let created = if c.remaining() == 0 {
        let quote_mint =
            Pubkey::from_base58(pumphound_domain::WSOL_MINT).expect("static WSOL mint");
        LaunchCreated {
            launch_id: deterministic_launch_id(tx, event_index),
            mint,
            // In the original create instruction the `user` signer created and owned the coin.
            creator: Some(user),
            market: bonding_curve,
            quote_mint,
            base_decimals: 6,
            quote_decimals: 9,
            supply_atomic: None,
            metadata_uri: Some(uri),
            name: Some(name),
            symbol: Some(symbol),
            protocol_flags: json!({
                "event_layout": "pump_create_v1_legacy",
                "creator_source": "user_signer_protocol_semantics",
                "initial_reserves_present": false,
                "user": user.to_string(),
            }),
            virtual_base_atomic: None,
            virtual_quote_atomic: None,
            real_base_atomic: None,
        }
    } else {
        let creator = c.pubkey()?;
        let ts = c.i64()?;
        event_ts = Some(ts);
        let virtual_token = c.atomic_u64()?;
        let virtual_sol = c.atomic_u64()?;
        let real_token = c.atomic_u64()?;
        let supply = c.atomic_u64()?;
        let token_program = c.pubkey()?;
        let is_mayhem = c.bool()?;
        let is_cashback = c.bool()?;
        let quote_mint = normalize_quote_mint(c.pubkey()?);
        let virtual_quote = c.atomic_u64()?;
        require_consumed(&c, "create event")?;
        LaunchCreated {
            launch_id: Uuid::now_v7().to_string(),
            mint,
            creator: Some(creator),
            market: bonding_curve,
            quote_mint,
            base_decimals: 6,
            quote_decimals: quote_decimals(&quote_mint),
            supply_atomic: Some(supply),
            metadata_uri: Some(uri),
            name: Some(name),
            symbol: Some(symbol),
            protocol_flags: json!({
                "is_mayhem_mode": is_mayhem,
                "is_cashback_enabled": is_cashback,
                "token_program": token_program.to_string(),
                "user": user.to_string(),
                "event_timestamp": ts,
            }),
            virtual_base_atomic: Some(virtual_token),
            virtual_quote_atomic: Some(if virtual_quote.is_zero() {
                virtual_sol
            } else {
                virtual_quote
            }),
            real_base_atomic: Some(real_token),
        }
    };
    Ok(wrap_at(
        tx,
        event_index,
        ChainEventPayload::LaunchCreated(created),
        event_ts,
    ))
}

fn decode_trade(
    tx: &RawTransaction,
    payload: &[u8],
    event_index: u32,
) -> Result<ChainEvent, DecodeError> {
    let mut c = BorshCursor::new(payload);
    let mint = c.pubkey()?;
    let sol_amount = c.atomic_u64()?;
    let token_amount = c.atomic_u64()?;
    let is_buy = c.bool()?;
    let user = c.pubkey()?;
    let timestamp = c.i64()?;
    let v_sol = c.atomic_u64()?;
    let v_tok = c.atomic_u64()?;

    if payload.len() == 105 || payload.len() == 121 {
        let (r_sol, r_tok, layout, missing_fields) = if payload.len() == 121 {
            (
                Some(c.atomic_u64()?),
                Some(c.atomic_u64()?),
                "pump_trade_v2_real_reserves",
                vec!["fees", "creator"],
            )
        } else {
            (
                None,
                None,
                "pump_trade_v1_legacy",
                vec![
                    "fees",
                    "creator",
                    "real_sol_reserves",
                    "real_token_reserves",
                ],
            )
        };
        require_consumed(&c, "legacy trade event")?;
        let quote_mint =
            Pubkey::from_base58(pumphound_domain::WSOL_MINT).expect("static WSOL mint");
        let market = market_for_mint(tx, &mint).unwrap_or(mint);
        return Ok(wrap_at(
            tx,
            event_index,
            ChainEventPayload::TradeExecuted(TradeExecuted {
                launch_id: None,
                mint,
                market,
                trader: user,
                fee_payer: fee_payer(tx, user),
                side: side(is_buy),
                base_amount_atomic: token_amount,
                quote_amount_atomic: sol_amount,
                quote_mint,
                fee_components: vec![],
                reserve_snapshot_id: None,
                instruction_name: instruction_name(is_buy).into(),
                virtual_base_atomic: Some(v_tok),
                virtual_quote_atomic: Some(v_sol),
                real_base_atomic: r_tok,
                real_quote_atomic: r_sol,
                raw_event: json!({
                    "event_layout": layout,
                    "timestamp": timestamp,
                    "missing_fields": missing_fields,
                    "executable_quote_supported": false,
                    "market_source": if market == mint {
                        "unavailable_fallback_mint"
                    } else {
                        "instruction_accounts"
                    },
                }),
                protocol_fee_bps: None,
                creator_fee_bps: None,
                lp_fee_bps: None,
                is_mayhem_mode: None,
                creator_fee_applies: None,
                base_supply_atomic: None,
                cashback_fee_bps: None,
            }),
            Some(timestamp),
        ));
    }

    let r_sol = c.atomic_u64()?;
    let r_tok = c.atomic_u64()?;
    let _fee_recipient = c.pubkey()?;
    let fee_bps = c.u64()?;
    let fee = c.atomic_u64()?;
    let creator = c.pubkey()?;
    let creator_fee_bps = c.u64()?;
    let creator_fee = c.atomic_u64()?;
    let _track = c.bool()?;
    let _ = c.u64()?;
    let _ = c.u64()?;
    let _ = c.u64()?;
    let _ = c.i64()?;
    let ix_name = c.string()?;
    let mayhem = c.bool()?;
    let cashback_bps = c.u64()?;
    let cashback = c.atomic_u64()?;
    let _ = c.u64()?;
    let buyback = c.atomic_u64()?;
    let _ = c.skip_vec_pubkey_u16()?;
    let quote_mint = normalize_quote_mint(c.pubkey()?);
    let quote_amount = c.atomic_u64()?;
    let v_quote = c.atomic_u64()?;
    let r_quote = c.atomic_u64()?;
    require_consumed(&c, "current trade event")?;
    let quote = if quote_amount.is_zero() {
        sol_amount
    } else {
        quote_amount
    };
    let qdec = quote_decimals(&quote_mint);
    Ok(wrap_at(
        tx,
        event_index,
        ChainEventPayload::TradeExecuted(TradeExecuted {
            launch_id: None,
            mint,
            market: mint,
            trader: user,
            fee_payer: fee_payer(tx, user),
            side: side(is_buy),
            base_amount_atomic: token_amount,
            quote_amount_atomic: quote,
            quote_mint,
            fee_components: vec![
                fee_comp(FeeKind::Protocol, quote_mint, fee, qdec),
                fee_comp(FeeKind::Creator, quote_mint, creator_fee, qdec),
                fee_comp(FeeKind::Cashback, quote_mint, cashback, qdec),
                fee_comp(FeeKind::Buyback, quote_mint, buyback, qdec),
            ],
            reserve_snapshot_id: None,
            instruction_name: ix_name,
            virtual_base_atomic: Some(v_tok),
            virtual_quote_atomic: Some(if v_quote.is_zero() { v_sol } else { v_quote }),
            real_base_atomic: Some(r_tok),
            real_quote_atomic: Some(if r_quote.is_zero() { r_sol } else { r_quote }),
            raw_event: json!({
                "creator": trade_creator_or_none(creator),
                "fee_basis_points": fee_bps,
                "creator_fee_basis_points": creator_fee_bps,
                "mayhem_mode": mayhem,
                "cashback_fee_basis_points": cashback_bps,
            }),
            protocol_fee_bps: Some(fee_bps),
            creator_fee_bps: Some(creator_fee_bps),
            lp_fee_bps: None,
            is_mayhem_mode: Some(mayhem),
            creator_fee_applies: Some(creator != Pubkey(DEFAULT_PUBKEY)),
            base_supply_atomic: None,
            cashback_fee_bps: Some(cashback_bps),
        }),
        Some(timestamp),
    ))
}

/// `Pubkey::default()` — the all-zero key Pump uses to mean "no creator".
const DEFAULT_PUBKEY: [u8; 32] = [0u8; 32];

fn require_consumed(c: &BorshCursor<'_>, layout: &str) -> Result<(), DecodeError> {
    if c.remaining() == 0 {
        Ok(())
    } else {
        Err(DecodeError::IncompatibleLayout(format!(
            "{layout} has {} trailing bytes",
            c.remaining()
        )))
    }
}

fn fee_payer(tx: &RawTransaction, fallback: Pubkey) -> Pubkey {
    tx.fee_payer
        .as_deref()
        .and_then(|s| Pubkey::from_base58(s).ok())
        .unwrap_or(fallback)
}

fn side(is_buy: bool) -> TradeSide {
    if is_buy {
        TradeSide::Buy
    } else {
        TradeSide::Sell
    }
}

fn instruction_name(is_buy: bool) -> &'static str {
    if is_buy {
        "buy"
    } else {
        "sell"
    }
}

fn market_for_mint(tx: &RawTransaction, mint: &Pubkey) -> Option<Pubkey> {
    let mint = mint.to_string();
    tx.instructions
        .iter()
        .chain(
            tx.inner_instructions
                .iter()
                .flat_map(|inner| inner.instructions.iter()),
        )
        .find(|ix| {
            ix.program_id == PROGRAM_ID
                && ix.accounts.get(2).map(String::as_str) == Some(mint.as_str())
        })
        .and_then(|ix| ix.accounts.get(3))
        .and_then(|market| Pubkey::from_base58(market).ok())
}

fn deterministic_launch_id(tx: &RawTransaction, event_index: u32) -> String {
    let hash = event_id(CHAIN, &tx.signature, PROGRAM_ID, &[0], event_index);
    let bytes = hex::decode(&hash[..32]).expect("event id is hexadecimal");
    let mut id = [0u8; 16];
    id.copy_from_slice(&bytes);
    Uuid::from_bytes(id).to_string()
}

fn fee_comp(kind: FeeKind, mint: Pubkey, amount: Atomic, decimals: u8) -> FeeComponent {
    FeeComponent {
        kind,
        asset: AssetAmount {
            mint,
            atomic: amount,
            decimals,
        },
        schedule_version: DECODER_VERSION.to_string(),
        charged_on_failure: false,
    }
}

/// The extended Create/Trade/Migrate event layouts added an explicit `quote_mint` field for
/// launches priced against something other than native SOL (USDC, PUMP, ...). For the ordinary
/// SOL-denominated bonding curve, pump.fun's own program leaves that field as `Pubkey::default()`
/// (32 zero bytes, which base58-encodes to the System Program address) rather than filling in
/// WSOL's real mint — the same "zero means: use the SOL-denominated fields instead" convention
/// already handled for `virtual_quote_reserves`/`quote_amount` (see the `is_zero()` fallbacks to
/// `virtual_sol`/`sol_amount` beside each call site below). Confirmed by field-order cross-check
/// against `vendor/pump-idl/pump.json`'s CreateEvent/TradeEvent/CompletePumpAmmMigrationEvent: the
/// borsh layout is read correctly end-to-end (every field after this one decodes to sane values),
/// so an unmapped zero pubkey here was this code failing to apply its own established convention
/// to `quote_mint`, not a misaligned read. Without this, every default-SOL-curve launch under the
/// extended layout stored `quote_mint = "111...111"`, which is neither WSOL nor a valid asset row
/// a price/decimals lookup can resolve — that is what left Radar's Price/MC columns blank for the
/// majority of tracked launches.
fn normalize_quote_mint(mint: Pubkey) -> Pubkey {
    if mint == Pubkey([0u8; 32]) {
        Pubkey::from_base58(pumphound_domain::WSOL_MINT).expect("static WSOL mint")
    } else {
        mint
    }
}

/// `None`, not the zero Pubkey, when pump.fun leaves a trade's creator-fee recipient at its
/// default — the same comparison `creator_fee_applies` already uses to mean "no distinct creator
/// set." Storage's launch-upsert reads this field's absence as a signal to fall back to a guess
/// from the trader's own address instead; stringifying the zero Pubkey here defeated that
/// fallback and let the System Program address get stored as a real-looking "creator" (confirmed
/// live: 400+ launches before this was added).
fn trade_creator_or_none(creator: Pubkey) -> Option<String> {
    if creator == Pubkey([0u8; 32]) {
        None
    } else {
        Some(creator.to_string())
    }
}

fn quote_decimals(mint: &Pubkey) -> u8 {
    let s = mint.to_base58();
    if s == pumphound_domain::USDC_MINT {
        6
    } else {
        9
    }
}

fn wrap(tx: &RawTransaction, event_index: u32, payload: ChainEventPayload) -> ChainEvent {
    wrap_at(tx, event_index, payload, None)
}

fn wrap_at(
    tx: &RawTransaction,
    event_index: u32,
    payload: ChainEventPayload,
    event_ts: Option<i64>,
) -> ChainEvent {
    let program = Pubkey::from_base58(PROGRAM_ID).expect("static program id");
    let chain_time = pumphound_domain::chain_time_from_parts(tx.block_time, event_ts);
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: event_id(CHAIN, &tx.signature, PROGRAM_ID, &[0], event_index),
        revision: 1,
        action: EventAction::Upsert,
        chain: CHAIN.to_string(),
        protocol: Protocol::PumpCurve,
        program_id: program,
        signature: tx.signature.clone(),
        slot: tx.slot,
        blockhash: None,
        tx_index: tx.transaction_index,
        instruction_path: vec![0],
        event_index,
        block_time: chain_time,
        observed_at: chain_time
            .or_else(|| {
                tx.block_time
                    .filter(|time| pumphound_domain::is_sane_chain_unix(time.timestamp()))
            })
            .unwrap_or_else(pumphound_domain::now_utc),
        computed_at: pumphound_domain::now_utc(),
        available_from_slot: tx.slot,
        commitment: Commitment::Processed,
        canonicality: if tx.transaction_index.is_some() {
            Canonicality::Provisional
        } else {
            Canonicality::Unknown
        },
        transaction_succeeded: tx.err.is_none(),
        provenance: Provenance {
            source: "collector".into(),
            source_record_id: tx.signature.clone(),
            payload_sha256: pumphound_domain::payload_sha256(tx.signature.as_bytes()),
            decoder_version: DECODER_VERSION.into(),
            idl_sha256: Some(IDL_SHA256.into()),
            raw_partition_uri: String::new(),
        },
        payload,
    }
}

fn quarantine(
    tx: &RawTransaction,
    event_index: u32,
    hex_disc: &str,
    payload: &[u8],
    reason: String,
) -> ChainEvent {
    wrap(
        tx,
        event_index,
        ChainEventPayload::DecodeQuarantined(DecodeQuarantined {
            discriminator_hex: hex_disc.to_string(),
            program_id: PROGRAM_ID.to_string(),
            reason,
            payload_length: Some(payload.len()),
            payload_sha256: Some(pumphound_domain::payload_sha256(payload)),
            decoder_version: DECODER_VERSION.into(),
            supporting_signature: Some(tx.signature.clone()),
        }),
    )
}

/// Best-effort settlement transfer tagging from token balance diffs.
pub fn settlement_transfers(tx: &RawTransaction, mint: &str) -> Vec<TransferObserved> {
    let mut out = Vec::new();
    for post in &tx.post_token_balances {
        let post_mint = post.get("mint").and_then(|v| v.as_str()).unwrap_or("");
        if post_mint != mint {
            continue;
        }
        let account_index = post
            .get("accountIndex")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let owner = post
            .get("owner")
            .and_then(|v| v.as_str())
            .and_then(|s| Pubkey::from_base58(s).ok());
        if let Some(acct) = tx.account_keys.get(account_index) {
            if let Ok(dest) = Pubkey::from_base58(acct) {
                out.push(TransferObserved {
                    source_owner: None,
                    destination_owner: owner,
                    source_account: dest,
                    destination_account: dest,
                    amount: AssetAmount {
                        mint: Pubkey::from_base58(mint).unwrap_or(dest),
                        atomic: Atomic(0),
                        decimals: 6,
                    },
                    transfer_role: TransferRole::TradeSettlement,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const TRADE_V1: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-02-01/223iNVDWMC4cNV5tHeAe5WA1WPhdcF2Xrjef6R9TZ3azeNfZb93S4M3d93zMhagKKN72uMYLuSJ8vsv4vWzUmF1S.json"
    );
    const TRADE_V2: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-06-01/125g5Ys9wjLF6xW2ebZxNxSNMaX58oAL4ErRs1yBFq4Tumh7zEoTtCBbcw8ug4KWk1DGo89igV7g9hg3M5UJyYXi.json"
    );
    const CREATE_191: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-02-01/414P4W9H7WCMSYPqTne85Z3eC81xPm3AP7uUWVfR9Xdze7DBz4UUHwg4jtcoQ1SBAffUGnJpC1pdVfPygbSVbAA3.json"
    );
    const CREATE_197: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-02-01/5xmKtQgHmQ7E4mzrQYLQPXDV2h2uRL1fpMAhVjbNpNbu7FJjyQv8APVERm4acZTqFVdhkU1fxtcMcneqevdsGxaK.json"
    );
    const CREATE_202: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-02-01/46VvPLPTvnwhEAWJoZNXTPLK2kkVJ4ddGjgff4LLX1maDn8656iNrihZKJpUvmh9HNo7LSp6zaxS1FQzGxTxDg8k.json"
    );
    const COMPLETE_V1: &str = include_str!(
        "../../../data/history-helius-full-25k/raw/date=2024-08-01/2dSkY6BJy2FhVcMET25U5EB5zEeijJDGJa7jaLEW8Xx4LAmaMqhVHZmaMUkN6xZ56rcybuUf8j4wJ9a2hHrGwdcH.json"
    );
    const CURRENT_TRADE: &str = include_str!(
        "../../../data/archive/raw/date=2026-09-07/4cEJocPCNfzSAMGxsgvHRGsTqkcn7xykx3LbPZ79R8H5ZwBE6dCGfp1HFrqsmH1GQGRkKMJpZLvA1qJm5otZREci.json"
    );

    fn archived(raw: &str) -> RawTransaction {
        let value: serde_json::Value = serde_json::from_str(raw).expect("archive JSON");
        serde_json::from_value(value["transaction"].clone()).expect("raw transaction")
    }

    fn decode(raw: &str) -> Vec<ChainEvent> {
        PumpBondingCurveDecoder::default()
            .decode(&archived(raw))
            .expect("decoder must not abort transaction")
    }

    #[test]
    fn decodes_archived_105_byte_trade_layout() {
        let tx = archived(TRADE_V1);
        let events = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        assert_eq!(events.len(), 1);
        let ChainEventPayload::TradeExecuted(trade) = &events[0].payload else {
            panic!("expected trade")
        };
        assert_eq!(trade.raw_event["event_layout"], "pump_trade_v1_legacy");
        assert_eq!(trade.quote_mint.to_string(), pumphound_domain::WSOL_MINT);
        assert!(trade.fee_components.is_empty());
        assert!(trade.real_base_atomic.is_none());
        assert!(trade.real_quote_atomic.is_none());
        assert_eq!(
            events[0].event_id,
            PumpBondingCurveDecoder::default().decode(&tx).unwrap()[0].event_id
        );
    }

    #[test]
    fn decodes_archived_121_byte_trade_layout() {
        let events = decode(TRADE_V2);
        assert_eq!(events.len(), 1);
        let trade = events
            .iter()
            .find_map(|event| match &event.payload {
                ChainEventPayload::TradeExecuted(trade) => Some(trade),
                _ => None,
            })
            .expect("trade");
        assert_eq!(
            trade.raw_event["event_layout"],
            "pump_trade_v2_real_reserves"
        );
        assert!(trade.real_base_atomic.is_some());
        assert!(trade.real_quote_atomic.is_some());
        assert!(trade.fee_components.is_empty());
    }

    #[test]
    fn decodes_variable_length_legacy_create_family() {
        for (raw, expected_len) in [(CREATE_191, 191), (CREATE_197, 197), (CREATE_202, 202)] {
            let tx = archived(raw);
            let payloads = collect_program_data_logs(&tx.log_messages, PROGRAM_ID);
            assert_eq!(payloads[0].1.len(), expected_len);
            let events = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
            assert_eq!(events.len(), 1);
            let ChainEventPayload::LaunchCreated(created) = &events[0].payload else {
                panic!("expected create")
            };
            assert_eq!(
                created.protocol_flags["event_layout"],
                "pump_create_v1_legacy"
            );
            assert!(created.creator.is_some());
            assert!(created.supply_atomic.is_none());
            assert!(created.virtual_base_atomic.is_none());
            assert!(created.virtual_quote_atomic.is_none());
            assert!(created.real_base_atomic.is_none());
            let repeated = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
            let ChainEventPayload::LaunchCreated(repeated) = &repeated[0].payload else {
                panic!("expected create")
            };
            assert_eq!(created.launch_id, repeated.launch_id);
        }
    }

    #[test]
    fn decodes_legacy_complete_without_dropping_following_trade() {
        let events = decode(COMPLETE_V1);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].payload,
            ChainEventPayload::LaunchCompleted(_)
        ));
        assert!(matches!(
            events[1].payload,
            ChainEventPayload::TradeExecuted(_)
        ));
    }

    #[test]
    fn current_trade_payload_is_stable_across_decodes() {
        let tx = archived(CURRENT_TRADE);
        let first = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        let second = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        let payload = |events: &[ChainEvent]| {
            events
                .iter()
                .find_map(|event| match &event.payload {
                    ChainEventPayload::TradeExecuted(trade) => {
                        Some(serde_json::to_vec(trade).unwrap())
                    }
                    _ => None,
                })
                .expect("current trade")
        };
        assert_eq!(payload(&first), payload(&second));
    }

    #[test]
    fn malformed_known_layout_is_quarantined_and_next_event_decodes() {
        let mut tx = archived(TRADE_V1);
        let (_, payload) = collect_program_data_logs(&tx.log_messages, PROGRAM_ID)
            .into_iter()
            .next()
            .unwrap();
        let mut malformed = Vec::from(TRADE);
        malformed.extend_from_slice(&payload[..payload.len() - 1]);
        let mut valid = Vec::from(TRADE);
        valid.extend_from_slice(&payload);
        tx.log_messages = vec![
            format!("Program {PROGRAM_ID} invoke [1]"),
            format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode(malformed)
            ),
            format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode(valid)
            ),
            format!("Program {PROGRAM_ID} success"),
        ];

        let first = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        let second = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        assert_eq!(first.len(), 2);
        let ChainEventPayload::DecodeQuarantined(quarantine) = &first[0].payload else {
            panic!("expected quarantine")
        };
        assert_eq!(quarantine.payload_length, Some(104));
        assert_eq!(
            quarantine.supporting_signature.as_deref(),
            Some(tx.signature.as_str())
        );
        assert!(quarantine.payload_sha256.is_some());
        assert_eq!(quarantine.decoder_version, DECODER_VERSION);
        assert!(matches!(
            first[1].payload,
            ChainEventPayload::TradeExecuted(_)
        ));
        assert_eq!(first[0].event_id, second[0].event_id);
        assert_eq!(first[1].event_id, second[1].event_id);
    }

    #[test]
    fn official_non_market_events_do_not_enter_decoder_quarantine() {
        for discriminator in NON_MARKET_EVENTS {
            let mut tx = archived(TRADE_V1);
            tx.log_messages = vec![
                format!("Program {PROGRAM_ID} invoke [1]"),
                format!(
                    "Program data: {}",
                    base64::engine::general_purpose::STANDARD.encode(discriminator)
                ),
                format!("Program {PROGRAM_ID} success"),
            ];
            let events = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
            assert!(
                events.is_empty(),
                "non-market event {} must remain raw-only",
                hex::encode(discriminator)
            );
        }
    }

    #[test]
    fn ignored_events_still_reserve_their_canonical_log_ordinal() {
        let mut tx = archived(TRADE_V1);
        tx.log_messages = vec![
            format!("Program {PROGRAM_ID} invoke [1]"),
            format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode(NON_MARKET_EVENTS[0])
            ),
            format!(
                "Program data: {}",
                base64::engine::general_purpose::STANDARD.encode([255u8; 8])
            ),
            format!("Program {PROGRAM_ID} success"),
        ];

        let events = PumpBondingCurveDecoder::default().decode(&tx).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_index, 1);
    }

    #[test]
    fn zero_pubkey_quote_mint_normalizes_to_wsol() {
        let zero = Pubkey([0u8; 32]);
        let wsol = Pubkey::from_base58(pumphound_domain::WSOL_MINT).unwrap();
        assert_eq!(normalize_quote_mint(zero), wsol);
    }

    #[test]
    fn non_zero_quote_mint_passes_through_unchanged() {
        let usdc = Pubkey::from_base58(pumphound_domain::USDC_MINT).unwrap();
        assert_eq!(normalize_quote_mint(usdc), usdc);
    }

    #[test]
    fn zero_pubkey_trade_creator_becomes_none_not_system_program_string() {
        assert_eq!(trade_creator_or_none(Pubkey([0u8; 32])), None);
    }

    #[test]
    fn non_zero_trade_creator_passes_through_as_string() {
        let real = Pubkey::from_base58(pumphound_domain::USDC_MINT).unwrap();
        assert_eq!(trade_creator_or_none(real), Some(real.to_string()));
    }
}
