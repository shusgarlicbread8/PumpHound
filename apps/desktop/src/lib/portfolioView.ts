import { asArray, asNumber, asRecord, asString } from "./safe";
import type { PaperPortfolio, PortfolioRisk, RiskGates } from "./types";

export type FieldState = "KNOWN" | "UNKNOWN" | "NOT_APPLICABLE" | "PENDING";

export type KnownNumber = { state: "KNOWN"; value: number } | { state: Exclude<FieldState, "KNOWN"> };

export type NormalizedPortfolio = {
  portfolio_id: string;
  name: string;
  mode: string;
  policy_version: string;
  strategy: string | null;
  sol_atomic: string | null;
  usdc_atomic: string | null;
  reserved_sol: string | null;
  positions: unknown[];
  orders: unknown[];
  pending_orders: unknown[];
  balances: unknown[];
  risk: PortfolioRisk | null;
  gates: RiskGates | null;
  riskState: FieldState;
  gatesState: FieldState;
  navUsd: KnownNumber;
  grossExposure: KnownNumber;
  tokenCostFrac: KnownNumber;
  entriesFrozen: boolean | null;
  exitsActive: boolean | null;
};

function knownNumber(value: unknown): KnownNumber {
  const n = asNumber(value);
  return n == null ? { state: "UNKNOWN" } : { state: "KNOWN", value: n };
}

function optionalRisk(raw: unknown): { risk: PortfolioRisk | null; state: FieldState } {
  if (raw == null) return { risk: null, state: "UNKNOWN" };
  const rec = asRecord(raw);
  if (!Object.keys(rec).length) return { risk: null, state: "UNKNOWN" };
  return {
    state: "KNOWN",
    risk: {
      nav_atomic_usd_e6: rec.nav_atomic_usd_e6 as PortfolioRisk["nav_atomic_usd_e6"],
      peak_nav_atomic_usd_e6: rec.peak_nav_atomic_usd_e6 as PortfolioRisk["peak_nav_atomic_usd_e6"],
      daily_pnl_atomic_usd_e6: rec.daily_pnl_atomic_usd_e6 as PortfolioRisk["daily_pnl_atomic_usd_e6"],
      gross_exposure_fraction: asNumber(rec.gross_exposure_fraction) ?? Number.NaN,
      token_cost_basis_fraction: asNumber(rec.token_cost_basis_fraction) ?? Number.NaN,
    },
  };
}

function optionalGates(raw: unknown): { gates: RiskGates | null; state: FieldState } {
  if (raw == null) return { gates: null, state: "UNKNOWN" };
  const rec = asRecord(raw);
  if (!Object.keys(rec).length) return { gates: null, state: "UNKNOWN" };
  const entries = rec.entries_frozen;
  const exits = rec.exits_active;
  return {
    state: entries === undefined && exits === undefined ? "UNKNOWN" : "KNOWN",
    gates: {
      max_token_cost_basis_fraction: asNumber(rec.max_token_cost_basis_fraction) ?? Number.NaN,
      max_gross_exposure_fraction: asNumber(rec.max_gross_exposure_fraction) ?? Number.NaN,
      daily_loss_breaker_fraction: asNumber(rec.daily_loss_breaker_fraction) ?? Number.NaN,
      peak_drawdown_breaker_fraction: asNumber(rec.peak_drawdown_breaker_fraction) ?? Number.NaN,
      entries_frozen: typeof entries === "boolean" ? entries : false,
      exits_active: typeof exits === "boolean" ? exits : true,
    },
  };
}

/** No unvalidated backend JSON reaches Risk/Orders/Positions renderers. */
export function normalizePortfolio(raw: unknown): NormalizedPortfolio {
  const rec = asRecord(raw);
  const { risk, state: riskState } = optionalRisk(rec.risk);
  const { gates, state: gatesState } = optionalGates(rec.gates);
  return {
    portfolio_id: asString(rec.portfolio_id, ""),
    name: asString(rec.name, "Paper book"),
    mode: asString(rec.mode, "UNKNOWN"),
    policy_version: asString(rec.policy_version, "UNKNOWN"),
    strategy: typeof rec.strategy === "string" ? rec.strategy : null,
    sol_atomic: rec.sol_atomic == null ? null : String(rec.sol_atomic),
    usdc_atomic: rec.usdc_atomic == null ? null : String(rec.usdc_atomic),
    reserved_sol: rec.reserved_sol == null ? null : String(rec.reserved_sol),
    positions: asArray(rec.positions),
    orders: asArray(rec.orders ?? rec.pending_orders),
    pending_orders: asArray(rec.pending_orders ?? rec.orders),
    balances: asArray(rec.balances),
    risk,
    gates,
    riskState,
    gatesState,
    navUsd: knownNumber(risk?.nav_atomic_usd_e6),
    grossExposure: knownNumber(risk?.gross_exposure_fraction),
    tokenCostFrac: knownNumber(risk?.token_cost_basis_fraction),
    entriesFrozen: typeof gates?.entries_frozen === "boolean" ? gates.entries_frozen : null,
    exitsActive: typeof gates?.exits_active === "boolean" ? gates.exits_active : null,
  };
}

export function asPaperPortfolio(raw: unknown): PaperPortfolio {
  const n = normalizePortfolio(raw);
  return {
    portfolio_id: n.portfolio_id,
    name: n.name,
    mode: n.mode,
    sol_atomic: n.sol_atomic ?? "0",
    usdc_atomic: n.usdc_atomic ?? "0",
    reserved_sol: n.reserved_sol ?? "0",
    positions: n.positions as PaperPortfolio["positions"],
    orders: n.orders,
    pending_orders: n.pending_orders,
    risk: n.risk ?? {
      nav_atomic_usd_e6: "",
      peak_nav_atomic_usd_e6: "",
      daily_pnl_atomic_usd_e6: "",
      gross_exposure_fraction: Number.NaN,
      token_cost_basis_fraction: Number.NaN,
    },
    gates: n.gates ?? {
      max_token_cost_basis_fraction: Number.NaN,
      max_gross_exposure_fraction: Number.NaN,
      daily_loss_breaker_fraction: Number.NaN,
      peak_drawdown_breaker_fraction: Number.NaN,
      entries_frozen: false,
      exits_active: true,
    },
    policy_version: n.policy_version,
  };
}
