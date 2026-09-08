import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../../lib/api";

export function SettingsPage() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["settings"], queryFn: api.settings });
  const [radarHz, setRadarHz] = useState("10");
  const patch = useMutation({
    mutationFn: () => api.patchSettings({ radar_max_hz: Number(radarHz) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["settings"] }),
  });

  const secrets = q.data?.secrets_present ?? {};
  const revision = q.data?.revision;

  return (
    <div className="p-3">
      <h1 className="text-header">Settings</h1>
      <p className="mt-1 text-body text-muted">
        Non-secret configuration only. Provider keys stay in the OS credential store and are never echoed.
      </p>
      <div className="mt-3 panel max-w-lg space-y-2 p-3">
        <div className="label">Revision {revision ?? "—"}</div>
        <label className="block text-label text-muted">
          Radar UI budget (Hz)
          <input
            className="mt-0.5 w-full rounded border border-hairline bg-canvas px-2 py-1 font-mono"
            value={radarHz}
            onChange={(e) => setRadarHz(e.target.value)}
          />
        </label>
        <button
          type="button"
          className="rounded border border-hairline px-2 py-1"
          onClick={() => patch.mutate()}
        >
          Save non-secret settings
        </button>
      </div>
      <div className="mt-3 panel max-w-lg p-3">
        <div className="text-header">Secret presence</div>
        {Object.keys(secrets).length ? (
          <ul className="mt-2 text-body">
            {Object.entries(secrets).map(([k, present]) => (
              <li key={k}>
                {k}: {present ? "present" : "absent"}
              </li>
            ))}
          </ul>
        ) : (
          <p className="mt-2 text-unknown">— API reported no secret flags</p>
        )}
      </div>
    </div>
  );
}
