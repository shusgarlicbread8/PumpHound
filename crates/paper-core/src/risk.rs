use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskGates {
    pub max_token_cost_basis_fraction: f64,
    pub max_gross_exposure_fraction: f64,
    pub daily_loss_breaker_fraction: f64,
    pub peak_drawdown_breaker_fraction: f64,
    pub entries_frozen: bool,
    pub exits_active: bool,
}

impl Default for RiskGates {
    fn default() -> Self {
        Self {
            max_token_cost_basis_fraction: 0.05,
            max_gross_exposure_fraction: 0.15,
            daily_loss_breaker_fraction: 0.05,
            peak_drawdown_breaker_fraction: 0.15,
            entries_frozen: false,
            exits_active: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortfolioRisk {
    pub nav_atomic_usd_e6: i128,
    pub peak_nav_atomic_usd_e6: i128,
    pub daily_pnl_atomic_usd_e6: i128,
    pub gross_exposure_fraction: f64,
    pub token_cost_basis_fraction: f64,
}

pub fn admit_entry(gates: &RiskGates, risk: &PortfolioRisk) -> Result<(), String> {
    if gates.entries_frozen {
        return Err("entries frozen".into());
    }
    if risk.token_cost_basis_fraction > gates.max_token_cost_basis_fraction {
        return Err("max_token_cost_basis".into());
    }
    if risk.gross_exposure_fraction > gates.max_gross_exposure_fraction {
        return Err("max_gross_exposure".into());
    }
    if risk.peak_nav_atomic_usd_e6 > 0 {
        let dd = (risk.peak_nav_atomic_usd_e6 - risk.nav_atomic_usd_e6) as f64
            / risk.peak_nav_atomic_usd_e6 as f64;
        if dd > gates.peak_drawdown_breaker_fraction {
            return Err("drawdown_breaker".into());
        }
    }
    if risk.nav_atomic_usd_e6 > 0 {
        let loss = (-risk.daily_pnl_atomic_usd_e6).max(0) as f64 / risk.nav_atomic_usd_e6 as f64;
        if loss > gates.daily_loss_breaker_fraction {
            return Err("daily_loss_breaker".into());
        }
    }
    Ok(())
}
