import { normalizePortfolio } from "./portfolioView";
import { aggregateGraphEdges } from "./graphAggregate";
import { formatQuoteMoney } from "./money";
import { strategyMetrics } from "./strategyMetrics";

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error(msg);
}

const missingRisk = normalizePortfolio({
  portfolio_id: "p1",
  name: "desk",
  orders: null,
  positions: null,
});
assert(missingRisk.riskState === "UNKNOWN", "missing risk must be UNKNOWN");
assert(missingRisk.navUsd.state === "UNKNOWN", "missing NAV must be UNKNOWN");
assert(missingRisk.orders.length === 0, "null orders become empty");
assert(missingRisk.positions.length === 0, "null positions become empty");

const edges = aggregateGraphEdges([
  { source: "wallet:a", target: "token:x", kind: "BOUGHT", weight: 2, count: 1 },
  { source: "wallet:a", target: "token:x", kind: "SOLD", weight: 1, count: 1 },
]);
assert(edges.length === 1, "BOUGHT+SOLD must be one rendered pair");
assert((edges[0].count as number) === 2, "pair count must sum");

const money = formatQuoteMoney("509215830");
assert(money.display.includes("SOL"), `P&L must show SOL, got ${money.display}`);
assert(!/^[+-]?\d+$/.test(money.display), "must not leak raw atomic integers");

const metrics = strategyMetrics({
  strategy_key: "INFORMATION_VALUE_EXPLORER_V1",
  report: { closed_trades: 356, net_pnl: "-3893652" },
});
assert(metrics.closed === 356, "nested closed_trades must flatten");
assert(metrics.netPnlDisplay.includes("SOL"), "strategy P&L must be labelled SOL");

console.log("truth-layer frontend contracts passed");
