//! GATE 1 — protocol parity.
//!
//! Every fixture is a real mainnet Pump / PumpSwap trade (Helius `getTransaction`, anchor
//! `emit_cpi` event bytes kept verbatim in `event_b64`). The twin must reproduce the program's
//! own numbers EXACTLY from the pre-trade state: swap amount, protocol/creator/lp/cashback fees
//! and the trader's net amount. Any disagreement beyond the documented +1 lamport offset of the
//! `buy_exact_*_in` curve instructions is a hard failure — never averaged, never tolerated.

use pumphound_execution_twin::fee_config::schedules;
use pumphound_execution_twin::pump_math::{
    curve_buy_cost_for_tokens, curve_sell, fee, pool_buy_cost_for_base, pool_buy_exact_quote_in,
    pool_sell, CurveState, PoolState, ONE_BILLION_SUPPLY,
};
use serde::Deserialize;

const DEFAULT_PUBKEY: &str = "11111111111111111111111111111111";

#[derive(Deserialize)]
struct Fixtures {
    trades: Vec<Trade>,
}

#[derive(Deserialize)]
struct Trade {
    protocol: String,
    signature: String,
    event: serde_json::Value,
}

fn u(v: &serde_json::Value, k: &str) -> u128 {
    v[k].as_u64()
        .unwrap_or_else(|| panic!("fixture field {k} missing")) as u128
}
fn i(v: &serde_json::Value, k: &str) -> i128 {
    v[k].as_i64().map(|x| x as i128).unwrap_or_else(|| {
        v[k].as_f64()
            .map(|f| f as i128)
            .unwrap_or_else(|| panic!("fixture field {k} missing"))
    })
}
fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn load() -> Fixtures {
    serde_json::from_str(include_str!("fixtures/golden_trades.json")).expect("fixture parses")
}

#[derive(Default, Debug)]
struct Tally {
    curve_buy: usize,
    curve_buy_exact_in: usize,
    curve_sell: usize,
    curve_cashback: usize,
    curve_mayhem_fee_exempt_skipped: usize,
    curve_mayhem_retail: usize,
    amm_buy: usize,
    amm_buy_exact_quote_in: usize,
    amm_sell: usize,
    amm_boost_pool: usize,
}

#[test]
fn archived_mainnet_trades_reproduce_exactly() {
    let fx = load();
    let sched = schedules();
    let mut t = Tally::default();
    assert!(
        fx.trades.len() >= 60,
        "need a meaningful sample, got {}",
        fx.trades.len()
    );

    for tr in &fx.trades {
        let e = &tr.event;
        let sig = &tr.signature;
        match tr.protocol.as_str() {
            "pump_curve" => {
                let mayhem = e["mayhem_mode"].as_bool().unwrap_or(false);
                let sol = u(e, "sol_amount");
                let tok = u(e, "token_amount");
                if mayhem && (u(e, "fee_basis_points") == 0 || sol == 0 || tok == 0) {
                    // Mayhem-mode coins carry fee-exempt trades executed by the mayhem authority
                    // (fee_basis_points=0). Those do NOT follow the retail constant-product path
                    // and are not something PumpHound could ever execute, so the twin never quotes
                    // them; fee-bearing retail trades on mayhem coins are checked like any other.
                    t.curve_mayhem_fee_exempt_skipped += 1;
                    continue;
                }
                if mayhem {
                    t.curve_mayhem_retail += 1;
                }
                let is_buy = e["is_buy"].as_bool().unwrap();
                let (vs, vt, rs, rt) = (
                    u(e, "virtual_sol_reserves"),
                    u(e, "virtual_token_reserves"),
                    u(e, "real_sol_reserves"),
                    u(e, "real_token_reserves"),
                );
                // TradeEvent reserves are POST-trade; roll back to the pre-trade curve.
                let pre = if is_buy {
                    CurveState {
                        virtual_token_reserves: vt + tok,
                        virtual_quote_reserves: vs - sol,
                        real_token_reserves: rt + tok,
                        real_quote_reserves: rs - sol,
                        creator_is_set: s(e, "creator") != DEFAULT_PUBKEY,
                        is_mayhem_mode: mayhem,
                        mint_supply: ONE_BILLION_SUPPLY,
                        cashback_fee_bps: u(e, "cashback_fee_basis_points") as u64,
                    }
                } else {
                    CurveState {
                        virtual_token_reserves: vt - tok,
                        virtual_quote_reserves: vs + sol,
                        real_token_reserves: rt - tok,
                        real_quote_reserves: rs + sol,
                        creator_is_set: s(e, "creator") != DEFAULT_PUBKEY,
                        is_mayhem_mode: mayhem,
                        mint_supply: ONE_BILLION_SUPPLY,
                        cashback_fee_bps: u(e, "cashback_fee_basis_points") as u64,
                    }
                };
                let ix = s(e, "ix_name");
                if pre.cashback_fee_bps > 0 {
                    t.curve_cashback += 1;
                }
                if is_buy {
                    let q = curve_buy_cost_for_tokens(&pre, &sched.curve, tok)
                        .unwrap_or_else(|err| panic!("{sig}: {err}"));
                    if ix == "buy" {
                        assert_eq!(q.sol_cost, sol, "{sig}: buy sol_cost");
                        assert_eq!(q.fees.protocol_fee_quote as u128, u(e, "fee"), "{sig}: fee");
                        assert_eq!(
                            q.fees.creator_fee_quote as u128,
                            u(e, "creator_fee"),
                            "{sig}: creator_fee"
                        );
                        assert_eq!(
                            q.fees.cashback_fee_quote as u128,
                            u(e, "cashback"),
                            "{sig}: cashback"
                        );
                        t.curve_buy += 1;
                    } else {
                        // buy_exact_sol_in / buy_exact_quote_in: the program derives the token
                        // amount from the input and then charges cost(tokens)+1 as sol_amount.
                        // Observed on every archived exact-in trade: exactly +1 lamport.
                        assert_eq!(q.sol_cost + 1, sol, "{sig}: {ix} sol_amount offset");
                        assert_eq!(
                            fee(sol, q.fees.protocol_fee_bps).unwrap(),
                            u(e, "fee"),
                            "{sig}: {ix} fee"
                        );
                        assert_eq!(
                            fee(sol, q.fees.creator_fee_bps).unwrap(),
                            u(e, "creator_fee"),
                            "{sig}: {ix} creator_fee"
                        );
                        t.curve_buy_exact_in += 1;
                    }
                    assert_eq!(
                        q.fees.protocol_fee_bps as u128,
                        u(e, "fee_basis_points"),
                        "{sig}: protocol bps"
                    );
                    assert_eq!(
                        q.fees.creator_fee_bps as u128,
                        u(e, "creator_fee_basis_points"),
                        "{sig}: creator bps"
                    );
                } else {
                    let q = curve_sell(&pre, &sched.curve, tok)
                        .unwrap_or_else(|err| panic!("{sig}: {err}"));
                    assert_eq!(q.sol_out_gross, sol, "{sig}: sell gross");
                    assert_eq!(q.fees.protocol_fee_quote as u128, u(e, "fee"), "{sig}: fee");
                    assert_eq!(
                        q.fees.creator_fee_quote as u128,
                        u(e, "creator_fee"),
                        "{sig}: creator_fee"
                    );
                    assert_eq!(
                        q.fees.cashback_fee_quote as u128,
                        u(e, "cashback"),
                        "{sig}: cashback"
                    );
                    assert_eq!(
                        q.fees.protocol_fee_bps as u128,
                        u(e, "fee_basis_points"),
                        "{sig}: protocol bps"
                    );
                    assert_eq!(
                        q.fees.creator_fee_bps as u128,
                        u(e, "creator_fee_basis_points"),
                        "{sig}: creator bps"
                    );
                    t.curve_sell += 1;
                }
            }
            "pump_swap" => {
                let is_buy = e["is_buy"].as_bool().unwrap();
                let vq = i(e, "virtual_quote_reserves").max(0) as u128;
                if vq > 0 {
                    t.amm_boost_pool += 1;
                }
                let pool = PoolState {
                    base_reserve: u(e, "pool_base_token_reserves"),
                    quote_reserve: u(e, "pool_quote_token_reserves"),
                    virtual_quote_reserves: vq,
                    base_mint_supply: u(e, "base_supply"),
                    coin_creator_is_set: s(e, "coin_creator") != DEFAULT_PUBKEY,
                    is_pump_pool: true,
                    cashback_fee_bps: u(e, "cashback_fee_basis_points") as u64,
                };
                let base = u(e, "base_amount");
                let quote = u(e, "quote_amount");
                let user_quote = u(e, "user_quote_amount");
                let ev_fees = (
                    u(e, "lp_fee"),
                    u(e, "protocol_fee"),
                    u(e, "coin_creator_fee"),
                );
                let ev_bps = (
                    u(e, "lp_fee_basis_points"),
                    u(e, "protocol_fee_basis_points"),
                    u(e, "coin_creator_fee_basis_points"),
                );
                let check_fees = |f: &pumphound_execution_twin::ProtocolFees, what: &str| {
                    assert_eq!(
                        (
                            f.lp_fee_quote as u128,
                            f.protocol_fee_quote as u128,
                            f.creator_fee_quote as u128
                        ),
                        ev_fees,
                        "{sig}: {what} fees"
                    );
                    assert_eq!(
                        (
                            f.lp_fee_bps as u128,
                            f.protocol_fee_bps as u128,
                            f.creator_fee_bps as u128
                        ),
                        ev_bps,
                        "{sig}: {what} fee tier bps (tier={})",
                        f.fee_tier
                    );
                };
                if !is_buy {
                    let q = pool_sell(&pool, &sched.pool, base)
                        .unwrap_or_else(|err| panic!("{sig}: {err}"));
                    assert_eq!(q.quote_out_gross, quote, "{sig}: sell gross");
                    check_fees(&q.fees, "sell");
                    assert_eq!(q.quote_out_net, user_quote, "{sig}: sell net to user");
                    t.amm_sell += 1;
                } else if s(e, "ix_name") == "buy_exact_quote_in" {
                    // quote_amount_in is the fee-inclusive budget; user_quote_amount_in is the
                    // effective swap quote the program derived from it.
                    let q = pool_buy_exact_quote_in(&pool, &sched.pool, quote)
                        .unwrap_or_else(|err| panic!("{sig}: {err}"));
                    assert_eq!(q.base_amount_out, base, "{sig}: exact-in base out");
                    assert_eq!(
                        q.quote_amount_in, user_quote,
                        "{sig}: exact-in effective quote"
                    );
                    check_fees(&q.fees, "buy_exact_quote_in");
                    assert!(q.total_quote <= quote, "{sig}: exact-in overspent budget");
                    t.amm_buy_exact_quote_in += 1;
                } else {
                    let q = pool_buy_cost_for_base(&pool, &sched.pool, base)
                        .unwrap_or_else(|err| panic!("{sig}: {err}"));
                    assert_eq!(q.quote_amount_in, quote, "{sig}: buy quote_in");
                    check_fees(&q.fees, "buy");
                    assert_eq!(q.total_quote, user_quote, "{sig}: buy total paid by user");
                    t.amm_buy += 1;
                }
            }
            other => panic!("unknown protocol {other}"),
        }
    }
    eprintln!("golden parity tally: {t:?}");
    assert!(t.curve_buy >= 10 && t.curve_sell >= 20 && t.curve_buy_exact_in >= 5);
    assert!(t.amm_sell >= 10 && t.amm_buy_exact_quote_in >= 5 && t.amm_buy >= 1);
    assert!(
        t.amm_boost_pool >= 1,
        "expected at least one boost pool in fixtures"
    );
    assert!(
        t.curve_cashback >= 1,
        "expected at least one cashback coin in fixtures"
    );
    assert!(
        t.curve_mayhem_retail >= 1,
        "expected a fee-bearing mayhem trade in fixtures"
    );
}
