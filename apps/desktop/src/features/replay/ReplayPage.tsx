import { useMutation, useQuery } from "@tanstack/react-query";
import { useSearch } from "@tanstack/react-router";
import { TradeTrace } from "./TradeTrace";
import { useState } from "react";
import { api } from "../../lib/api";
import { normalizeMode } from "../../lib/format";
import { useSystem } from "../../stores/system";

const SPEEDS = ["0.25", "1", "10", "100", "max"] as const;

export function ReplayPage() {
  const search = useSearch({ from: "/replay" });
  if (search.trace) return <div className="h-full overflow-auto p-3"><TradeTrace key={search.trace} id={search.trace} /></div>;
  return <BundledReplay />;
}

function BundledReplay() {
  const mode = normalizeMode(useSystem((s) => s.health?.mode ?? s.status?.mode));
  const [runId, setRunId] = useState<string | null>(null);
  const [speed, setSpeed] = useState<(typeof SPEEDS)[number]>("1");
  const start = useMutation({
    mutationFn: () =>
      api.startReplay({
        source: "bundled",
        execution: "deterministic",
        speed,
      }),
    onSuccess: (r) => setRunId(r.run_id),
  });
  const run = useQuery({
    queryKey: ["replay", runId],
    queryFn: () => api.replay(runId!),
    enabled: Boolean(runId),
    refetchInterval: 2000,
  });
  const control = useMutation({
    mutationFn: (c: Record<string, unknown>) => api.controlReplay(runId!, c),
  });

  return (
    <div className="p-3">
      {mode === "REPLAY" ? (
        <div className="mb-3 rounded border border-amber bg-amber/10 px-3 py-1 text-header text-amber">
          REPLAY
        </div>
      ) : (
        <p className="mb-3 text-label text-muted">
          Live mode is {mode}. Starting a replay job does not switch the collector unless the API is in REPLAY.
        </p>
      )}
      <h1 className="text-header">Replay</h1>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <select
          className="rounded border border-hairline bg-canvas px-2 py-1"
          value={speed}
          onChange={(e) => setSpeed(e.target.value as (typeof SPEEDS)[number])}
        >
          {SPEEDS.map((s) => (
            <option key={s} value={s}>
              {s}×
            </option>
          ))}
        </select>
        <button
          type="button"
          className="rounded border border-hairline px-2 py-1"
          onClick={() => start.mutate()}
        >
          Start bundled replay
        </button>
        {runId ? (
          <>
            <button type="button" className="rounded border border-hairline px-2 py-1" onClick={() => control.mutate({ pause: true })}>
              Pause
            </button>
            <button type="button" className="rounded border border-hairline px-2 py-1" onClick={() => control.mutate({ resume: true })}>
              Resume
            </button>
            <button type="button" className="rounded border border-hairline px-2 py-1" onClick={() => control.mutate({ speed })}>
              Set speed
            </button>
          </>
        ) : null}
      </div>
      <p className="mt-3 font-mono text-body">
        run {run.data?.run_id ?? runId ?? "—"} · state {run.data?.state ?? start.data?.state ?? "—"}
      </p>
      <p className="mt-2 text-label text-muted">
        Space pauses/restarts replay only. “What was knowable?” lists sources that existed at the virtual clock — open
        from the command palette when a run is active.
      </p>
    </div>
  );
}
