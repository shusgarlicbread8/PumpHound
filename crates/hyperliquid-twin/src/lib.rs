//! Hyperliquid paper Execution Twin. Venue-specific. No Pump/Solana assumptions.

use chrono::{DateTime, Utc};
use pumphound_venue_core::OptionalNumber;
use pumphound_venue_hyperliquid::{
    BookLevel, HipClass, L2Book, BASE_PERP_MAKER, BASE_PERP_TAKER, FEE_SCHEDULE_VERSION,
};
use serde::{Deserialize, Serialize};

pub const TWIN_VERSION: &str = "hl-twin-v1";
pub const QUEUE_MODEL: &str = "conservative_unobserved_queue";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Market,
    Limit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeInputs {
    pub class: HipClass,
    pub deployer_fee_scale: OptionalNumber,
    pub growth_mode: Option<String>,
    pub referral_discount: OptionalNumber,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeeQuote {
    pub schedule_version: String,
    pub taker_rate: f64,
    pub maker_rate: f64,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub coin: String,
    pub side: Side,
    pub order_type: OrderType,
    pub notional_usd: f64,
    pub limit_px: OptionalNumber,
    pub leverage: OptionalNumber,
    pub decision_ts: DateTime<Utc>,
    pub latency_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FillResult {
    pub status: FillStatus,
    pub filled_notional: OptionalNumber,
    pub unfilled_notional: OptionalNumber,
    pub avg_px: OptionalNumber,
    pub fee_usd: OptionalNumber,
    pub fee_rate: OptionalNumber,
    pub slip_from_mid: OptionalNumber,
    pub funding_carry_usd: OptionalNumber,
    pub twin_version: String,
    pub fee_schedule_version: String,
    pub queue_model: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FillStatus {
    Filled,
    Partial,
    Unfilled,
    RejectedStaleBook,
    RejectedInsufficientDepth,
    RejectedUnknownMid,
}

pub fn fee_quote(inputs: &FeeInputs) -> FeeQuote {
    let mut notes = vec![
        "paper only; no referral or aligned-quote discount applied unless supplied".into(),
        format!("schedule {FEE_SCHEDULE_VERSION}"),
    ];
    let mut taker = BASE_PERP_TAKER;
    let mut maker = BASE_PERP_MAKER;
    if inputs.class == HipClass::Hip3 {
        let scale = inputs.deployer_fee_scale.as_f64().unwrap_or(0.0);
        let hip_scale = if scale < 1.0 { scale + 1.0 } else { scale * 2.0 };
        let growth = if inputs
            .growth_mode
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("enabled") || s == "true")
            .unwrap_or(false)
        {
            0.1
        } else {
            1.0
        };
        taker *= hip_scale * growth;
        maker *= hip_scale * growth;
        notes.push(format!(
            "HIP-3 official scaleIfHip3={} growthModeScale={}",
            hip_scale, growth
        ));
    }
    if let Some(disc) = inputs.referral_discount.as_f64() {
        taker *= 1.0 - disc;
        if maker > 0.0 {
            maker *= 1.0 - disc;
        }
        notes.push("applied supplied referral discount".into());
    }
    FeeQuote {
        schedule_version: FEE_SCHEDULE_VERSION.into(),
        taker_rate: taker,
        maker_rate: maker,
        notes,
    }
}

pub fn simulate_taker(
    book: &L2Book,
    mid: OptionalNumber,
    intent: &OrderIntent,
    fees: &FeeQuote,
    book_age_ms: Option<i64>,
    stale_after_ms: i64,
    funding_rate: OptionalNumber,
    hold_hours: OptionalNumber,
) -> FillResult {
    let mut notes = fees.notes.clone();
    notes.push(format!("twin {TWIN_VERSION}"));
    notes.push("candle-touch is not a fill".into());
    if book_age_ms.is_some_and(|age| age > stale_after_ms) {
        return FillResult {
            status: FillStatus::RejectedStaleBook,
            filled_notional: OptionalNumber::unknown(),
            unfilled_notional: OptionalNumber::observed(intent.notional_usd),
            avg_px: OptionalNumber::unknown(),
            fee_usd: OptionalNumber::unknown(),
            fee_rate: OptionalNumber::unknown(),
            slip_from_mid: OptionalNumber::unknown(),
            funding_carry_usd: OptionalNumber::unknown(),
            twin_version: TWIN_VERSION.into(),
            fee_schedule_version: fees.schedule_version.clone(),
            queue_model: QUEUE_MODEL.into(),
            notes: {
                notes.push("arrival book exceeded stale threshold".into());
                notes
            },
        };
    }
    let Some(mid) = mid.as_f64().filter(|m| *m > 0.0) else {
        return FillResult {
            status: FillStatus::RejectedUnknownMid,
            filled_notional: OptionalNumber::unknown(),
            unfilled_notional: OptionalNumber::observed(intent.notional_usd),
            avg_px: OptionalNumber::unknown(),
            fee_usd: OptionalNumber::unknown(),
            fee_rate: OptionalNumber::unknown(),
            slip_from_mid: OptionalNumber::unknown(),
            funding_carry_usd: OptionalNumber::unknown(),
            twin_version: TWIN_VERSION.into(),
            fee_schedule_version: fees.schedule_version.clone(),
            queue_model: QUEUE_MODEL.into(),
            notes: {
                notes.push("mid unknown; missing is not zero".into());
                notes
            },
        };
    };
    if intent.order_type == OrderType::Limit {
        notes.push(QUEUE_MODEL.into());
        notes.push("unobserved queue: resting limit does not fill without tape evidence".into());
        return FillResult {
            status: FillStatus::Unfilled,
            filled_notional: OptionalNumber::observed(0.0),
            unfilled_notional: OptionalNumber::observed(intent.notional_usd),
            avg_px: OptionalNumber::unknown(),
            fee_usd: OptionalNumber::observed(0.0),
            fee_rate: OptionalNumber::observed(fees.maker_rate),
            slip_from_mid: OptionalNumber::unknown(),
            funding_carry_usd: OptionalNumber::observed(0.0),
            twin_version: TWIN_VERSION.into(),
            fee_schedule_version: fees.schedule_version.clone(),
            queue_model: QUEUE_MODEL.into(),
            notes,
        };
    }
    let levels = match intent.side {
        Side::Buy => &book.asks,
        Side::Sell => &book.bids,
    };
    let walk = walk_notional(levels, intent.notional_usd);
    if walk.filled <= 0.0 {
        return FillResult {
            status: FillStatus::RejectedInsufficientDepth,
            filled_notional: OptionalNumber::observed(0.0),
            unfilled_notional: OptionalNumber::observed(intent.notional_usd),
            avg_px: OptionalNumber::unknown(),
            fee_usd: OptionalNumber::unknown(),
            fee_rate: OptionalNumber::observed(fees.taker_rate),
            slip_from_mid: OptionalNumber::unknown(),
            funding_carry_usd: OptionalNumber::unknown(),
            twin_version: TWIN_VERSION.into(),
            fee_schedule_version: fees.schedule_version.clone(),
            queue_model: QUEUE_MODEL.into(),
            notes: {
                notes.push("no executable L2 notional".into());
                notes
            },
        };
    }
    let status = if walk.unfilled > 1e-9 {
        FillStatus::Partial
    } else {
        FillStatus::Filled
    };
    let fee = walk.filled * fees.taker_rate;
    let slip = (walk.avg_px - mid).abs() / mid;
    let funding = match (funding_rate.as_f64(), hold_hours.as_f64()) {
        (Some(rate), Some(hours)) => OptionalNumber::observed(walk.filled * rate * (hours / 1.0)),
        _ => OptionalNumber::unknown(),
    };
    notes.push("funding_carry uses supplied hourly-equivalent rate * hold_hours when both known".into());
    FillResult {
        status,
        filled_notional: OptionalNumber::observed(walk.filled),
        unfilled_notional: OptionalNumber::observed(walk.unfilled),
        avg_px: OptionalNumber::observed(walk.avg_px),
        fee_usd: OptionalNumber::observed(fee),
        fee_rate: OptionalNumber::observed(fees.taker_rate),
        slip_from_mid: OptionalNumber::observed(slip),
        funding_carry_usd: funding,
        twin_version: TWIN_VERSION.into(),
        fee_schedule_version: fees.schedule_version.clone(),
        queue_model: QUEUE_MODEL.into(),
        notes,
    }
}

struct Walk {
    filled: f64,
    unfilled: f64,
    avg_px: f64,
}

fn walk_notional(levels: &[BookLevel], target: f64) -> Walk {
    let mut remaining = target;
    let mut notional = 0.0;
    let mut qty = 0.0;
    for level in levels {
        let (Some(px), Some(sz)) = (level.px.as_f64(), level.sz.as_f64()) else {
            continue;
        };
        if px <= 0.0 || sz <= 0.0 {
            continue;
        }
        let available = px * sz;
        let take = available.min(remaining);
        notional += take;
        qty += take / px;
        remaining -= take;
        if remaining <= 1e-9 {
            break;
        }
    }
    Walk {
        filled: notional,
        unfilled: remaining.max(0.0),
        avg_px: if qty > 0.0 { notional / qty } else { 0.0 },
    }
}

pub fn net_funding_result(
    gross_funding: OptionalNumber,
    entry_cost: OptionalNumber,
    exit_cost: OptionalNumber,
    hedge_fees: OptionalNumber,
    hedge_spread: OptionalNumber,
    basis_pnl: OptionalNumber,
) -> OptionalNumber {
    let parts = [
        gross_funding,
        entry_cost,
        exit_cost,
        hedge_fees,
        hedge_spread,
        basis_pnl,
    ];
    if parts.iter().any(|p| p.is_unknown()) {
        return OptionalNumber::unknown();
    }
    OptionalNumber::observed(
        gross_funding.as_f64().unwrap_or(0.0)
            - entry_cost.as_f64().unwrap_or(0.0)
            - exit_cost.as_f64().unwrap_or(0.0)
            - hedge_fees.as_f64().unwrap_or(0.0)
            - hedge_spread.as_f64().unwrap_or(0.0)
            + basis_pnl.as_f64().unwrap_or(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn book(ask_sz: f64) -> L2Book {
        L2Book {
            coin: "BTC".into(),
            source_ts: None,
            bids: vec![BookLevel {
                px: OptionalNumber::observed(100.0),
                sz: OptionalNumber::observed(10.0),
                n: Some(1),
            }],
            asks: vec![BookLevel {
                px: OptionalNumber::observed(101.0),
                sz: OptionalNumber::observed(ask_sz),
                n: Some(1),
            }],
        }
    }

    fn intent(notional: f64, order_type: OrderType) -> OrderIntent {
        OrderIntent {
            coin: "BTC".into(),
            side: Side::Buy,
            order_type,
            notional_usd: notional,
            limit_px: OptionalNumber::observed(101.0),
            leverage: OptionalNumber::observed(2.0),
            decision_ts: Utc.with_ymd_and_hms(2026, 9, 10, 0, 0, 0).unwrap(),
            latency_ms: 80,
        }
    }

    #[test]
    fn candle_touch_limit_does_not_fill() {
        let fees = fee_quote(&FeeInputs {
            class: HipClass::Native,
            deployer_fee_scale: OptionalNumber::unknown(),
            growth_mode: None,
            referral_discount: OptionalNumber::unknown(),
        });
        let fill = simulate_taker(
            &book(100.0),
            OptionalNumber::observed(100.5),
            &intent(1_000.0, OrderType::Limit),
            &fees,
            Some(10),
            5_000,
            OptionalNumber::unknown(),
            OptionalNumber::unknown(),
        );
        assert_eq!(fill.status, FillStatus::Unfilled);
        assert_eq!(fill.filled_notional.as_f64(), Some(0.0));
    }

    #[test]
    fn insufficient_depth_is_partial_or_reject_not_full_fill() {
        let fees = fee_quote(&FeeInputs {
            class: HipClass::Native,
            deployer_fee_scale: OptionalNumber::unknown(),
            growth_mode: None,
            referral_discount: OptionalNumber::unknown(),
        });
        let fill = simulate_taker(
            &book(1.0),
            OptionalNumber::observed(100.5),
            &intent(10_000.0, OrderType::Market),
            &fees,
            Some(10),
            5_000,
            OptionalNumber::unknown(),
            OptionalNumber::unknown(),
        );
        assert_ne!(fill.status, FillStatus::Filled);
        assert!(fill.filled_notional.as_f64().unwrap() < 10_000.0);
    }

    #[test]
    fn unknown_mid_is_not_zero() {
        let fees = fee_quote(&FeeInputs {
            class: HipClass::Native,
            deployer_fee_scale: OptionalNumber::unknown(),
            growth_mode: None,
            referral_discount: OptionalNumber::unknown(),
        });
        let fill = simulate_taker(
            &book(10.0),
            OptionalNumber::unknown(),
            &intent(500.0, OrderType::Market),
            &fees,
            Some(10),
            5_000,
            OptionalNumber::unknown(),
            OptionalNumber::unknown(),
        );
        assert_eq!(fill.status, FillStatus::RejectedUnknownMid);
        assert!(fill.avg_px.is_unknown());
    }

    #[test]
    fn hip3_growth_mode_uses_official_0_1_multiplier() {
        let native = fee_quote(&FeeInputs {
            class: HipClass::Native,
            deployer_fee_scale: OptionalNumber::unknown(),
            growth_mode: None,
            referral_discount: OptionalNumber::unknown(),
        });
        let hip = fee_quote(&FeeInputs {
            class: HipClass::Hip3,
            deployer_fee_scale: OptionalNumber::observed(0.5),
            growth_mode: Some("enabled".into()),
            referral_discount: OptionalNumber::unknown(),
        });
        assert!((native.taker_rate - 0.00045).abs() < 1e-12);
        // scale 0.5 => scaleIfHip3=1.5, growth=0.1 => 0.00045*1.5*0.1
        assert!((hip.taker_rate - 0.0000675).abs() < 1e-12);
    }

    #[test]
    fn missing_funding_inputs_keep_net_unknown() {
        let net = net_funding_result(
            OptionalNumber::observed(10.0),
            OptionalNumber::observed(1.0),
            OptionalNumber::unknown(),
            OptionalNumber::observed(0.0),
            OptionalNumber::observed(0.0),
            OptionalNumber::observed(0.0),
        );
        assert!(net.is_unknown());
    }

    #[test]
    fn stale_book_rejects() {
        let fees = fee_quote(&FeeInputs {
            class: HipClass::Native,
            deployer_fee_scale: OptionalNumber::unknown(),
            growth_mode: None,
            referral_discount: OptionalNumber::unknown(),
        });
        let fill = simulate_taker(
            &book(10.0),
            OptionalNumber::observed(100.5),
            &intent(100.0, OrderType::Market),
            &fees,
            Some(30_000),
            5_000,
            OptionalNumber::unknown(),
            OptionalNumber::unknown(),
        );
        assert_eq!(fill.status, FillStatus::RejectedStaleBook);
    }
}
