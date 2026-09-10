import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../lib/api";
import { asArray, asRecord, asString } from "../../lib/safe";

const PAIRS: Array<[string, string, string]> = [
  ["RANDOM_SELECT_V1", "MARKET_CONFIRMED_V1", "Does microstructure beat random on the same twin window?"],
  ["MARKET_CONFIRMED_V1", "LOSS_FILTER_V1", "Does toxicity filtering remove losers without killing the tail?"],
  ["RANDOM_SELECT_V1", "IV_SELECT_V1", "Does information-value selection survive realistic execution?"],
  ["MARKET_CONFIRMED_V1", "SURVIVOR_PICKER_V1", "Same rule, later window — timing confounder by design"],
  ["INFORMATION_VALUE_EXPLORER_V1", "RANDOM_CONTROL_V1", "Legacy IV vs random (legacy simulator)"],
];

export function SelectionLab() {
  const [pair, setPair] = useState(0);
  const [a, b, question] = PAIRS[pair] ?? PAIRS[0];
  const diff = useQuery({
    queryKey: ["strategy-diff", a, b],
    queryFn: () => api.researchStrategyDiff(a, b),
    refetchInterval: 30_000,
  });
  const forensics = useQuery({
    queryKey: ["selection-forensics"],
    queryFn: api.researchSelectionForensics,
    refetchInterval: 30_000,
  });
  const board = useQuery({
    queryKey: ["exec-select"],
    queryFn: api.researchExecSelect,
    refetchInterval: 15_000,
  });
  const diffData = asRecord(diff.data);
  const status = asString(diffData.status, "UNKNOWN");
  const same = asArray<string>(diffData.same);
  const different = asArray<string>(diffData.different);
  const confounders = asArray<string>(diffData.confounders);
  const forensic = asRecord(forensics.data);
  const books = asArray(forensic.books);
  const contrasts = asArray(forensic.contrasts);
  const boardItems = asArray(asRecord(board.data).items);

  return (
    <section className="panel mb-3 overflow-hidden">
      <div className="border-b border-hairline px-2 py-1.5">
        <h2 className="text-header">What actually improves selection?</h2>
        <p className="text-label text-muted">
          PumpHound is a picker, not a sniper. New books run on Execution Twin only. Frozen books stay frozen.
        </p>
      </div>

      <div className="grid gap-3 border-b border-hairline p-2 md:grid-cols-2">
        <div>
          <div className="label">EXEC-SELECT-V1 votes</div>
          {boardItems.length === 0 ? (
            <p className="text-label text-muted">No EXEC-SELECT decisions yet. Books appear after the next scan.</p>
          ) : (
            <table className="w-full text-left text-label">
              <thead className="text-muted">
                <tr>
                  <th className="font-normal">Book</th>
                  <th className="font-normal">BUY</th>
                  <th className="font-normal">WATCH</th>
                  <th className="font-normal">AVOID</th>
                </tr>
              </thead>
              <tbody>
                {boardItems.map((row, i) => {
                  const item = asRecord(row);
                  return (
                    <tr key={i} className="border-t border-hairline">
                      <td className="font-mono">{asString(item.strategy_version)}</td>
                      <td className="tabular-nums">{asString(item.buys, "0")}</td>
                      <td className="tabular-nums">{asString(item.watches, "0")}</td>
                      <td className="tabular-nums">{asString(item.avoids, "0")}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>
        <div>
          <div className="label">Strategy Diff</div>
          <select
            className="mb-2 w-full border border-hairline bg-surface1 px-2 py-1 text-label"
            value={pair}
            onChange={(event) => setPair(Number(event.target.value))}
          >
            {PAIRS.map((item, i) => (
              <option key={i} value={i}>
                {item[0]} vs {item[1]}
              </option>
            ))}
          </select>
          <p className="text-label text-muted">{question}</p>
          <p className={`mt-1 font-mono text-label ${status === "COMPARABLE" ? "text-positive" : "text-amber"}`}>
            {status}
          </p>
          <div className="mt-1 text-label">
            SAME {same.join(" · ") || "none"}
          </div>
          <div className="text-label">
            DIFFERENT {different.join(" · ") || "none"}
          </div>
          {confounders.length ? (
            <div className="text-label text-amber">CONFOUNDED {confounders.join(" · ")}</div>
          ) : null}
        </div>
      </div>

      <div className="p-2">
        <div className="label">Why does Information Value select different tokens?</div>
        <p className="text-label text-muted">
          Entry-time medians on BUY decisions. Null means the feature was absent — never substituted with zero.
        </p>
        {books.length === 0 ? (
          <p className="mt-2 text-label text-muted">Forensics appear after BUY decisions exist.</p>
        ) : (
          <div className="mt-2 overflow-auto">
            <table className="w-full min-w-[720px] text-left text-label">
              <thead className="text-muted">
                <tr>
                  <th className="font-normal">Book</th>
                  <th className="font-normal">N</th>
                  {["age_seconds", "unique_buyers_30", "buy_sell_imbalance_30", "return_30", "holder_top_1_share", "same_slot_first10_share"].map((h) => (
                    <th key={h} className="font-normal">{h}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {books.map((row, i) => {
                  const item = asRecord(row);
                  const med = asRecord(item.entry_medians);
                  return (
                    <tr key={i} className="border-t border-hairline">
                      <td className="font-mono">{asString(item.strategy_version)}</td>
                      <td className="tabular-nums">{asString(item.n)}</td>
                      {["age_seconds", "unique_buyers_30", "buy_sell_imbalance_30", "return_30", "holder_top_1_share", "same_slot_first10_share"].map((k) => (
                        <td key={k} className="font-mono">{asString(med[k], "UNKNOWN")}</td>
                      ))}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        {contrasts.length ? (
          <details className="mt-2">
            <summary className="cursor-pointer text-label text-info">IV minus other book (median deltas)</summary>
            <pre className="mt-2 max-h-56 overflow-auto text-[11px]">{JSON.stringify(contrasts, null, 2)}</pre>
          </details>
        ) : null}
      </div>
    </section>
  );
}
