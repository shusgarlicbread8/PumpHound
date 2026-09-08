use pumphound_domain::{AssetAmount, Atomic, FeeComponent, FeeKind, PaperError, Pubkey};

/// Versioned Pump.fun curve schedule (protocol 0.95% + creator 0.30% = 1.25%).
pub const FEE_SCHEDULE_VERSION: &str = "pump-curve-125bps-2026-09";
pub const DEFAULT_FEE_BPS: u64 = 95;
pub const DEFAULT_CREATOR_FEE_BPS: u64 = 30;

pub fn apply_curve_fees(
    gross: u128,
    mint: Pubkey,
    protocol_bps: u64,
    creator_bps: u64,
) -> Result<(u128, Vec<FeeComponent>), PaperError> {
    let proto = bps(gross, protocol_bps)?;
    let creator = bps(gross, creator_bps)?;
    let reserve = gross
        .checked_sub(proto + creator)
        .ok_or(PaperError::Arithmetic("fee exceeds input".into()))?;
    Ok((
        reserve,
        vec![
            component(FeeKind::Protocol, mint, proto),
            component(FeeKind::Creator, mint, creator),
        ],
    ))
}

fn bps(amount: u128, bps: u64) -> Result<u128, PaperError> {
    amount
        .checked_mul(bps as u128)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(PaperError::Arithmetic("bps".into()))
}

fn component(kind: FeeKind, mint: Pubkey, atomic: u128) -> FeeComponent {
    FeeComponent {
        kind,
        asset: AssetAmount {
            mint,
            atomic: Atomic(atomic),
            decimals: 9,
        },
        schedule_version: FEE_SCHEDULE_VERSION.into(),
        charged_on_failure: false,
    }
}
