import { staleCaption } from "../lib/format";
import { useRadar } from "../stores/radar";
import { useSyncExternalStore } from "react";

export function StaleOverlay() {
  const connected = useRadar((s) => s.connected);
  const last = useRadar((s) => s.lastUpdateAt);
  const now = useSyncExternalStore(
    (cb) => {
      const id = window.setInterval(cb, 200);
      return () => window.clearInterval(id);
    },
    () => Date.now(),
    () => 0,
  );
  if (connected) return null;
  return (
    <div className="pointer-events-none absolute inset-0 z-20 flex items-start justify-center pt-10">
      <div className="rounded border border-amber/50 bg-canvas/80 px-3 py-1.5 font-mono text-body text-amber tabular-nums">
        {staleCaption(last, now)}
      </div>
    </div>
  );
}
