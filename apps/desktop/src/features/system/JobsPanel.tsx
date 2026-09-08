import { useQuery } from "@tanstack/react-query";
import { api } from "../../lib/api";

export function JobsPanel() {
  const alerts = useQuery({ queryKey: ["alerts"], queryFn: api.alerts, refetchInterval: 4000 });
  const items = Array.isArray(alerts.data?.items) ? alerts.data.items : [];
  return (
    <div className="panel p-3">
      <div className="text-header">Jobs / alerts</div>
      <p className="mt-1 text-body text-muted">
        Alert items: {items.length}. A trading block is always an open alert.
      </p>
      {items.length === 0 ? (
        <p className="mt-2 text-label text-muted">No open operator alerts.</p>
      ) : (
        <ul className="mt-2 space-y-1 text-label">
          {items.map((row, i) => {
            const item = row && typeof row === "object" ? (row as Record<string, unknown>) : {};
            return (
              <li key={String(item.alert_id ?? i)} className="text-negative">
                {String(item.severity ?? "ALERT")} · {String(item.title ?? item.code ?? "unspecified")}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
