import { asNumber, asRecord, asString, confidenceLabel } from "./safe";
import { formatQuoteMoneyFromReport } from "./money";
import { WSOL_MINT } from "./types";

export type StrategyMetrics = {
  key: string;
  closed: number | null;
  trades: number | null;
  netPnlDisplay: string;
  expectancyDisplay: string;
  drawdownDisplay: string;
  bookId: string;
  experimentId: string;
  open: number | null;
  sample: string;
  edge: string;
  authority: string;
  netPnlSol: number | null;
  grossPnlDisplay: string;
  feesDisplay: string;
  status: string;
  policy: string;
  ml: string;
  /// "legacy-paper-v1" or "execution-twin-v1.0.0"
  simulatorVersion: string;
  fidelity: string;
  netExLargestDisplay: string;
  netExTop1PctDisplay: string;
  top1ShareDisplay: string;
  headlineDependsOnLargest: boolean;
  fillRateDisplay: string;
  executionDragDisplay: string;
  profitFactorDisplay: string;
};

function nested(item: Record<string, unknown>, key: string): unknown {
  if (item[key] !== undefined && item[key] !== null) return item[key];
  const report = asRecord(item.report);
  return report[key];
}

export function strategyMetrics(row: unknown): StrategyMetrics {
  const item = asRecord(row);
  const closed = asNumber(nested(item, "closed_trades") ?? nested(item, "trades"));
  const units = asRecord(nested(item, "financial_units"));
  const money = (key: string) => {
    const structured = asRecord(nested(item, "money"))[key];
    if (structured != null) return formatQuoteMoneyFromReport(structured);
    const value = nested(item, key);
    if (units.metric_values === "SOL") return asNumber(value) == null ? "UNKNOWN" : `${asNumber(value)!.toFixed(6)} SOL`;
    return formatQuoteMoneyFromReport(value, WSOL_MINT);
  };
  const report = asRecord(item.report);
  const tail = asRecord(report.tail);
  const execution = asRecord(report.execution);
  const sol = (value: unknown) => (asNumber(value) == null ? "UNKNOWN" : `${asNumber(value)!.toFixed(6)} SOL`);
  const pct = (value: unknown) => (asNumber(value) == null ? "UNKNOWN" : `${(asNumber(value)! * 100).toFixed(1)}%`);
  const simulatorVersion = asString(nested(item, "simulator_version"), "legacy-paper-v1");
  return {
    simulatorVersion,
    fidelity: simulatorVersion.startsWith("execution-twin") ? "EXECUTION TWIN V1" : "LEGACY SIMULATOR",
    netExLargestDisplay: sol(tail.net_ex_largest_winner),
    netExTop1PctDisplay: sol(tail.net_ex_top_1pct_winners),
    top1ShareDisplay: pct(tail.top1_share_of_profit),
    headlineDependsOnLargest: tail.headline_depends_on_largest_winner === true,
    fillRateDisplay: execution.fill_rate == null ? (simulatorVersion.startsWith("execution-twin") ? "NO ATTEMPTS YET" : "ASSUMED 100% (legacy)") : pct(execution.fill_rate),
    executionDragDisplay: execution.execution_drag_sol_per_fill == null ? (simulatorVersion.startsWith("execution-twin") ? "NO FILLS YET" : "NOT MODELED (legacy)") : `${asNumber(execution.execution_drag_sol_per_fill)!.toFixed(6)} SOL/fill`,
    profitFactorDisplay: asNumber(tail.profit_factor) == null ? (asNumber(tail.n) ? "NO LOSSES" : "UNKNOWN") : asNumber(tail.profit_factor)!.toFixed(2),
    bookId: asString(item.book_id ?? item.simulation_run_id, ""),
    experimentId: asString(item.experiment_id, "UNKNOWN"),
    open: asNumber(nested(item, "open_positions")),
    sample: asString(nested(item, "sample_status"), confidenceLabel(closed)),
    edge: asString(nested(item, "edge_status"), "INCONCLUSIVE"),
    authority: asString(nested(item, "authority"), "UNKNOWN"),
    netPnlSol: units.metric_values === "SOL" ? asNumber(nested(item, "net_pnl")) : null,
    grossPnlDisplay: money("gross_pnl"),
    feesDisplay: money("fees"),
    key: asString(item.strategy_key ?? item.strategy ?? item.label),
    closed,
    trades: asNumber(nested(item, "trades")),
    netPnlDisplay: money("net_pnl"),
    expectancyDisplay: money("expectancy"),
    drawdownDisplay: money("max_drawdown"),
    status: asString(nested(item, "status") ?? item.ml_status, "UNKNOWN"),
    policy: asString(item.policy_version, "UNKNOWN"),
    ml: asString(item.ml_status ?? nested(item, "status"), "UNKNOWN"),
  };
}
