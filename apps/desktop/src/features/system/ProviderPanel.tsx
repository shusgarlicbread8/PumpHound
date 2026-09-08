import type { Capabilities, SystemStatus } from "../../lib/types";

export function ProviderPanel({
  status,
  caps,
}: {
  status: SystemStatus | undefined;
  caps: Capabilities | undefined;
}) {
  return (
    <div className="panel p-3">
      <div className="text-header">Providers</div>
      <Row k="Source" v={status?.source ?? "—"} />
      <Row k="LaserStream" v={caps?.laserstream ? "enabled" : "off"} />
      <Row k="RPC fallback" v={caps?.rpc_fallback ? "enabled" : "off"} />
      <Row k="Birdeye" v={caps?.birdeye ? "enabled" : "degraded / off"} />
      <Row k="Jupiter" v={caps?.jupiter ? "enabled" : "degraded / off"} />
      <Row k="Pyth" v={caps?.pyth ? "enabled" : "degraded / off"} />
      <Row k="Investigator" v={caps?.investigator ? "optional ready" : "optional"} />
      <p className="mt-2 text-label text-muted">Secrets are redacted. Flags never include key material.</p>
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="mt-1 flex justify-between text-body">
      <span className="text-muted">{k}</span>
      <span>{v}</span>
    </div>
  );
}
