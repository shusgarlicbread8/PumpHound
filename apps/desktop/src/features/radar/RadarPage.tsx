import { StaleOverlay } from "../../components/StaleOverlay";
import { useRadar } from "../../stores/radar";
import { useUi } from "../../stores/ui";
import { FastInspect } from "./FastInspect";
import { RadarFilters } from "./RadarFilters";
import { RadarTable } from "./RadarTable";
import { SavedFilters } from "./SavedFilters";
import { useWatchSubjects, WatchlistPanel } from "./WatchlistPanel";

export function RadarPage() {
  const ingested = useRadar((s) => s.ingested);
  const order = useRadar((s) => s.order);
  const displayRows = useRadar((s) => s.displayRows);
  const displayOrder = useRadar((s) => s.displayOrder);
  const paused = useRadar((s) => s.paused);
  const hydrating = useRadar((s) => s.hydrating);
  const error = useRadar((s) => s.error);
  const selected = useUi((s) => s.selectedLaunchId);
  const watch = useWatchSubjects();

  const rows = (paused && displayRows && displayOrder
    ? displayOrder.map((id) => displayRows.get(id))
    : order.map((id) => ingested.get(id))
  ).filter((r): r is NonNullable<typeof r> => Boolean(r));

  return (
    <div className="relative flex h-full min-h-0">
      <StaleOverlay />
      <div className="flex min-w-0 flex-[68] flex-col border-r border-hairline">
        <RadarFilters />
        <SavedFilters />
        {error ? <div className="border-b border-negative/40 px-3 py-1 text-label text-negative">{error}</div> : null}
        {hydrating && !rows.length ? (
          <div className="p-3 text-muted">Loading authoritative snapshot…</div>
        ) : (
          <div className="min-h-0 flex-1">
            <RadarTable rows={rows} watch={watch} />
          </div>
        )}
        <WatchlistPanel />
      </div>
      <div className="flex-[32] bg-surface1">
        <FastInspect launchId={selected} />
      </div>
    </div>
  );
}
