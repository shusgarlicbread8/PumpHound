import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useEffect, type ReactNode } from "react";
import { EmptyState } from "../../components/EmptyState";
import { ApiRequestError, api } from "../../lib/api";
import { normalizeMode, solscanTx, truncatePubkey } from "../../lib/format";
import type {
  LaunchDecision,
  PaperPortfolioSnapshot,
  PortfolioAmount,
  PortfolioSnapshotPosition,
  PositionTimelineEvent,
  ReportValue,
  SystemStatus,
} from "../../lib/types";
import { API_BASE } from "../../lib/types";
import { usePaper } from "../../stores/paper";
import { useSystem } from "../../stores/system";
import { EnrichmentBudgetStrip } from "../enrichment/EnrichmentBudgetStrip";

export function PortfolioPage() {
  return <PortfolioWorkspace />;
}

export function PortfolioPositionPage() {
  const { portfolioId, positionId } = useParams({
    from: "/portfolio/$portfolioId/positions/$positionId",
  });
  return <PortfolioWorkspace requestedPortfolioId={portfolioId} requestedPositionId={positionId} />;
}

function PortfolioWorkspace({
  requestedPortfolioId,
  requestedPositionId,
}: {
  requestedPortfolioId?: string;
  requestedPositionId?: string;
}) {
  const navigate = useNavigate();
  const ensure = usePaper((state) => state.ensureDefault);
  const load = usePaper((state) => state.load);
  const portfolios = usePaper((state) => state.portfolios);
  const activeId = usePaper((state) => state.activeId);
  const portfolioError = usePaper((state) => state.error);
  const setActive = usePaper((state) => state.setActive);
  const systemStatus = useSystem((state) => state.status);
  const portfolioId = requestedPortfolioId ?? activeId;

  useEffect(() => {
    if (requestedPortfolioId) {
      setActive(requestedPortfolioId);
      void load();
      return;
    }
    void ensure();
  }, [ensure, load, requestedPortfolioId, setActive]);

  const snapshotQuery = useQuery({
    queryKey: ["portfolio-snapshot", portfolioId],
    queryFn: () => api.portfolioSnapshot(portfolioId!),
    enabled: Boolean(portfolioId),
    refetchInterval: 3_000,
    refetchIntervalInBackground: true,
    retry: (count, error) => !(error instanceof ApiRequestError && error.status === 404) && count < 1,
  });

  const snapshot = snapshotQuery.data;
  const positions = snapshot?.positions ?? [];
  const selectedPosition =
    positions.find((position) => position.position_id === requestedPositionId) ??
    (requestedPositionId ? undefined : positions[0]);
  const unavailable =
    snapshotQuery.error instanceof ApiRequestError && snapshotQuery.error.status === 404;
  const connection =
    snapshot && snapshotQuery.isRefetchError
      ? "STALE"
      : snapshot
        ? "CONNECTED"
        : snapshotQuery.isError
          ? "DISCONNECTED"
          : "CONNECTING";

  return (
    <div className="h-full overflow-auto p-3">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <h1 className="text-header">Portfolio</h1>
        <span className="rounded border border-amber/60 bg-amber/10 px-2 py-0.5 text-label font-semibold text-amber">
          PAPER ONLY — no live wallet/signing/submission
        </span>
        <span
          className={`ml-auto rounded border px-2 py-0.5 font-mono text-label ${
            connection === "CONNECTED"
              ? "border-positive/40 text-positive"
              : connection === "CONNECTING"
                ? "border-hairline text-muted"
                : "border-negative/50 text-negative"
          }`}
        >
          {connection}
        </span>
      </div>

      <div className="mb-3 flex flex-wrap items-center gap-2 border-y border-hairline bg-surface1 px-2 py-1">
        <span className="font-mono text-label text-amber">
          {normalizeMode(snapshot?.operating_mode ?? snapshot?.mode ?? systemStatus?.operating_mode ?? systemStatus?.mode)}
        </span>
        <select
          aria-label="Active paper portfolio"
          className="rounded border border-hairline bg-canvas px-2 py-0.5 text-label"
          value={portfolioId ?? ""}
          onChange={(event) => {
            setActive(event.target.value);
            void navigate({ to: "/portfolio" });
          }}
        >
          {!portfolioId ? <option value="">Portfolio unknown</option> : null}
          {portfolios.map((portfolio) => (
            <option key={portfolio.portfolio_id} value={portfolio.portfolio_id}>
              {portfolio.name}
            </option>
          ))}
        </select>
        <span className="text-label text-muted">as of {snapshot?.as_of ?? "Unknown"}</span>
        {snapshotQuery.isFetching ? <span className="text-label text-info">REFRESHING</span> : null}
        {snapshot && snapshotQuery.isRefetchError ? (
          <span className="text-label text-negative" title={snapshotQuery.error.message}>
            Last known values retained · refresh failed
          </span>
        ) : null}
        <button
          type="button"
          className="ml-auto rounded border border-hairline px-2 py-0.5 text-label hover:border-amber"
          onClick={() => void snapshotQuery.refetch()}
        >
          Refetch
        </button>
      </div>

      <OperatingModeStrip snapshot={snapshot} status={systemStatus} />
      <div className="mb-3">
        <EnrichmentBudgetStrip />
      </div>

      {!portfolioId ? (
        <EmptyState
          title="No paper portfolio"
          reason={portfolioError ?? "Experiment books appear when the paper desk is running. Unused Default paper portfolios are archived."}
        />
      ) : unavailable ? (
        <EmptyState
          title="Portfolio snapshot not available"
          reason="GET /api/v1/paper/portfolios/{id}/snapshot returned 404. The desktop will begin polling automatically when the backend provides it."
        />
      ) : !snapshot ? (
        <EmptyState
          title="Portfolio disconnected"
          reason={snapshotQuery.error?.message ?? "Waiting for the first portfolio snapshot."}
        />
      ) : (
        <PortfolioContents
          snapshot={snapshot}
          selectedPosition={selectedPosition}
          requestedPositionId={requestedPositionId}
        />
      )}
    </div>
  );
}

function OperatingModeStrip({
  snapshot,
  status,
}: {
  snapshot: PaperPortfolioSnapshot | undefined;
  status: SystemStatus | null;
}) {
  const mode = normalizeMode(
    snapshot?.operating_mode ?? snapshot?.mode ?? status?.operating_mode ?? status?.mode,
  );
  const strategy =
    snapshot?.strategy ??
    snapshot?.strategy_version ??
    status?.strategy ??
    (mode === "LIVE_PAPER_HEURISTIC" || mode === "LIVE_PAPER_SHADOW_ML" ? "HEURISTIC_V1" : "Unknown");
  const ml = snapshot?.ml_status ?? status?.ml_status ?? "Unknown";
  const ai = aiLabel(snapshot?.ai_status ?? status?.ai_status ?? status?.ai);
  const capital = snapshot?.capital ?? status?.capital ?? "SIMULATED";

  return (
    <section className="panel mb-3 overflow-hidden">
      <div className="grid grid-cols-2 divide-x divide-hairline md:grid-cols-6">
        <div className="p-2">
          <div className="label">Operating mode</div>
          <select
            aria-label="Operating mode (display only)"
            className="mt-0.5 w-full appearance-none border-0 bg-transparent p-0 font-mono text-label text-amber"
            value={mode}
            disabled
          >
            <option value="LIVE_WATCH">LIVE_WATCH</option>
            <option value="LIVE_PAPER_HEURISTIC">LIVE_PAPER_HEURISTIC</option>
            <option value="LIVE_PAPER_SHADOW_ML">LIVE_PAPER_SHADOW_ML</option>
            <option value="LIVE_PAPER_ML">LIVE_PAPER_ML</option>
            <option value="REPLAY">REPLAY</option>
          </select>
        </div>
        <ModeDatum label="Experiment" value={snapshot?.experiment_id ?? "Unknown"} />
        <ModeDatum label="Strategy" value={strategy} />
        <ModeDatum label="ML" value={mlStatusLabel(ml)} />
        <ModeDatum label="AI" value={ai} />
        <ModeDatum label="Capital" value={capital === "SIMULATED" ? "SIMULATED" : "Unknown"} />
      </div>
      <div className="border-t border-hairline px-2 py-1 text-label text-muted">
        Display only · the runtime mode is configured outside the desktop and cannot be changed here.
      </div>
    </section>
  );
}

function ModeDatum({ label, value }: { label: string; value: string }) {
  return (
    <div className="p-2">
      <div className="label">{label}</div>
      <div className={`font-mono text-label ${value === "Unknown" ? "text-unknown" : "text-ink"}`}>
        {value}
      </div>
    </div>
  );
}

function PortfolioContents({
  snapshot,
  selectedPosition,
  requestedPositionId,
}: {
  snapshot: PaperPortfolioSnapshot;
  selectedPosition: PortfolioSnapshotPosition | undefined;
  requestedPositionId?: string;
}) {
  const pending = snapshot.pending_orders;
  const fills = snapshot.fills;
  const rejections = snapshot.rejections;
  const positions = snapshot.positions?.filter(isOpenPosition) ?? snapshot.positions;

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-2 md:grid-cols-4 xl:grid-cols-8">
        <Metric label="Current equity" value={amountText(snapshot.current_equity)} />
        <Metric
          label="Realized P&L"
          value={amountText(snapshot.realized_pnl) === "Unknown" ? valueText(snapshot.pnl?.realized_quote_atomic) : amountText(snapshot.realized_pnl)}
          tone={amountTone(snapshot.realized_pnl)}
          hint={snapshot.sample_warning ?? undefined}
        />
        <Metric
          label="Unrealized P&L"
          value={amountText(snapshot.unrealized_pnl) === "Unknown" ? valueText(snapshot.pnl?.unrealized_quote_atomic) : amountText(snapshot.unrealized_pnl)}
          tone={amountTone(snapshot.unrealized_pnl)}
        />
        <Metric
          label="Closed trades"
          value={snapshot.closed_trades == null ? "Unknown" : String(snapshot.closed_trades)}
          hint={snapshot.sample_warning ?? undefined}
        />
        <Metric label="Open positions" value={positions ? String(positions.length) : "Unknown"} />
        <Metric label="Pending orders" value={pending ? String(pending.length) : "Unknown"} />
        <Metric label="Fills" value={fills ? String(fills.length) : "Unknown"} />
        <Metric label="Fees" value={amountText(snapshot.fees?.total)} />
        <Metric label="Rejections" value={rejections ? String(rejections.length) : "Unknown"} />
      </div>

      <section className="panel overflow-hidden">
        <div className="flex flex-wrap items-center gap-x-5 gap-y-1 border-b border-hairline px-2 py-1.5">
          <h2 className="text-header">Balances & versions</h2>
          <span className="text-label text-muted">
            ML <strong className="font-mono text-ink">{mlStatusLabel(snapshot.ml_status)}</strong>
          </span>
          <span className="text-label text-muted">
            Strategy <strong className="font-mono text-ink">{snapshot.strategy ?? snapshot.strategy_version ?? "Unknown"}</strong>
          </span>
        </div>
        {snapshot.balances == null ? (
          <UnknownBlock label="Balances Unknown" />
        ) : snapshot.balances.length === 0 ? (
          <NoneBlock label="No balances reported" />
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[620px] text-left text-label">
              <thead className="bg-surface2 text-muted">
                <tr>
                  <Th>Asset</Th>
                  <Th>Mint</Th>
                  <Th right>Available</Th>
                  <Th right>Reserved</Th>
                  <Th right>Total</Th>
                </tr>
              </thead>
              <tbody>
                {snapshot.balances.map((balance) => (
                  <tr key={`${balance.asset ?? ""}-${balance.mint ?? balance.asset_mint ?? ""}`} className="border-t border-hairline">
                    <Td>{balance.asset || balance.asset_mint || "Unknown"}</Td>
                    <Td mono title={balance.mint ?? balance.asset_mint ?? undefined}>
                      {balance.mint || balance.asset_mint
                        ? truncatePubkey(balance.mint ?? balance.asset_mint ?? "")
                        : "Unknown"}
                    </Td>
                    <Td right mono>{valueText(balance.available)}</Td>
                    <Td right mono>{valueText(balance.reserved)}</Td>
                    <Td right mono>{valueText(balance.total ?? balance.atomic)}</Td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="panel overflow-hidden">
        <div className="border-b border-hairline px-2 py-1.5 text-header">Open positions</div>
        {positions == null ? (
          <UnknownBlock label="Positions Unknown" />
        ) : positions.length === 0 ? (
          <NoneBlock label="No open positions" />
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[1180px] text-left text-label">
              <thead className="bg-surface2 text-muted">
                <tr>
                  <Th>Token</Th>
                  <Th>Status</Th>
                  <Th right>Size</Th>
                  <Th right>Cost basis</Th>
                  <Th right>Current value</Th>
                  <Th right>Realized</Th>
                  <Th right>Unrealized</Th>
                  <Th right>MFE</Th>
                  <Th right>MAE</Th>
                  <Th right>Fees</Th>
                  <Th>Opened</Th>
                </tr>
              </thead>
              <tbody>
                {positions.map((position) => {
                  const selected = selectedPosition?.position_id === position.position_id;
                  return (
                    <tr
                      key={position.position_id}
                      className={`border-t border-hairline ${selected ? "bg-amber/10" : "hover:bg-surface2"}`}
                    >
                      <Td>
                        <Link
                          to="/portfolio/$portfolioId/positions/$positionId"
                          params={{
                            portfolioId: snapshot.portfolio_id,
                            positionId: position.position_id,
                          }}
                          className="font-mono text-info hover:underline"
                          title={positionMint(position)}
                        >
                          {position.symbol ?? truncatePubkey(positionMint(position))}
                        </Link>
                      </Td>
                      <Td>{position.status ?? "Unknown"}</Td>
                      <Td right mono>{positionAmountText(position.size, position.base_atomic, position.base_mint)}</Td>
                      <Td right mono>{positionAmountText(position.cost_basis, position.cost_quote_atomic, position.quote_mint)}</Td>
                      <Td right mono>{positionAmountText(position.current_value, position.executable_exit_quote_atomic, position.quote_mint)}</Td>
                      <Td right mono className={amountTone(position.realized_pnl)}>{positionAmountText(position.realized_pnl, position.realized_pnl_quote_atomic, position.quote_mint)}</Td>
                      <Td right mono className={amountTone(position.unrealized_pnl)}>{positionAmountText(position.unrealized_pnl, position.unrealized_pnl_quote_atomic, position.quote_mint)}</Td>
                      <Td right mono>{valueText(position.mfe)}</Td>
                      <Td right mono>{valueText(position.mae)}</Td>
                      <Td right mono>{amountText(position.fees)}</Td>
                      <Td mono>{position.opened_at ?? "Unknown"}</Td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <OrdersPanel snapshot={snapshot} />
        <FillsPanel snapshot={snapshot} />
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <RejectionsPanel snapshot={snapshot} />
        <FeesPanel snapshot={snapshot} />
      </div>

      <PositionDetail
        position={selectedPosition}
        portfolioTimeline={snapshot.timeline}
        missingRequestedPosition={Boolean(requestedPositionId && !selectedPosition)}
      />
    </div>
  );
}

function OrdersPanel({ snapshot }: { snapshot: PaperPortfolioSnapshot }) {
  const orders = snapshot.pending_orders;
  return (
    <section className="panel overflow-hidden">
      <div className="border-b border-hairline px-2 py-1.5 text-header">Pending orders</div>
      {orders == null ? (
        <UnknownBlock label="Pending orders Unknown" />
      ) : orders.length === 0 ? (
        <NoneBlock label="No pending orders" />
      ) : (
        <div className="max-h-64 overflow-auto">
          <table className="w-full min-w-[720px] text-left text-label">
            <thead className="sticky top-0 bg-surface2 text-muted">
              <tr>
                <Th>Submitted</Th>
                <Th>Token</Th>
                <Th>Side</Th>
                <Th>Status</Th>
                <Th right>Requested</Th>
                <Th>Rejection reason</Th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={order.order_id} className="border-t border-hairline">
                  <Td mono>{order.submitted_at ?? order.accepted_at ?? "Unknown"}</Td>
                  <Td mono title={order.mint ?? order.output_mint ?? undefined}>
                    {order.mint || order.output_mint
                      ? truncatePubkey(order.mint ?? order.output_mint ?? "")
                      : "Unknown"}
                  </Td>
                  <Td>{order.side ?? "Unknown"}</Td>
                  <Td>{order.status ?? "Unknown"}</Td>
                  <Td right mono>
                    {positionAmountText(order.requested_amount, order.input_atomic, order.input_mint)}
                  </Td>
                  <Td className={order.rejection_reason ? "text-negative" : "text-unknown"}>
                    {order.rejection_reason ?? "Unknown"}
                  </Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function FillsPanel({ snapshot }: { snapshot: PaperPortfolioSnapshot }) {
  const fills = snapshot.fills;
  return (
    <section className="panel overflow-hidden">
      <div className="border-b border-hairline px-2 py-1.5 text-header">Fills</div>
      {fills == null ? (
        <UnknownBlock label="Fills Unknown" />
      ) : fills.length === 0 ? (
        <NoneBlock label="No fills reported" />
      ) : (
        <div className="max-h-64 overflow-auto">
          <table className="w-full min-w-[720px] text-left text-label">
            <thead className="sticky top-0 bg-surface2 text-muted">
              <tr>
                <Th>Filled</Th>
                <Th>Token</Th>
                <Th>Side</Th>
                <Th right>Quantity</Th>
                <Th right>Price</Th>
                <Th right>Fees</Th>
              </tr>
            </thead>
            <tbody>
              {fills.map((fill) => (
                <tr key={fill.fill_id} className="border-t border-hairline">
                  <Td mono>{fill.filled_at ?? fill.arrival_at ?? "Unknown"}</Td>
                  <Td mono title={fill.mint ?? fill.output_mint ?? undefined}>
                    {fill.mint || fill.output_mint
                      ? truncatePubkey(fill.mint ?? fill.output_mint ?? "")
                      : "Unknown"}
                  </Td>
                  <Td>{fill.side ?? "Unknown"}</Td>
                  <Td right mono>
                    {positionAmountText(fill.quantity, fill.output_atomic, fill.output_mint)}
                  </Td>
                  <Td right mono>{amountText(fill.price)}</Td>
                  <Td right mono>{amountText(fill.fees)}</Td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function RejectionsPanel({ snapshot }: { snapshot: PaperPortfolioSnapshot }) {
  const rejections = snapshot.rejections;
  return (
    <section className="panel overflow-hidden">
      <div className="border-b border-hairline px-2 py-1.5 text-header">Rejections</div>
      {rejections == null ? (
        <UnknownBlock label="Rejections Unknown" />
      ) : rejections.length === 0 ? (
        <NoneBlock label="No rejected orders reported" />
      ) : (
        <div className="max-h-56 overflow-auto">
          {rejections.map((rejection, index) => (
            <div
              key={rejection.rejection_id ?? rejection.order_id ?? index}
              className="grid grid-cols-[150px_90px_1fr] gap-2 border-b border-hairline px-2 py-1 text-label"
            >
              <span className="font-mono text-muted">{rejection.occurred_at ?? "Unknown"}</span>
              <span className="font-mono">{rejection.code ?? "Unknown"}</span>
              <span className="text-negative">{rejection.reason ?? "Unknown"}</span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function FeesPanel({ snapshot }: { snapshot: PaperPortfolioSnapshot }) {
  return (
    <section className="panel overflow-hidden">
      <div className="flex items-center justify-between border-b border-hairline px-2 py-1.5">
        <span className="text-header">Fees</span>
        <span className="font-mono text-label">{amountText(snapshot.fees?.total)}</span>
      </div>
      {snapshot.fees == null || snapshot.fees.by_type == null ? (
        <UnknownBlock label="Fee breakdown Unknown" />
      ) : snapshot.fees.by_type.length === 0 ? (
        <NoneBlock label="No fee components reported" />
      ) : (
        snapshot.fees.by_type.map((fee) => (
          <div key={fee.type} className="flex justify-between border-b border-hairline px-2 py-1 text-label">
            <span>{fee.type}</span>
            <span className="font-mono">{amountText(fee.amount)}</span>
          </div>
        ))
      )}
    </section>
  );
}

function PositionDetail({
  position,
  portfolioTimeline,
  missingRequestedPosition,
}: {
  position: PortfolioSnapshotPosition | undefined;
  portfolioTimeline: PositionTimelineEvent[] | null | undefined;
  missingRequestedPosition: boolean;
}) {
  if (!position) {
    return (
      <section className="panel">
        <EmptyState
          title={missingRequestedPosition ? "Position not present" : "No selected position"}
          reason={
            missingRequestedPosition
              ? "The requested position is not present in the latest retained snapshot."
              : "Select an open position to inspect its complete timeline."
          }
        />
      </section>
    );
  }

  return (
    <section className="panel overflow-hidden">
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
        <h2 className="text-header">
          Selected position · {position.symbol ?? truncatePubkey(positionMint(position))}
        </h2>
        <span className="font-mono text-label text-muted">{position.position_id}</span>
        {position.launch_id ? (
          <Link
            to="/token/$launchId"
            params={{ launchId: position.launch_id }}
            className="ml-auto text-label text-info hover:underline"
          >
            Open token
          </Link>
        ) : null}
      </div>
      <div className="grid grid-cols-1 border-b border-hairline md:grid-cols-2 xl:grid-cols-4">
        <Detail label="Entry reason" value={position.entry_reason ?? "Unknown"} />
        <Detail label="Latest exit reason" value={position.latest_exit_reason ?? "Unknown"} />
        <Detail label="Model version" value={position.model_version ?? "MODEL WITHHELD"} mono />
        <Detail label="Strategy version" value={position.strategy_version ?? "Unknown"} mono />
      </div>
      <div className="border-b border-hairline p-2">
        <div className="label mb-1">Latest probabilities</div>
        {position.latest_probabilities == null ? (
          <span className="text-label text-unknown">MODEL WITHHELD</span>
        ) : position.latest_probabilities.length === 0 ? (
          <span className="text-label text-unknown">MODEL WITHHELD</span>
        ) : (
          <div className="grid grid-cols-2 gap-1 md:grid-cols-4">
            {position.latest_probabilities.map((probability) => (
              <div key={`${probability.target}-${probability.horizon ?? ""}`} className="bg-surface2 px-2 py-1">
                <div className="text-label text-muted">
                  {probability.target} {probability.horizon ?? ""}
                </div>
                <div className="font-mono text-body14">
                  {probability.probability === null
                    ? "MODEL WITHHELD"
                    : `${(probability.probability * 100).toFixed(2)}%`}
                </div>
                <div className="text-label text-muted">
                  {probability.model_version ?? position.model_version ?? "MODEL WITHHELD"}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
      <DecisionTimeline position={position} />
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline px-2 py-1.5">
        <span className="text-header">Execution timeline</span>
        {position.timeline == null && portfolioTimeline != null ? (
          <span className="text-label text-unknown">
            Portfolio-level events · backend position linkage unavailable
          </span>
        ) : null}
      </div>
      {(position.timeline ?? portfolioTimeline) == null ? (
        <UnknownBlock label="Timeline Unknown" />
      ) : (position.timeline ?? portfolioTimeline ?? []).length === 0 ? (
        <NoneBlock label="No timeline events reported" />
      ) : (
        <div>
          {(position.timeline ?? portfolioTimeline ?? []).map((event, index) => (
            <div
              key={event.event_id ?? event.order_event_id ?? `${event.order_id ?? "event"}-${index}`}
              className="grid grid-cols-[20px_minmax(140px,180px)_minmax(110px,160px)_1fr] gap-2 border-b border-hairline px-2 py-1.5 text-label"
            >
              <span className="font-mono text-muted">{String(index + 1).padStart(2, "0")}</span>
              <span className="font-mono text-muted">{event.occurred_at ?? event.at_time ?? "Unknown"}</span>
              <span className="font-semibold text-ink">{event.type || event.status || "Unknown"}</span>
              <span>
                <span className={event.status ? "text-info" : "text-unknown"}>{event.status ?? "Unknown"}</span>
                {" · "}
                <span className={event.reason || event.reason_code ? "text-ink" : "text-unknown"}>
                  {event.reason ?? event.reason_code ?? "Unknown"}
                </span>
                {event.model_version ? ` · model ${event.model_version}` : ""}
                {event.strategy_version ? ` · strategy ${event.strategy_version}` : ""}
                {event.details || event.payload ? (
                  <pre className="mt-1 whitespace-pre-wrap break-all font-mono text-[11px] text-muted">
                    {JSON.stringify(event.details ?? event.payload)}
                  </pre>
                ) : null}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function DecisionTimeline({ position }: { position: PortfolioSnapshotPosition }) {
  const decisionsQuery = useQuery({
    queryKey: ["position-decisions", position.launch_id],
    queryFn: () => api.decisions(position.launch_id!),
    enabled: Boolean(position.launch_id),
    refetchInterval: 5_000,
    refetchIntervalInBackground: true,
  });
  const decisions = decisionsQuery.data?.items;

  return (
    <>
      <div className="border-b border-hairline px-2 py-1.5 text-header">
        Decision timeline · actual vs shadow
      </div>
      {!position.launch_id ? (
        <UnknownBlock label="Decision timeline Unknown · launch ID absent" />
      ) : decisions == null ? (
        <UnknownBlock
          label={
            decisionsQuery.error
              ? `Decision timeline unavailable · ${decisionsQuery.error.message}`
              : "Decision timeline loading"
          }
        />
      ) : decisions.length === 0 ? (
        <NoneBlock label="No decisions reported for this position's launch" />
      ) : (
        <div>
          {decisions.map((decision, index) => {
            const scope = decisionScope(decision);
            return (
              <div
                key={decision.decision_id}
                className="border-b border-hairline px-2 py-1.5 text-label"
              >
                <div className="grid grid-cols-[20px_minmax(140px,180px)_120px_100px_1fr] gap-2">
                  <span className="font-mono text-muted">{String(index + 1).padStart(2, "0")}</span>
                  <span className="font-mono text-muted">{decision.decision_at ?? "Unknown"}</span>
                  <span
                    className={
                      scope === "ACTUAL DECISION"
                        ? "text-positive"
                        : scope === "SHADOW MODEL" || scope === "SHADOW AI"
                          ? "text-amber"
                          : "text-info"
                    }
                  >
                    {scope}
                  </span>
                  <span className="font-semibold">{decision.action ?? "Unknown"}</span>
                  <span>
                    kind {decision.decision_kind ?? "Unknown"} · strategy{" "}
                    {decision.heuristic_version ?? "Unknown"} · reasons{" "}
                    {reasonText(decision.reason_codes)}
                  </span>
                </div>
                <DecisionEvidence decisionId={decision.decision_id} />
              </div>
            );
          })}
        </div>
      )}
    </>
  );
}

function DecisionEvidence({ decisionId }: { decisionId: string }) {
  const dossierQuery = useQuery({
    queryKey: ["candidate-dossier", decisionId],
    queryFn: () => api.candidateDossier(decisionId),
    retry: false,
    staleTime: 60_000,
  });
  const dossier = dossierQuery.data;
  if (!dossier) return null;

  return (
    <div className="mt-1 border-l-2 border-info/40 pl-2 text-label">
      <a
        href={`${API_BASE}/enrichment/candidates/${encodeURIComponent(decisionId)}`}
        target="_blank"
        rel="noreferrer"
        className="text-info hover:underline"
      >
        Candidate dossier · {dossier.status ?? "Unknown"} · {dossier.version ?? "Unknown"}
      </a>
      {dossier.selected_wallets?.map((selected) =>
        selected.wallet ? (
          <WalletFundingEvidence
            key={`${decisionId}-${selected.wallet}`}
            wallet={selected.wallet}
            role={selected.role}
          />
        ) : null,
      )}
    </div>
  );
}

function WalletFundingEvidence({ wallet, role }: { wallet: string; role?: string }) {
  const walletQuery = useQuery({
    queryKey: ["enriched-wallet", wallet],
    queryFn: () => api.enrichedWallet(wallet),
    retry: false,
    staleTime: 60_000,
  });
  const inference = walletQuery.data?.funding_inference;
  if (!inference?.funder) return null;

  return (
    <div className="mt-1 flex flex-wrap items-center gap-2 text-muted">
      <Link to="/wallets/$address" params={{ address: wallet }} className="font-mono text-info hover:underline">
        {truncatePubkey(wallet)}
      </Link>
      <span>{role ?? "candidate wallet"}</span>
      <span className="font-semibold text-amber">INFERRED_INITIAL_FUNDER</span>
      <Link
        to="/wallets/$address"
        params={{ address: inference.funder }}
        className="font-mono text-info hover:underline"
      >
        {truncatePubkey(inference.funder)}
      </Link>
      <span>
        confidence{" "}
        {inference.confidence === null || inference.confidence === undefined
          ? "Unknown"
          : `${(inference.confidence * 100).toFixed(1)}%`}
      </span>
      {inference.evidence_signature ? (
        <a
          href={solscanTx(inference.evidence_signature)}
          target="_blank"
          rel="noreferrer"
          className="text-info hover:underline"
        >
          funding evidence
        </a>
      ) : null}
      <a
        href={`${API_BASE}/enrichment/wallets/${encodeURIComponent(wallet)}`}
        target="_blank"
        rel="noreferrer"
        className="text-info hover:underline"
      >
        dossier JSON
      </a>
      <span className="text-unknown">inferred relationship · never ownership</span>
    </div>
  );
}

function Metric({
  label,
  value,
  tone = "",
  hint,
}: {
  label: string;
  value: string;
  tone?: string;
  hint?: string;
}) {
  return (
    <div className="panel min-w-0 p-2">
      <div className="label">{label}</div>
      <div className={`truncate font-mono text-body14 tabular-nums ${tone}`}>{value}</div>
      {hint ? <div className="mt-1 text-[11px] text-amber">{hint}</div> : null}
    </div>
  );
}

function Detail({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="min-w-0 border-r border-hairline p-2">
      <div className="label">{label}</div>
      <div className={`break-words text-label ${mono ? "font-mono" : ""} ${value === "Unknown" ? "text-unknown" : ""}`}>
        {value}
      </div>
    </div>
  );
}

function Th({ children, right = false }: { children: ReactNode; right?: boolean }) {
  return <th className={`whitespace-nowrap px-2 py-1 font-medium ${right ? "text-right" : ""}`}>{children}</th>;
}

function Td({
  children,
  right = false,
  mono = false,
  className = "",
  title,
}: {
  children: ReactNode;
  right?: boolean;
  mono?: boolean;
  className?: string;
  title?: string;
}) {
  return (
    <td
      className={`whitespace-nowrap px-2 py-1 ${right ? "text-right" : ""} ${mono ? "font-mono tabular-nums" : ""} ${className}`}
      title={title}
    >
      {children}
    </td>
  );
}

function UnknownBlock({ label }: { label: string }) {
  return <div className="px-2 py-4 text-center text-label text-unknown">{label}</div>;
}

function NoneBlock({ label }: { label: string }) {
  return <div className="px-2 py-4 text-center text-label text-muted">{label}</div>;
}

function valueText(value: ReportValue | undefined): string {
  if (value === null || value === undefined || value === "") return "Unknown";
  return String(value);
}

function amountText(amount: PortfolioAmount | null | undefined): string {
  if (!amount) return "Unknown";
  const value = valueText(amount.value);
  if (value === "Unknown") return value;
  return amount.asset ? `${value} ${amount.asset}` : value;
}

function positionAmountText(
  amount: PortfolioAmount | null | undefined,
  raw: ReportValue | undefined,
  asset: string | null | undefined,
): string {
  const formatted = amountText(amount);
  if (formatted !== "Unknown") return formatted;
  const rawText = valueText(raw);
  if (rawText === "Unknown") return rawText;
  return asset ? `${rawText} ${truncatePubkey(asset)}` : rawText;
}

function positionMint(position: PortfolioSnapshotPosition): string {
  return position.mint ?? position.base_mint ?? "";
}

function isOpenPosition(position: PortfolioSnapshotPosition): boolean {
  const status = position.status?.trim().toUpperCase();
  return status !== "CLOSED" && status !== "EXITED" && status !== "LIQUIDATED";
}

function mlStatusLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "SHADOW") return "SHADOW";
  if (normalized === "CONTROL") return "CONTROL";
  if (normalized === "NOT_TRAINED" || normalized === "MODEL_NOT_TRAINED") return "NOT TRAINED";
  return "Unknown";
}

function aiLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "ADVISORY") return "ADVISORY";
  if (normalized === "RESEARCH_FILTER") return "RESEARCH_FILTER";
  return "Unknown";
}

function decisionScope(decision: LaunchDecision): "ACTUAL DECISION" | "SHADOW MODEL" | "SHADOW AI" | "AI ADVISORY" {
  const payload = decision.payload ?? {};
  const source = String(payload.decision_source ?? payload.source ?? "").toUpperCase();
  const shadow =
    decision.heuristic_version === "ML_SHADOW" ||
    String(payload.ml_status ?? "").toUpperCase() === "SHADOW" ||
    source.includes("SHADOW");
  const ai = source.includes("AI") || "ai_status" in payload;
  if (ai && shadow) return "SHADOW AI";
  if (ai) return "AI ADVISORY";
  if (shadow) return "SHADOW MODEL";
  return "ACTUAL DECISION";
}

function reasonText(reasons: unknown[] | undefined): string {
  if (!reasons?.length) return "Unknown";
  return reasons.map(String).join(", ");
}

function amountTone(amount: PortfolioAmount | null | undefined): string {
  if (!amount || amount.value === null || amount.value === undefined) return "text-unknown";
  const numeric = typeof amount.value === "number" ? amount.value : Number(amount.value);
  if (!Number.isFinite(numeric) || numeric === 0) return "";
  return numeric > 0 ? "text-positive" : "text-negative";
}
