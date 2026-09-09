//! INTEGRATION PATH (A2): execute the twin's canonical Pump BUY message inside a Surfpool
//! mainnet fork and compare the program's own `TradeEvent` with the twin's exact quote.
//!
//! Paper-only: the fee payer is the synthetic identity (no key exists). Surfpool cheatcodes fund
//! it and `simulateTransaction { sigVerify:false }` runs the unsigned message. Nothing is sent to
//! mainnet. This test is `#[ignore]` and only runs when `PUMPHOUND_SURFPOOL_RPC_URL` is set:
//!
//!   surfpool start --no-tui &          # or: just execution-twin-surfpool
//!   PUMPHOUND_SURFPOOL_RPC_URL=http://127.0.0.1:8899 \
//!   PUMPHOUND_SURFPOOL_MINT=<live pump curve mint> \
//!   cargo test -p pumphound-execution-twin --test surfpool_integration -- --ignored --nocapture
//!
//! Surfpool being offline degrades OFFLINE VERIFICATION only; live paper trading never depends
//! on it.

use base64::Engine;
use pumphound_execution_twin::builder::{self, MarketIdentity, SwapArgs};
use pumphound_execution_twin::contract::TransactionKind;
use pumphound_execution_twin::network::ComputeBudgetModel;
use pumphound_execution_twin::pda::{self, pk};
use pumphound_execution_twin::pump_math::{
    curve_buy_cost_for_tokens, curve_buy_tokens_for_sol, CurveState, ONE_BILLION_SUPPLY,
};
use pumphound_domain::{Pubkey, Slot, WSOL_MINT};
use serde_json::{json, Value};

const BONDING_CURVE_DISC: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];
const TRADE_EVENT_DISC: [u8; 8] = [189, 219, 127, 211, 78, 230, 97, 238];

struct Rpc {
    url: String,
    client: reqwest::blocking::Client,
}

impl Rpc {
    fn call(&self, method: &str, params: Value) -> Value {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let v: Value = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .expect("surfpool reachable")
            .json()
            .expect("json");
        assert!(
            v.get("error").is_none(),
            "{method} failed: {}",
            v["error"]
        );
        v["result"].clone()
    }
}

fn le_u64(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

struct OnChainCurve {
    state: CurveState,
    creator: Pubkey,
    complete: bool,
}

fn decode_bonding_curve(data: &[u8]) -> OnChainCurve {
    assert_eq!(&data[..8], &BONDING_CURVE_DISC, "not a BondingCurve account");
    let b = &data[8..];
    let creator = Pubkey(b[41..73].try_into().unwrap());
    OnChainCurve {
        state: CurveState {
            virtual_token_reserves: le_u64(b, 0) as u128,
            virtual_quote_reserves: le_u64(b, 8) as u128,
            real_token_reserves: le_u64(b, 16) as u128,
            real_quote_reserves: le_u64(b, 24) as u128,
            creator_is_set: creator != Pubkey([0u8; 32]),
            is_mayhem_mode: b[73] != 0,
            mint_supply: ONE_BILLION_SUPPLY,
            cashback_fee_bps: if b[74] != 0 { 30 } else { 0 },
        },
        creator,
        complete: b[40] != 0,
    }
}

/// `Program data: <b64>` lines carry anchor events; the CPI-emitted TradeEvent is what the
/// golden fixtures use, so the parity comparison is apples to apples.
fn trade_event_from_logs(logs: &[Value]) -> Option<(u64, u64, u64, u64)> {
    for line in logs {
        let Some(text) = line.as_str() else { continue };
        let Some(b64) = text.strip_prefix("Program data: ") else { continue };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else { continue };
        if bytes.len() < 8 + 32 + 8 + 8 + 1 || bytes[..8] != TRADE_EVENT_DISC {
            continue;
        }
        let sol_amount = le_u64(&bytes, 40);
        let token_amount = le_u64(&bytes, 48);
        // ... skip is_buy(1) user(32) timestamp(8) vsol(8) vtok(8) rsol(8) rtok(8) fee_recipient(32)
        // fee_basis_points(8) fee(8) creator(32) creator_fee_basis_points(8) creator_fee(8)
        let fee_off = 56 + 1 + 32 + 8 + 8 * 4 + 32 + 8;
        let fee = le_u64(&bytes, fee_off);
        let creator_fee = le_u64(&bytes, fee_off + 8 + 32 + 8);
        return Some((sol_amount, token_amount, fee, creator_fee));
    }
    None
}

#[test]
#[ignore = "requires a running Surfpool mainnet fork (PUMPHOUND_SURFPOOL_RPC_URL)"]
fn pump_buy_message_executes_in_surfpool_fork_and_matches_twin_quote() {
    let Ok(url) = std::env::var("PUMPHOUND_SURFPOOL_RPC_URL") else {
        eprintln!("PUMPHOUND_SURFPOOL_RPC_URL unset; skipping (OFFLINE VERIFICATION DEGRADED)");
        return;
    };
    let mint = std::env::var("PUMPHOUND_SURFPOOL_MINT")
        .expect("PUMPHOUND_SURFPOOL_MINT: a live (not complete) pump bonding-curve mint");
    let mint = pk(&mint);
    let rpc = Rpc {
        url,
        client: reqwest::blocking::Client::new(),
    };
    let curve_pda = pda::pump::bonding_curve(&mint);
    let info = rpc.call(
        "getAccountInfo",
        json!([curve_pda.to_base58(), {"encoding": "base64"}]),
    );
    let data_b64 = info["value"]["data"][0]
        .as_str()
        .expect("bonding curve account exists on the fork (lazy-cloned from mainnet)");
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_b64)
        .unwrap();
    let curve = decode_bonding_curve(&data);
    assert!(!curve.complete, "curve complete; pick a live mint");

    // 1. twin quote from the forked state
    let budget: u64 = 20_000_000; // 0.02 SOL
    let schedule = &pumphound_execution_twin::fee_config::schedules().curve;
    let tokens = curve_buy_tokens_for_sol(&curve.state, schedule, budget as u128).expect("tokens");
    let quote = curve_buy_cost_for_tokens(&curve.state, schedule, tokens).expect("quote");
    let filled_base = u64::try_from(quote.token_amount).unwrap();

    // 2. canonical message; fund the synthetic identity via Surfpool cheatcode
    let user = builder::synthetic_identity();
    rpc.call(
        "surfnet_setAccount",
        json!([user.to_base58(), {"lamports": 5_000_000_000u64}]),
    );
    let cu = ComputeBudgetModel::default().estimate(TransactionKind::PumpBuyExactSolIn, true);
    let identity = MarketIdentity {
        mint,
        market: curve_pda,
        quote_mint: pk(WSOL_MINT),
        creator: Some(curve.creator),
        base_token_program: pk(pda::TOKEN_PROGRAM),
    };
    let built = builder::build(
        &identity,
        &SwapArgs {
            kind: TransactionKind::PumpBuyExactSolIn,
            amount: filled_base,
            limit: budget,
            compute_unit_limit: cu.requested_limit,
            compute_unit_price_micro_lamports: 1_000,
            create_user_ata: true,
        },
        Slot(0),
    )
    .expect("build");
    let message = hex::decode(&built.message_hex).unwrap();
    // legacy wire tx = compact-u16 sig count + 64-byte zero signature + message
    let mut wire = vec![built.num_required_signatures];
    wire.extend(std::iter::repeat(0u8).take(64 * built.num_required_signatures as usize));
    wire.extend(message);
    let tx_b64 = base64::engine::general_purpose::STANDARD.encode(&wire);

    // 3. simulate in the fork (unsigned; blockhash replaced by the fork)
    let sim = rpc.call(
        "simulateTransaction",
        json!([tx_b64, {"encoding": "base64", "sigVerify": false, "replaceRecentBlockhash": true, "commitment": "processed"}]),
    );
    let value = &sim["value"];
    let logs = value["logs"].as_array().cloned().unwrap_or_default();
    eprintln!("surfpool logs:\n{}", logs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"));
    assert!(
        value["err"].is_null(),
        "program rejected the twin-built transaction: {} — the builder is wrong, not the chain",
        value["err"]
    );
    let consumed = value["unitsConsumed"].as_u64().expect("unitsConsumed");
    eprintln!(
        "CU consumed={consumed} twin_estimate={} requested={}",
        cu.estimated_cu, cu.requested_limit
    );
    assert!(
        consumed <= cu.requested_limit as u64,
        "ComputeBudgetModel under-requests: consumed {consumed} > limit {}",
        cu.requested_limit
    );

    // 4. parity: the program's TradeEvent must equal the twin's numbers exactly
    let (sol_amount, token_amount, protocol_fee, creator_fee) =
        trade_event_from_logs(&logs).expect("TradeEvent in logs");
    assert_eq!(token_amount as u128, quote.token_amount, "token_amount parity");
    assert_eq!(sol_amount as u128, quote.sol_cost, "sol_amount parity");
    assert_eq!(protocol_fee, quote.fees.protocol_fee_quote, "protocol fee parity");
    assert_eq!(creator_fee, quote.fees.creator_fee_quote, "creator fee parity");
}
