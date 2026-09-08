import { invoke } from "@tauri-apps/api/core";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../lib/api";

type Source = "helius" | "rpc" | "replay";

export function Onboarding() {
  const health = useQuery({
    queryKey: ["health"],
    queryFn: api.health,
    refetchInterval: 1500,
    retry: false,
  });
  const [source, setSource] = useState<Source>("rpc");
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: async () => {
      setNote(null);
      if (!health.isSuccess || !health.data.ready) {
        throw new Error(
          "core-api is not running. In a terminal: open Docker Desktop, then `just capture-live` (or `just capture-replay`). Leave this window open.",
        );
      }
      if (source === "helius") {
        if (!key.trim()) throw new Error("Helius key required for Live Helius");
        try {
          await invoke("store_provider_secret", { name: "helius", value: key });
        } catch (e) {
          throw new Error(
            e instanceof Error
              ? `Native secret store: ${e.message}`
              : "Native secret store unavailable",
          );
        }
        setKey("");
      }
      try {
        await api.patchSettings({
          source:
            source === "helius" ? "live_helius" : source === "replay" ? "bundled_replay" : "live_rpc_fallback",
        });
      } catch (e) {
        throw new Error(
          e instanceof Error
            ? `Could not save source: ${e.message}`
            : "Could not save source — is core-api on 127.0.0.1:8080?",
        );
      }
      setNote("Source recorded. Waiting for GET /health.");
    },
  });

  return (
    <div className="flex h-full items-center justify-center bg-canvas">
      <div className="w-[440px] rounded border border-hairline bg-surface1 p-4">
        <div className="h-px w-full bg-gradient-to-r from-transparent via-amber to-transparent" />
        <h1 className="mt-3 text-header">PumpHound</h1>
        <p className="mt-1 text-body text-muted">
          Local research terminal. The source radios do nothing until core-api answers on 127.0.0.1:8080.
          No cloud login.
        </p>
        <div className="mt-4 space-y-2">
          {(
            [
              ["helius", "Live Helius"],
              ["rpc", "Live RPC Fallback"],
              ["replay", "Bundled Replay"],
            ] as const
          ).map(([id, label]) => (
            <label key={id} className="flex items-center gap-2 text-body">
              <input type="radio" name="src" checked={source === id} onChange={() => setSource(id)} />
              {label}
            </label>
          ))}
        </div>
        {source === "helius" ? (
          <label className="mt-3 block text-label text-muted">
            Helius API key (OS secret store, never echoed after save)
            <input
              type="password"
              autoComplete="off"
              className="mt-1 w-full rounded border border-hairline bg-canvas px-2 py-1"
              value={key}
              onChange={(e) => setKey(e.target.value)}
            />
          </label>
        ) : null}
        <button
          type="button"
          className="mt-4 w-full rounded border border-amber px-2 py-1.5 text-amber disabled:opacity-40"
          disabled={!health.isSuccess || !health.data.ready || save.isPending}
          onClick={() => save.mutate()}
        >
          {health.isSuccess && health.data.ready ? "Use this source" : "Waiting for core-api…"}
        </button>
        <div className="mt-3 text-label text-muted">
          Health:{" "}
          {health.isSuccess && health.data.ready
            ? `ready · ${health.data.source_label}`
            : "API unreachable — open Docker Desktop, then run `just capture-live` (live) or `just capture-replay` (synthetic). Keep this window open."}
        </div>
        {save.error ? <div className="mt-2 text-label text-negative">{save.error.message}</div> : null}
        {note ? <div className="mt-2 text-label text-info">{note}</div> : null}
      </div>
    </div>
  );
}
