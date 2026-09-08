import { useState } from "react";
import { defaultFilters, useRadar, type RadarFilters } from "../../stores/radar";

const KEY = "pumphound.radar.filters";

function loadSaved(): Record<string, RadarFilters> {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as Record<string, RadarFilters>) : {};
  } catch {
    return {};
  }
}

export function SavedFilters() {
  const filters = useRadar((s) => s.filters);
  const setFilters = useRadar((s) => s.setFilters);
  const [name, setName] = useState("");
  const [saved, setSaved] = useState(loadSaved);

  return (
    <div className="flex items-center gap-1 px-3 py-1 text-label">
      <span className="text-muted">Saved</span>
      <select
        className="rounded border border-hairline bg-canvas px-1 py-0.5"
        defaultValue=""
        onChange={(e) => {
          const f = saved[e.target.value];
          if (f) setFilters(f);
        }}
      >
        <option value="">—</option>
        {Object.keys(saved).map((k) => (
          <option key={k} value={k}>
            {k}
          </option>
        ))}
      </select>
      <input
        className="w-24 rounded border border-hairline bg-canvas px-1 py-0.5"
        placeholder="name"
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <button
        type="button"
        className="rounded border border-hairline px-1.5 py-0.5"
        onClick={() => {
          if (!name.trim()) return;
          const next = { ...saved, [name.trim()]: filters };
          localStorage.setItem(KEY, JSON.stringify(next));
          setSaved(next);
          setName("");
        }}
      >
        Save
      </button>
      <button
        type="button"
        className="rounded border border-hairline px-1.5 py-0.5"
        onClick={() => setFilters(defaultFilters())}
      >
        Reset
      </button>
    </div>
  );
}
