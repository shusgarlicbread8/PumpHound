import { useQuery } from "@tanstack/react-query";
import { api } from "../lib/api";
import { formatLag, formatSlot, normalizeMode } from "../lib/format";
import { asRecord, asString } from "../lib/safe";
import { useSystem } from "../stores/system";
import { useUi } from "../stores/ui";

export function TopRail() {
  const health = useSystem((s) => s.health);
  const status = useSystem((s) => s.status);
  const readiness = useQuery({ queryKey: ["readiness"], queryFn: api.readiness, refetchInterval: 4000 });
  const openCmd = useUi((s) => s.setCommandOpen);
  const mode = normalizeMode(status?.operating_mode ?? health?.mode ?? status?.mode);
  const ready = asRecord(readiness.data);
  const fromReady = typeof ready.headline === "string" ? ready.headline : "";
  const fromStatus = status && "headline" in status && typeof status.headline === "string" ? status.headline : "";
  const headline = fromReady || fromStatus || (status?.chain === "healthy" ? "SYSTEM RUNNING" : (status?.chain ?? "disconnected"));
  const market = asRecord(ready.subsystems)["MARKET DATA"];
  const blocked = asString(ready.autonomous_paper) === "BLOCKED" || market === "STALE";

  return (
    <header className="flex h-rail shrink-0 items-center gap-3 border-b border-hairline bg-surface1 px-2 text-label">
      <span className="font-medium text-amber">PumpHound</span>
      <span className="rounded border border-amber/40 px-1.5 py-0.5 text-amber">{mode}</span>
      <span className={blocked ? "text-negative" : "text-positive"} title={asArrayReasons(ready.reasons)}>
        {headline}
      </span>
      <span className="font-mono tabular-nums text-muted">
        slot {formatSlot(status?.last_slot)}
      </span>
      <span className="font-mono tabular-nums text-muted">lag {formatLag(status?.lag_ms)}</span>
      <span className="font-mono text-muted">strategy {status?.strategy ?? "Unknown"}</span>
      <span className="font-mono text-muted">ML {mlLabel(status?.ml_status)}</span>
      <span className="font-mono text-muted">AI {aiLabel(status?.ai_status ?? status?.ai)}</span>
      <span className="font-mono text-amber">CAPITAL SIMULATED</span>
      <span className="ml-auto truncate text-negative">PAPER ONLY · NO SIGNING</span>
      <button
        type="button"
        className="rounded border border-hairline px-2 py-0.5 text-muted hover:text-ink"
        onClick={() => openCmd(true)}
      >
        ⌘K
      </button>
    </header>
  );
}

function mlLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "SHADOW" || normalized === "CONTROL") return normalized;
  if (normalized === "NOT_TRAINED" || normalized === "MODEL_NOT_TRAINED") return "NOT TRAINED";
  return "Unknown";
}

function asArrayReasons(value: unknown): string {
  return Array.isArray(value) ? value.map(String).join(" · ") : "";
}

function aiLabel(value: string | null | undefined): string {
  const normalized = value?.trim().toUpperCase().replace(/-/g, "_");
  if (normalized === "ADVISORY") return "ADVISORY";
  if (normalized === "RESEARCH_FILTER") return "RESEARCH_FILTER";
  return "Unknown";
}
