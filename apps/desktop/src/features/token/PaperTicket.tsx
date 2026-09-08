import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { EmptyState } from "../../components/EmptyState";
import { api, ApiRequestError } from "../../lib/api";
import { newId } from "../../lib/format";
import { parseDecimalToAtomic, quoteDecimals } from "../../lib/units";
import type { LaunchDetail, PaperOrderRequest, TradeSide } from "../../lib/types";
import { usePaper } from "../../stores/paper";
import { CostPreview } from "../paper/CostPreview";

export function PaperTicket({
  detail,
  side,
}: {
  detail: LaunchDetail;
  side: TradeSide;
}) {
  const activeId = usePaper((s) => s.activeId);
  const ensure = usePaper((s) => s.ensureDefault);
  const [amount, setAmount] = useState("0.1");
  const [slip, setSlip] = useState("100");
  const [err, setErr] = useState<string | null>(null);

  const market = detail.market?.market;
  const inputMint = side === "buy" ? detail.quote_mint : detail.mint;
  const outputMint = side === "buy" ? detail.mint : detail.quote_mint;
  const decimals = quoteDecimals(inputMint);

  const buildReq = async (): Promise<PaperOrderRequest> => {
    const portfolio = activeId ?? (await ensure());
    if (!portfolio) throw new Error("no paper portfolio");
    if (!market) throw new Error("no market public key on launch snapshot");
    const atomic = parseDecimalToAtomic(amount, decimals);
    return {
      client_request_id: newId(),
      portfolio_id: portfolio,
      launch_id: detail.launch_id,
      market,
      side,
      input: { mint: inputMint, atomic, decimals },
      output_mint: outputMint,
      max_slippage_bps: Number(slip),
      quote_id: newId(),
      policy_version: "manual",
      decision_id: newId(),
    };
  };

  const quote = useQuery({
    queryKey: ["paper-quote", detail.launch_id, side, amount, slip, market],
    queryFn: async () => api.paperQuote(await buildReq()),
    enabled: Boolean(market && amount),
    retry: false,
  });

  const order = useMutation({
    mutationFn: async () => {
      setErr(null);
      const req = await buildReq();
      if (quote.data?.quote_id) req.quote_id = quote.data.quote_id;
      return api.paperOrder(req);
    },
    onError: (e) => {
      setErr(e instanceof ApiRequestError ? e.message : e instanceof Error ? e.message : "order failed");
    },
  });

  return (
    <div className="space-y-2 rounded border border-hairline bg-surface1 p-3">
      <div className="text-header text-amber">{side === "buy" ? "PAPER BUY" : "PAPER SELL"}</div>
      <p className="text-label text-muted">Simulated only. This never signs or submits a live transaction.</p>
      {!market ? (
        <EmptyState title="No market key" reason="Launch snapshot has no market public key; quote is unavailable." />
      ) : (
        <>
          <label className="block text-label text-muted">
            Input amount
            <input
              className="mt-0.5 w-full rounded border border-hairline bg-canvas px-2 py-1 font-mono"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </label>
          <label className="block text-label text-muted">
            Max slippage bps
            <input
              className="mt-0.5 w-full rounded border border-hairline bg-canvas px-2 py-1 font-mono"
              value={slip}
              onChange={(e) => setSlip(e.target.value)}
            />
          </label>
          <CostPreview quote={quote.data} error={quote.error instanceof Error ? quote.error.message : null} />
          <button
            type="button"
            className="w-full rounded border border-amber bg-amber/10 px-2 py-1.5 text-body text-amber"
            onClick={() => order.mutate()}
            disabled={order.isPending}
          >
            {order.isPending ? "Placing…" : "Place paper order"}
          </button>
          {order.data ? (
            <div className="font-mono text-label text-positive">
              {order.data.status} · {order.data.order_id} · {order.data.mode ?? "PAPER"}
            </div>
          ) : null}
          {err ? <div className="text-label text-negative">{err}</div> : null}
        </>
      )}
    </div>
  );
}
