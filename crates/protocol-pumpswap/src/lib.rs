use pumphound_domain::{
    event_id, AssetAmount, Atomic, Canonicality, ChainEvent, ChainEventPayload, Commitment,
    DecodeError, DecodeQuarantined, EventAction, EventEnvelope, FeeComponent, FeeKind,
    LaunchCreated, LaunchpadDecoder, LiquidityChanged, LiquidityDirection, Protocol, Provenance,
    Pubkey, RawTransaction, TradeExecuted, TradeSide, CHAIN, SCHEMA_VERSION,
};
use pumphound_protocol_pump::BorshCursor;
use serde_json::json;

pub const PROGRAM_ID: &str = pumphound_domain::PUMPSWAP_PROGRAM_ID;
pub const IDL_SHA256: &str = "e18072707bd59afd1fca2245a18b48eee40c64904f2b34aa84100a0f014ddbe0";
pub const DECODER_VERSION: &str = pumphound_domain::DECODER_VERSION;

const CREATE_POOL: [u8; 8] = [177, 49, 12, 210, 160, 118, 167, 116];
const BUY: [u8; 8] = [103, 244, 82, 31, 44, 245, 119, 119];
const SELL: [u8; 8] = [62, 47, 55, 10, 165, 3, 220, 42];
const DEPOSIT: [u8; 8] = [120, 248, 61, 83, 31, 142, 107, 144];
const WITHDRAW: [u8; 8] = [22, 9, 133, 26, 160, 44, 71, 192];
// Non-market events from official @pump-fun/pump-swap-sdk 1.19.0. Raw
// sightings retain these payloads; they are intentionally not canonical
// market events and therefore must not inflate decoder quarantine health.
const NON_MARKET_EVENTS: [[u8; 8]; 20] = [
    [45, 220, 93, 24, 25, 97, 172, 104], // AdminSetCoinCreatorEvent
    [147, 250, 108, 120, 247, 29, 67, 222], // AdminUpdateTokenIncentivesEvent
    [63, 69, 28, 22, 48, 92, 194, 185],  // BoostBuyAndBurnEvent
    [226, 214, 246, 33, 7, 242, 147, 229], // ClaimCashbackEvent
    [79, 172, 246, 49, 205, 91, 206, 232], // ClaimTokenIncentivesEvent
    [146, 159, 189, 172, 146, 88, 56, 244], // CloseUserVolumeAccumulatorEvent
    [232, 245, 194, 238, 234, 218, 58, 89], // CollectCoinCreatorFeeEvent
    [107, 52, 89, 129, 55, 226, 81, 22], // CreateConfigEvent
    [107, 253, 193, 76, 228, 202, 27, 104], // DisableEvent
    [97, 97, 215, 144, 93, 146, 22, 124], // ExtendAccountEvent
    [174, 124, 74, 249, 4, 81, 246, 17], // InitBoostEvent
    [134, 36, 13, 72, 232, 101, 130, 216], // InitUserVolumeAccumulatorEvent
    [170, 221, 82, 199, 147, 165, 247, 46], // MigratePoolCoinCreatorEvent
    [43, 188, 250, 18, 221, 75, 187, 95], // ReservedFeeRecipientsEvent
    [242, 231, 235, 102, 65, 99, 189, 211], // SetBondingCurveCoinCreatorEvent
    [89, 128, 240, 141, 91, 202, 71, 105], // SetBoostAuthorityEvent
    [150, 107, 199, 123, 124, 207, 102, 228], // SetMetaplexCoinCreatorEvent
    [197, 122, 167, 124, 116, 81, 91, 255], // SyncUserVolumeAccumulatorEvent
    [225, 152, 171, 87, 246, 63, 66, 234], // UpdateAdminEvent
    [90, 23, 65, 35, 62, 244, 188, 208], // UpdateFeeConfigEvent
];

pub struct PumpSwapDecoder {
    program_ids: Vec<String>,
}

impl Default for PumpSwapDecoder {
    fn default() -> Self {
        Self {
            program_ids: vec![PROGRAM_ID.to_string()],
        }
    }
}

impl LaunchpadDecoder for PumpSwapDecoder {
    fn protocol(&self) -> Protocol {
        Protocol::PumpSwap
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
        for (event_index, (disc, payload)) in collect_logs(&tx.log_messages).into_iter().enumerate()
        {
            let event_index = event_index as u32;
            match decode_one(tx, disc, &payload, event_index) {
                Ok(Some(ev)) => out.push(ev),
                Ok(None) => {}
                Err(DecodeError::UnknownDiscriminator(hex)) => {
                    out.push(quarantine(
                        tx,
                        event_index,
                        hex,
                        &payload,
                        "unknown PumpSwap discriminator".into(),
                    ));
                }
                Err(error) => {
                    out.push(quarantine(
                        tx,
                        event_index,
                        hex::encode(disc),
                        &payload,
                        format!(
                            "known PumpSwap discriminator with unsupported or malformed {}-byte payload: {error}",
                            payload.len()
                        ),
                    ));
                }
            }
        }
        Ok(out)
    }
}

fn collect_logs(logs: &[String]) -> Vec<([u8; 8], Vec<u8>)> {
    let mut stack = Vec::new();
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
            if stack.last().map(|s| s.as_str()) != Some(PROGRAM_ID) {
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

/// Best-effort parse of the additive BuyEvent/SellEvent suffix (pump_amm IDL @9c82f61):
/// Buy: track_volume, total_unclaimed, total_claimed, current_sol_volume, last_update,
///      min_base_amount_out, ix_name, cashback_bps, cashback, buyback_bps, buyback_fee,
///      virtual_quote_reserves(i128), can_boost, base_supply.
/// Sell: cashback_bps, cashback, buyback_bps, buyback_fee, virtual_quote_reserves(i128),
///       can_boost, base_supply.
/// Older layouts stop early; every field stays `None` rather than being invented.
#[derive(Debug, Default)]
struct TradeSuffix {
    ix_name: Option<String>,
    cashback_bps: Option<u64>,
    virtual_quote_reserves: Option<i128>,
    can_boost: Option<bool>,
    base_supply: Option<u64>,
    layout: &'static str,
}

fn parse_trade_suffix(c: &mut BorshCursor<'_>, is_buy: bool) -> TradeSuffix {
    let mut out = TradeSuffix {
        layout: "legacy_no_suffix",
        ..Default::default()
    };
    if c.remaining() == 0 {
        return out;
    }
    let attempt = (|| -> Result<TradeSuffix, DecodeError> {
        let mut s = TradeSuffix::default();
        if is_buy {
            let _track = c.bool()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.i64()?;
            let _min_base_out = c.u64()?;
            s.ix_name = Some(c.string()?);
        }
        s.cashback_bps = Some(c.u64()?);
        let _cashback = c.u64()?;
        let _buyback_bps = c.u64()?;
        let _buyback = c.u64()?;
        s.virtual_quote_reserves = Some(c.i128()?);
        s.can_boost = Some(c.bool()?);
        s.base_supply = Some(c.u64()?);
        s.layout = "boost_v1";
        Ok(s)
    })();
    if let Ok(parsed) = attempt {
        out = parsed;
    } else {
        out.layout = "unrecognized_suffix";
    }
    out
}

fn decode_one(
    tx: &RawTransaction,
    disc: [u8; 8],
    payload: &[u8],
    event_index: u32,
) -> Result<Option<ChainEvent>, DecodeError> {
    let mut c = BorshCursor::new(payload);
    let ok = tx.err.is_none();
    match disc {
        CREATE_POOL => {
            let _ts = c.i64()?;
            let _index = c.u16()?;
            let creator = c.pubkey()?;
            let pool_base_mint = c.pubkey()?;
            let pool_quote_mint = c.pubkey()?;
            let pool_base_decimals = c.u8()?;
            let pool_quote_decimals = c.u8()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let pool_base = c.atomic_u64()?;
            let pool_quote = c.atomic_u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u8()?;
            let pool = c.pubkey()?;
            let _lp = c.pubkey()?;
            let _ = c.pubkey()?;
            let _ = c.pubkey()?;
            let coin_creator = c.pubkey()?;
            let is_mayhem = c.bool()?;
            let identity = normalize_pool_identity(
                pool_base_mint,
                pool_quote_mint,
                pool_base_decimals,
                pool_quote_decimals,
            );
            let (token_reserve, quote_reserve) = identity.orient(pool_base, pool_quote);
            Ok(Some(wrap(
                tx,
                event_index,
                ChainEventPayload::LaunchCreated(LaunchCreated {
                    launch_id: uuid::Uuid::now_v7().to_string(),
                    mint: identity.token_mint,
                    creator: Some(coin_creator),
                    market: pool,
                    quote_mint: identity.quote_mint,
                    base_decimals: identity.token_decimals,
                    quote_decimals: identity.quote_decimals,
                    supply_atomic: Some(token_reserve),
                    metadata_uri: None,
                    name: None,
                    symbol: None,
                    protocol_flags: json!({
                        "is_mayhem_mode": is_mayhem,
                        "pool_creator": creator.to_string(),
                        "pool_base_mint": pool_base_mint.to_string(),
                        "pool_quote_mint": pool_quote_mint.to_string(),
                        "orientation_normalized": identity.reversed,
                    }),
                    virtual_base_atomic: Some(Atomic(0)),
                    // PumpSwap uses real pool reserves. MarketSnapshot has one
                    // quote reserve accumulator, so carry the normalized pool
                    // quote reserve here until the first trade update.
                    virtual_quote_atomic: Some(quote_reserve),
                    real_base_atomic: Some(token_reserve),
                }),
            )))
        }
        BUY | SELL => {
            if !ok {
                return Ok(None);
            }
            let is_buy = disc == BUY;
            let _ts = c.i64()?;
            let base_amt = c.atomic_u64()?;
            let _limit = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let pool_base = c.atomic_u64()?;
            let pool_quote = c.atomic_u64()?;
            let quote_amt = c.atomic_u64()?;
            let lp_bps = c.u64()?;
            let lp_fee = c.atomic_u64()?;
            let proto_bps = c.u64()?;
            let proto_fee = c.atomic_u64()?;
            let _ = c.u64()?;
            let user_quote = c.atomic_u64()?;
            let pool = c.pubkey()?;
            let user = c.pubkey()?;
            let _ = c.pubkey()?;
            let _ = c.pubkey()?;
            let _ = c.pubkey()?;
            let _ = c.pubkey()?;
            let coin_creator = c.pubkey()?;
            let creator_bps = c.u64()?;
            let creator_fee = c.atomic_u64()?;
            // Newer PumpSwap releases append cashback, buyback and boost fields. The suffix is
            // parsed best-effort so old archives still decode; when present it yields the boost
            // pool's virtual quote reserves and the post-burn base supply, both required for
            // exact quoting (effective reserve = real + virtual; fee tier keyed by market cap).
            let boost = parse_trade_suffix(&mut c, is_buy);
            // `quote_amount_atomic` is what the trader paid (buy) or received (sell):
            //   buy:                quote_amount_in = swap quote, user_quote_amount_in = total incl. fees
            //   buy_exact_quote_in: quote_amount_in = total incl. fees, user_quote_amount_in = swap quote
            //   sell:               quote_amount_out = gross, user_quote_amount_out = net of fees
            // (verified against mainnet BuyEvent/SellEvent fixtures in execution-twin golden tests).
            let quote = if user_quote.is_zero() {
                quote_amt
            } else if is_buy {
                Atomic(quote_amt.0.max(user_quote.0))
            } else {
                user_quote
            };
            let swap_quote = if is_buy && !user_quote.is_zero() {
                Atomic(quote_amt.0.min(user_quote.0))
            } else {
                quote_amt
            };
            // Event reserves are PRE-trade. Derive the post-trade pool state the reducer needs:
            // buys add swap_quote + lp_fee to the pool and remove base_out; sells add base_in and
            // remove (quote_out - lp_fee). Protocol/creator fees leave for their recipients.
            let (pool_base_post, pool_quote_post) = if is_buy {
                (
                    Atomic(pool_base.0.saturating_sub(base_amt.0)),
                    Atomic(
                        pool_quote
                            .0
                            .saturating_add(swap_quote.0)
                            .saturating_add(lp_fee.0),
                    ),
                )
            } else {
                (
                    Atomic(pool_base.0.saturating_add(base_amt.0)),
                    Atomic(
                        pool_quote
                            .0
                            .saturating_sub(quote_amt.0.saturating_sub(lp_fee.0)),
                    ),
                )
            };
            let Some(identity) = pool_identity_from_transaction(tx, pool, user) else {
                return Err(DecodeError::IncompatibleLayout(format!(
                    "pump_swap_identity_unresolved pool={pool} full_transaction_required"
                )));
            };
            let (token_amount, quote_amount) = identity.orient(base_amt, quote);
            let (token_reserve, quote_reserve) = identity.orient(pool_base_post, pool_quote_post);
            let (pre_token_reserve, pre_quote_reserve) = identity.orient(pool_base, pool_quote);
            let normalized_side = if identity.reversed {
                if is_buy {
                    TradeSide::Sell
                } else {
                    TradeSide::Buy
                }
            } else if is_buy {
                TradeSide::Buy
            } else {
                TradeSide::Sell
            };
            let fee_payer = tx
                .fee_payer
                .as_deref()
                .and_then(|s| Pubkey::from_base58(s).ok())
                .unwrap_or(user);
            let fee_decimals = token_decimals(tx, identity.pool_quote_mint)
                .unwrap_or(identity.pool_quote_decimals);
            Ok(Some(wrap(
                tx,
                event_index,
                ChainEventPayload::TradeExecuted(TradeExecuted {
                    launch_id: None,
                    mint: identity.token_mint,
                    market: pool,
                    trader: user,
                    fee_payer,
                    side: normalized_side,
                    base_amount_atomic: token_amount,
                    quote_amount_atomic: quote_amount,
                    quote_mint: identity.quote_mint,
                    fee_components: vec![
                        fee(FeeKind::Lp, lp_fee, identity.pool_quote_mint, fee_decimals),
                        fee(
                            FeeKind::Protocol,
                            proto_fee,
                            identity.pool_quote_mint,
                            fee_decimals,
                        ),
                        fee(
                            FeeKind::Creator,
                            creator_fee,
                            identity.pool_quote_mint,
                            fee_decimals,
                        ),
                    ],
                    reserve_snapshot_id: None,
                    instruction_name: boost.ix_name.clone().unwrap_or_else(|| {
                        if is_buy {
                            "buy".into()
                        } else {
                            "sell".into()
                        }
                    }),
                    virtual_base_atomic: Some(Atomic(0)),
                    // Boost pools: effective quote = real + virtual. Legacy layouts: 0 (known).
                    virtual_quote_atomic: Some(Atomic(
                        boost.virtual_quote_reserves.filter(|v| *v > 0).unwrap_or(0) as u128,
                    )),
                    real_base_atomic: Some(token_reserve),
                    real_quote_atomic: Some(quote_reserve),
                    raw_event: json!({
                        "identity_source": "transaction_instruction_accounts",
                        "pool_base_mint": identity.pool_base_mint.to_string(),
                        "pool_quote_mint": identity.pool_quote_mint.to_string(),
                        "token_decimals": identity.token_decimals,
                        "quote_decimals": identity.quote_decimals,
                        "orientation_normalized": identity.reversed,
                        "executable_quote_supported": true,
                        "reserve_semantics": "post_trade_derived_from_pre_trade_event",
                        "pre_trade_base_reserve": pre_token_reserve.to_string(),
                        "pre_trade_quote_reserve": pre_quote_reserve.to_string(),
                        "pool_quote_amount": quote_amt.to_string(),
                        "coin_creator": coin_creator.to_string(),
                        "lp_fee_basis_points": lp_bps,
                        "protocol_fee_basis_points": proto_bps,
                        "coin_creator_fee_basis_points": creator_bps,
                        "virtual_quote_reserves": boost.virtual_quote_reserves,
                        "base_supply": boost.base_supply,
                        "can_boost": boost.can_boost,
                        "suffix_layout": boost.layout,
                    }),
                    protocol_fee_bps: Some(proto_bps),
                    creator_fee_bps: Some(creator_bps),
                    lp_fee_bps: Some(lp_bps),
                    is_mayhem_mode: None,
                    creator_fee_applies: Some(coin_creator.0 != [0u8; 32]),
                    base_supply_atomic: boost.base_supply.map(|v| Atomic(v as u128)),
                    cashback_fee_bps: boost.cashback_bps,
                }),
            )))
        }
        DEPOSIT | WITHDRAW => {
            if !ok {
                return Ok(None);
            }
            let _ts = c.i64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _ = c.u64()?;
            let _pool_base = c.atomic_u64()?;
            let _pool_quote = c.atomic_u64()?;
            let base_in = c.atomic_u64()?;
            let quote_in = c.atomic_u64()?;
            let _ = c.u64()?;
            let pool = c.pubkey()?;
            let user = c.pubkey()?;
            let Some(identity) = pool_identity_from_transaction(tx, pool, user) else {
                return Err(DecodeError::IncompatibleLayout(format!(
                    "pump_swap_identity_unresolved pool={pool} full_transaction_required"
                )));
            };
            let (base_delta, quote_delta) = identity.orient(base_in, quote_in);
            Ok(Some(wrap(
                tx,
                event_index,
                ChainEventPayload::LiquidityChanged(LiquidityChanged {
                    mint: identity.token_mint,
                    market: pool,
                    lp_provider: Some(user),
                    base_delta_atomic: base_delta,
                    quote_delta_atomic: quote_delta,
                    direction: if disc == DEPOSIT {
                        LiquidityDirection::Deposit
                    } else {
                        LiquidityDirection::Withdraw
                    },
                    instruction_name: if disc == DEPOSIT {
                        "deposit".into()
                    } else {
                        "withdraw".into()
                    },
                }),
            )))
        }
        other if NON_MARKET_EVENTS.contains(&other) => Ok(None),
        other => Err(DecodeError::UnknownDiscriminator(hex::encode(other))),
    }
}

#[derive(Clone, Copy, Debug)]
struct PoolIdentity {
    pool_base_mint: Pubkey,
    pool_quote_mint: Pubkey,
    pool_quote_decimals: u8,
    token_mint: Pubkey,
    quote_mint: Pubkey,
    token_decimals: u8,
    quote_decimals: u8,
    reversed: bool,
}

impl PoolIdentity {
    fn orient(self, pool_base: Atomic, pool_quote: Atomic) -> (Atomic, Atomic) {
        if self.reversed {
            (pool_quote, pool_base)
        } else {
            (pool_base, pool_quote)
        }
    }
}

fn normalize_pool_identity(
    pool_base_mint: Pubkey,
    pool_quote_mint: Pubkey,
    pool_base_decimals: u8,
    pool_quote_decimals: u8,
) -> PoolIdentity {
    let reversed = is_supported_quote(pool_base_mint) && !is_supported_quote(pool_quote_mint);
    if reversed {
        PoolIdentity {
            pool_base_mint,
            pool_quote_mint,
            pool_quote_decimals,
            token_mint: pool_quote_mint,
            quote_mint: pool_base_mint,
            token_decimals: pool_quote_decimals,
            quote_decimals: pool_base_decimals,
            reversed,
        }
    } else {
        PoolIdentity {
            pool_base_mint,
            pool_quote_mint,
            pool_quote_decimals,
            token_mint: pool_base_mint,
            quote_mint: pool_quote_mint,
            token_decimals: pool_base_decimals,
            quote_decimals: pool_quote_decimals,
            reversed,
        }
    }
}

fn is_supported_quote(mint: Pubkey) -> bool {
    mint.to_string() == pumphound_domain::WSOL_MINT
        || mint.to_string() == pumphound_domain::USDC_MINT
}

fn pool_identity_from_transaction(
    tx: &RawTransaction,
    pool: Pubkey,
    user: Pubkey,
) -> Option<PoolIdentity> {
    let pool = pool.to_string();
    let user = user.to_string();
    let instruction = tx
        .instructions
        .iter()
        .chain(
            tx.inner_instructions
                .iter()
                .flat_map(|group| group.instructions.iter()),
        )
        .find(|instruction| {
            instruction.program_id == PROGRAM_ID
                && instruction.accounts.len() >= 5
                && instruction.accounts[0] == pool
                // Swap instructions place user at index 1; deposit/withdraw
                // place global_config at 1 and user at 2.
                && (instruction.accounts[1] == user || instruction.accounts[2] == user)
        })?;
    let pool_base_mint = Pubkey::from_base58(&instruction.accounts[3]).ok()?;
    let pool_quote_mint = Pubkey::from_base58(&instruction.accounts[4]).ok()?;
    let pool_base_decimals =
        token_decimals(tx, pool_base_mint).unwrap_or_else(|| default_decimals(pool_base_mint));
    let pool_quote_decimals =
        token_decimals(tx, pool_quote_mint).unwrap_or_else(|| default_decimals(pool_quote_mint));
    Some(normalize_pool_identity(
        pool_base_mint,
        pool_quote_mint,
        pool_base_decimals,
        pool_quote_decimals,
    ))
}

fn token_decimals(tx: &RawTransaction, mint: Pubkey) -> Option<u8> {
    let mint = mint.to_string();
    tx.pre_token_balances
        .iter()
        .chain(tx.post_token_balances.iter())
        .find(|balance| balance.get("mint").and_then(serde_json::Value::as_str) == Some(&mint))
        .and_then(|balance| {
            balance
                .pointer("/uiTokenAmount/decimals")
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|value| u8::try_from(value).ok())
}

fn default_decimals(mint: Pubkey) -> u8 {
    if mint.to_string() == pumphound_domain::WSOL_MINT {
        9
    } else {
        6
    }
}

fn fee(kind: FeeKind, amount: Atomic, mint: Pubkey, decimals: u8) -> FeeComponent {
    FeeComponent {
        kind,
        asset: AssetAmount {
            mint,
            atomic: amount,
            decimals,
        },
        schedule_version: DECODER_VERSION.into(),
        charged_on_failure: false,
    }
}

fn quarantine(
    tx: &RawTransaction,
    event_index: u32,
    discriminator_hex: String,
    payload: &[u8],
    reason: String,
) -> ChainEvent {
    wrap(
        tx,
        event_index,
        ChainEventPayload::DecodeQuarantined(DecodeQuarantined {
            discriminator_hex,
            program_id: PROGRAM_ID.into(),
            reason,
            payload_length: Some(payload.len()),
            payload_sha256: Some(pumphound_domain::payload_sha256(payload)),
            decoder_version: DECODER_VERSION.into(),
            supporting_signature: Some(tx.signature.clone()),
        }),
    )
}

fn wrap(tx: &RawTransaction, event_index: u32, payload: ChainEventPayload) -> ChainEvent {
    EventEnvelope {
        schema_version: SCHEMA_VERSION,
        event_id: event_id(CHAIN, &tx.signature, PROGRAM_ID, &[0], event_index),
        revision: 1,
        action: EventAction::Upsert,
        chain: CHAIN.into(),
        protocol: Protocol::PumpSwap,
        program_id: Pubkey::from_base58(PROGRAM_ID).expect("program"),
        signature: tx.signature.clone(),
        slot: tx.slot,
        blockhash: None,
        tx_index: tx.transaction_index,
        instruction_path: vec![0],
        event_index,
        block_time: tx.block_time,
        observed_at: pumphound_domain::now_utc(),
        computed_at: pumphound_domain::now_utc(),
        available_from_slot: tx.slot,
        commitment: Commitment::Processed,
        canonicality: Canonicality::Provisional,
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

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use pumphound_domain::{ChainEventPayload, RawInnerInstructions, RawInstruction, Slot};

    fn key(byte: u8) -> Pubkey {
        Pubkey([byte; 32])
    }

    fn raw_tx(log_data: Vec<u8>, instructions: Vec<RawInstruction>) -> RawTransaction {
        RawTransaction {
            signature: "test-signature".into(),
            slot: Slot(42),
            block_time: None,
            transaction_index: None,
            err: None,
            fee_payer: Some(key(2).to_string()),
            account_keys: vec![PROGRAM_ID.into()],
            loaded_writable_addresses: vec![],
            loaded_readonly_addresses: vec![],
            instructions,
            inner_instructions: Vec::<RawInnerInstructions>::new(),
            log_messages: vec![
                format!("Program {PROGRAM_ID} invoke [1]"),
                format!(
                    "Program data: {}",
                    base64::engine::general_purpose::STANDARD.encode(log_data)
                ),
                format!("Program {PROGRAM_ID} success"),
            ],
            pre_balances: vec![],
            post_balances: vec![],
            pre_token_balances: vec![],
            post_token_balances: vec![],
            compute_units_consumed: None,
            raw_json: json!({}),
        }
    }

    fn push_u64(payload: &mut Vec<u8>, value: u64) {
        payload.extend_from_slice(&value.to_le_bytes());
    }

    fn modern_sell_event(pool: Pubkey, user: Pubkey) -> Vec<u8> {
        let mut payload = SELL.to_vec();
        payload.extend_from_slice(&1i64.to_le_bytes());
        for value in [
            10_000,
            1,
            2,
            3,
            105_003_422_795,
            809_878_642_946_578,
            76_897_390,
            25,
            192_822,
            5,
            38_565,
            0,
            76_897_390,
        ] {
            push_u64(&mut payload, value);
        }
        payload.extend_from_slice(&pool.0);
        payload.extend_from_slice(&user.0);
        for byte in 10..15 {
            payload.extend_from_slice(&key(byte).0);
        }
        push_u64(&mut payload, 0);
        push_u64(&mut payload, 0);
        // Modern sell-only additive suffix.
        for value in [0, 0, 0, 0] {
            push_u64(&mut payload, value);
        }
        payload.extend_from_slice(&0i128.to_le_bytes());
        payload.push(0);
        push_u64(&mut payload, 1_000_000);
        payload
    }

    #[test]
    fn full_sell_normalizes_reversed_wsol_pool_identity() {
        let pool = key(1);
        let user = key(2);
        let token = key(3);
        let wsol = Pubkey::from_base58(pumphound_domain::WSOL_MINT).unwrap();
        let instruction = RawInstruction {
            program_id: PROGRAM_ID.into(),
            accounts: vec![
                pool.to_string(),
                user.to_string(),
                key(8).to_string(),
                wsol.to_string(),
                token.to_string(),
            ],
            data_base64: String::new(),
            stack_height: Some(1),
        };
        let events = PumpSwapDecoder::default()
            .decode(&raw_tx(modern_sell_event(pool, user), vec![instruction]))
            .unwrap();
        let ChainEventPayload::TradeExecuted(trade) = &events[0].payload else {
            panic!("expected normalized trade");
        };
        assert_eq!(trade.market, pool);
        assert_eq!(trade.mint, token);
        assert_eq!(trade.quote_mint, wsol);
        assert_eq!(trade.side, TradeSide::Buy);
        assert_eq!(trade.base_amount_atomic, Atomic(76_897_390));
        assert_eq!(trade.quote_amount_atomic, Atomic(10_000));
        // Event reserves are pre-trade; the canonical event carries the derived post-trade
        // state: pool base (WSOL here, reversed) += base_in; pool quote -= (quote_out - lp_fee).
        assert_eq!(
            trade.real_base_atomic,
            Some(Atomic(809_878_642_946_578 - (76_897_390 - 192_822)))
        );
        assert_eq!(
            trade.real_quote_atomic,
            Some(Atomic(105_003_422_795 + 10_000))
        );
        assert_eq!(trade.raw_event["pre_trade_base_reserve"], "809878642946578");
        assert_eq!(trade.base_supply_atomic, Some(Atomic(1_000_000)));
        assert_eq!(trade.lp_fee_bps, Some(25));
        assert_eq!(trade.protocol_fee_bps, Some(5));
        assert_eq!(trade.creator_fee_applies, Some(true)); // coin_creator = key(14) != default
        assert_eq!(trade.raw_event["suffix_layout"], "boost_v1");
        assert_eq!(trade.fee_components[0].asset.mint, token);
    }

    #[test]
    fn logs_only_trade_is_quarantined_instead_of_using_pool_as_mint() {
        let pool = key(1);
        let user = key(2);
        let events = PumpSwapDecoder::default()
            .decode(&raw_tx(modern_sell_event(pool, user), vec![]))
            .unwrap();
        assert_eq!(events.len(), 1);
        let ChainEventPayload::DecodeQuarantined(quarantine) = &events[0].payload else {
            panic!("unresolved identity must be quarantined");
        };
        assert!(quarantine.reason.contains("pump_swap_identity_unresolved"));
        assert!(quarantine.reason.contains(&format!("pool={pool}")));
    }

    #[test]
    fn official_non_market_events_do_not_enter_decoder_quarantine() {
        for discriminator in NON_MARKET_EVENTS {
            let events = PumpSwapDecoder::default()
                .decode(&raw_tx(discriminator.to_vec(), vec![]))
                .unwrap();
            assert!(
                events.is_empty(),
                "non-market event {} must remain raw-only",
                hex::encode(discriminator)
            );
        }
    }

    #[test]
    fn ignored_events_still_reserve_their_canonical_log_ordinal() {
        let mut tx = raw_tx(Vec::new(), vec![]);
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

        let events = PumpSwapDecoder::default().decode(&tx).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_index, 1);
    }

    #[test]
    fn liquidity_instruction_account_order_resolves_pool_identity() {
        let pool = key(1);
        let user = key(2);
        let token = key(3);
        let wsol = Pubkey::from_base58(pumphound_domain::WSOL_MINT).unwrap();
        let instruction = RawInstruction {
            program_id: PROGRAM_ID.into(),
            accounts: vec![
                pool.to_string(),
                key(9).to_string(), // global_config
                user.to_string(),
                token.to_string(),
                wsol.to_string(),
            ],
            data_base64: String::new(),
            stack_height: Some(1),
        };
        let tx = raw_tx(Vec::new(), vec![instruction]);
        let identity = pool_identity_from_transaction(&tx, pool, user).unwrap();
        assert_eq!(identity.token_mint, token);
        assert_eq!(identity.quote_mint, wsol);
    }
}
