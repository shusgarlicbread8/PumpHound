//! Rehydrate market state for open paper positions whose token has gone cold.
//!
//! The hot market reducer is in-memory and rebuilt from the live event stream. A token that
//! stopped trading before a `core-api` restart never re-enters it, so any open paper position in
//! that token could never be exited: `scan_learning_exits` requires a snapshot, found none, and
//! skipped the position forever. Books then hit `position_capacity_reached` and silently stopped
//! trading (observed: RANDOM_CONTROL_V1 with 17 open positions against a cap of 5 and zero
//! entries for 17 hours).
//!
//! This module reads the *current* on-chain account state (paper-only RPC read — no signing,
//! no submission) and turns it into a `MarketSnapshot`. A Pump bonding curve's reserves change
//! only through trades, so reading the account is the exact current state, not a fabrication.
//!
//! Scope: Pump bonding curves only. PumpSwap pools need token-account balance reads plus pool
//! decoding; positions there stay `STRANDED` with an explicit reason instead of a made-up mark.

use pumphound_domain::{
    Atomic, Canonicality, Commitment, Coverage, DataQuality, MarketSnapshot, ObservationPoint,
    Protocol, Pubkey, QualityGrade,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Anchor account discriminator for `BondingCurve` (vendor/pump-idl/pump.json).
const BONDING_CURVE_DISC: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];
const ONE_BILLION_SUPPLY: u128 = 1_000_000_000_000_000;
const MIN_REFETCH_INTERVAL: Duration = Duration::from_secs(45);

#[derive(Debug, Clone)]
pub enum Rehydrated {
    /// Current on-chain curve state, quotable.
    Curve(MarketSnapshot),
    /// Curve is `complete`: liquidity migrated to PumpSwap; the curve account can no longer be
    /// traded and the pool has not been resolved. Not a fabricated price — an explicit gap.
    Graduated { market: Pubkey },
    /// PumpSwap pool positions are not rehydrated in this pass.
    UnsupportedProtocol(Protocol),
    /// Rate limited / RPC failure / account missing. Reason is kept for the trace.
    Unavailable(String),
}

impl Rehydrated {
    pub fn reason(&self) -> String {
        match self {
            Rehydrated::Curve(_) => "rehydrated_from_chain_account".into(),
            Rehydrated::Graduated { .. } => "curve_complete_pool_unresolved".into(),
            Rehydrated::UnsupportedProtocol(p) => format!("rehydrate_unsupported_{p:?}").to_lowercase(),
            Rehydrated::Unavailable(r) => format!("rehydrate_unavailable:{r}"),
        }
    }
}

fn throttle() -> &'static Mutex<HashMap<String, Instant>> {
    static T: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rpc_url() -> String {
    pumphound_chain_source::SourceConfig::from_env().rpc_url
}

/// Fetch the current on-chain state for a position's market. Throttled per market so a stuck
/// book cannot turn into an RPC loop.
pub async fn rehydrate(
    client: &reqwest::Client,
    protocol: Protocol,
    market: Pubkey,
    mint: Pubkey,
    quote_mint: Pubkey,
) -> Rehydrated {
    if protocol != Protocol::PumpCurve {
        return Rehydrated::UnsupportedProtocol(protocol);
    }
    // Positions opened from a create-event snapshot can carry `market == mint`; the curve PDA
    // is a pure function of the mint, so derive it rather than trusting the stored column.
    let curve = pumphound_execution_twin::pda::pump::bonding_curve(&mint);
    let market = if market == mint { curve } else { market };
    let key = market.to_string();
    {
        let mut t = throttle().lock().expect("rehydrate throttle");
        if let Some(last) = t.get(&key) {
            if last.elapsed() < MIN_REFETCH_INTERVAL {
                return Rehydrated::Unavailable("throttled".into());
            }
        }
        t.insert(key.clone(), Instant::now());
    }
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "getAccountInfo",
        "params": [key, {"encoding": "base64", "commitment": "confirmed"}]
    });
    let response = match client.post(rpc_url()).json(&body).send().await {
        Ok(r) => r,
        Err(e) => return Rehydrated::Unavailable(format!("rpc_transport:{e}")),
    };
    let value: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => return Rehydrated::Unavailable(format!("rpc_decode:{e}")),
    };
    let slot = value["result"]["context"]["slot"].as_u64().unwrap_or(0);
    let Some(data_b64) = value["result"]["value"]["data"][0].as_str() else {
        return Rehydrated::Unavailable("account_missing".into());
    };
    let Ok(data) = base64_decode(data_b64) else {
        return Rehydrated::Unavailable("account_data_not_base64".into());
    };
    match snapshot_from_curve_account(&data, market, mint, quote_mint, slot) {
        Some(snap) if snap.complete => Rehydrated::Graduated { market },
        Some(snap) => Rehydrated::Curve(snap),
        None => Rehydrated::Unavailable("not_a_bonding_curve_account".into()),
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|_| ())
}

fn le_u64(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off + 8)
        .map(|s| u64::from_le_bytes(s.try_into().expect("8 bytes")))
}

/// Decode a `BondingCurve` account into a `MarketSnapshot` using the same layout the golden /
/// Surfpool tests decode (`crates/execution-twin/tests/surfpool_integration.rs`).
pub fn snapshot_from_curve_account(
    data: &[u8],
    market: Pubkey,
    mint: Pubkey,
    quote_mint: Pubkey,
    slot: u64,
) -> Option<MarketSnapshot> {
    if data.len() < 8 + 75 || data[..8] != BONDING_CURVE_DISC {
        return None;
    }
    let b = &data[8..];
    let virtual_token = le_u64(b, 0)? as u128;
    let virtual_sol = le_u64(b, 8)? as u128;
    let real_token = le_u64(b, 16)? as u128;
    let real_sol = le_u64(b, 24)? as u128;
    let complete = b[40] != 0;
    let creator = Pubkey(b[41..73].try_into().ok()?);
    let is_mayhem_mode = b[73] != 0;
    let cashback = b[74] != 0;
    let now = pumphound_domain::now_utc();
    let mut quality = DataQuality::live(0, 0, true);
    quality.grade = QualityGrade::B;
    quality.coverage = Coverage::CompleteForScope;
    quality
        .reasons
        .push("rehydrated from on-chain BondingCurve account (RPC read, paper-only)".into());
    Some(MarketSnapshot {
        snapshot_id: uuid::Uuid::now_v7().to_string(),
        market,
        mint,
        quote_mint,
        protocol: Protocol::PumpCurve,
        as_of: ObservationPoint {
            slot: pumphound_domain::Slot(slot),
            tx_index: None,
            event_index: None,
            event_time: None,
            observed_at: now,
            knowledge_cutoff: now,
            commitment: Commitment::Confirmed,
        },
        canonicality: Canonicality::Canonical,
        virtual_base_atomic: Atomic(virtual_token),
        virtual_quote_atomic: Atomic(virtual_sol),
        real_base_atomic: Atomic(real_token),
        real_quote_atomic: Atomic(real_sol),
        effective_base_atomic: Atomic(virtual_token),
        effective_quote_atomic: Atomic(virtual_sol),
        complete,
        migrated_market: None,
        config_snapshot_id: "rehydrated_from_chain_account".into(),
        quality,
        is_mayhem_mode: Some(is_mayhem_mode),
        creator_fee_applies: Some(creator != Pubkey([0u8; 32])),
        base_supply_atomic: Some(Atomic(ONE_BILLION_SUPPLY)),
        cashback_fee_bps: Some(if cashback { 30 } else { 0 }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve_bytes(complete: bool) -> Vec<u8> {
        let mut d = BONDING_CURVE_DISC.to_vec();
        d.extend_from_slice(&1_000u64.to_le_bytes()); // virtual token
        d.extend_from_slice(&2_000u64.to_le_bytes()); // virtual sol
        d.extend_from_slice(&300u64.to_le_bytes()); // real token
        d.extend_from_slice(&400u64.to_le_bytes()); // real sol
        d.extend_from_slice(&ONE_BILLION_SUPPLY.to_le_bytes()[..8]); // token_total_supply
        d.push(complete as u8);
        d.extend_from_slice(&[7u8; 32]); // creator
        d.push(1); // mayhem
        d.push(1); // cashback
        d
    }

    #[test]
    fn decodes_bonding_curve_account_into_exact_snapshot() {
        let snap = snapshot_from_curve_account(
            &curve_bytes(false),
            Pubkey([1; 32]),
            Pubkey([2; 32]),
            Pubkey([3; 32]),
            42,
        )
        .expect("decodes");
        assert_eq!(snap.virtual_base_atomic, Atomic(1_000));
        assert_eq!(snap.virtual_quote_atomic, Atomic(2_000));
        assert_eq!(snap.real_base_atomic, Atomic(300));
        assert_eq!(snap.real_quote_atomic, Atomic(400));
        assert!(!snap.complete);
        assert_eq!(snap.is_mayhem_mode, Some(true));
        assert_eq!(snap.cashback_fee_bps, Some(30));
        assert_eq!(snap.creator_fee_applies, Some(true));
        assert_eq!(snap.as_of.slot.0, 42);
        assert_eq!(snap.config_snapshot_id, "rehydrated_from_chain_account");
    }

    #[test]
    fn graduated_curve_is_flagged_not_priced() {
        let snap = snapshot_from_curve_account(
            &curve_bytes(true),
            Pubkey([1; 32]),
            Pubkey([2; 32]),
            Pubkey([3; 32]),
            1,
        )
        .expect("decodes");
        assert!(snap.complete);
    }

    #[test]
    fn rejects_foreign_account_data() {
        let mut d = curve_bytes(false);
        d[0] ^= 0xff;
        assert!(snapshot_from_curve_account(&d, Pubkey([1; 32]), Pubkey([2; 32]), Pubkey([3; 32]), 1).is_none());
        assert!(snapshot_from_curve_account(&d[..20], Pubkey([1; 32]), Pubkey([2; 32]), Pubkey([3; 32]), 1).is_none());
    }
}
