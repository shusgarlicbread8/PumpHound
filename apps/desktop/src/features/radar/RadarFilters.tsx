import { useRadar, type RadarFilters as Filters } from "../../stores/radar";

export function RadarFilters() {
  const filters = useRadar((s) => s.filters);
  const set = useRadar((s) => s.setFilters);
  const paused = useRadar((s) => s.paused);
  const setPaused = useRadar((s) => s.setPaused);
  const nNew = useRadar((s) => s.newBuffer.length);
  const flush = useRadar((s) => s.flushNew);
  const pinned = useRadar((s) => s.sortPinned);

  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-hairline bg-surface1 px-3 py-2">
      <label className="flex items-center gap-1 text-label text-muted">
        Age ≤
        <input
          className="w-14 rounded border border-hairline bg-canvas px-1 py-0.5 font-mono text-body text-ink"
          type="number"
          min={0}
          placeholder="min"
          value={filters.maxAgeMin ?? ""}
          onChange={(e) => set({ maxAgeMin: e.target.value === "" ? null : Number(e.target.value) })}
        />
        m
      </label>
      <Select
        value={filters.venue}
        onChange={(venue) => set({ venue: venue as Filters["venue"] })}
        options={[
          ["all", "All venues"],
          ["curve", "Curve"],
          ["pumpswap", "PumpSwap"],
        ]}
      />
      <Select
        value={filters.quote}
        onChange={(quote) => set({ quote: quote as Filters["quote"] })}
        options={[
          ["all", "All quotes"],
          ["sol", `SOL`],
          ["usdc", `USDC`],
        ]}
      />
      <label className="flex items-center gap-1 text-label text-muted">
        Buyers ≥
        <input
          className="w-12 rounded border border-hairline bg-canvas px-1 py-0.5 font-mono text-body"
          type="number"
          min={0}
          value={filters.minBuyers ?? ""}
          onChange={(e) => set({ minBuyers: e.target.value === "" ? null : Number(e.target.value) })}
        />
      </label>
      <label className="flex items-center gap-1 text-label text-muted">
        Curve ≥
        <input
          className="w-12 rounded border border-hairline bg-canvas px-1 py-0.5 font-mono text-body"
          type="number"
          min={0}
          max={100}
          value={filters.minCurve === null ? "" : Math.round(filters.minCurve * 100)}
          onChange={(e) =>
            set({ minCurve: e.target.value === "" ? null : Number(e.target.value) / 100 })
          }
        />
        %
      </label>
      <label className="flex items-center gap-1 text-label text-muted">
        <input
          type="checkbox"
          checked={filters.excludeUnknownQuality}
          onChange={(e) => set({ excludeUnknownQuality: e.target.checked })}
        />
        Hide unknown quality
      </label>
      <label className="flex items-center gap-1 text-label text-muted">
        <input
          type="checkbox"
          checked={filters.watchlistedOnly}
          onChange={(e) => set({ watchlistedOnly: e.target.checked })}
        />
        Watchlisted
      </label>
      <label className="flex items-center gap-1 text-label text-muted">
        <input
          type="checkbox"
          checked={filters.heldOnly}
          onChange={(e) => set({ heldOnly: e.target.checked })}
        />
        Held
      </label>
      <label className="flex items-center gap-1 text-label text-unknown" title="Target-specific model qualification is not part of the Radar row contract">
        <input type="checkbox" checked={filters.modelQualifiedOnly} disabled readOnly />
        Model-qualified · unavailable in Radar
      </label>
      <span className="text-label text-unknown" title="Open Inspect for the per-token collapse target state">
        Max rug · see target state
      </span>
      <button
        type="button"
        className={`rounded border px-2 py-0.5 text-label ${paused ? "border-amber text-amber" : "border-hairline text-muted"}`}
        onClick={() => setPaused(!paused)}
      >
        {paused ? "Display paused" : "Pause display"}
      </button>
      {nNew > 0 ? (
        <button
          type="button"
          className="rounded border border-amber px-2 py-0.5 text-label text-amber"
          onClick={flush}
        >
          {nNew} new
        </button>
      ) : null}
      {pinned ? <span className="text-label text-amber">Sort pinned</span> : null}
    </div>
  );
}

function Select({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: Array<[string, string]>;
}) {
  return (
    <select
      className="rounded border border-hairline bg-canvas px-1.5 py-0.5 text-label"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    >
      {options.map(([v, l]) => (
        <option key={v} value={v}>
          {l}
        </option>
      ))}
    </select>
  );
}
