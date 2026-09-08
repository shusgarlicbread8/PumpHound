import { useQuery } from "@tanstack/react-query";
import Graph from "graphology";
import { useEffect, useMemo, useRef, useState } from "react";
import Sigma from "sigma";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { truncatePubkey } from "../../lib/format";
import { aggregateGraphEdges } from "../../lib/graphAggregate";
import { asArray, asNumber, asString } from "../../lib/safe";
import { GRAPH_EDGE_CAP, GRAPH_NODE_CAP, type GraphEdge, type GraphResponse } from "../../lib/types";
import { useUi } from "../../stores/ui";

export function GraphPage() {
  const subject = useUi((s) => s.selectedLaunchId);
  const [mode, setMode] = useState("token");
  const q = useQuery({
    queryKey: ["graph", subject, mode],
    queryFn: () => api.launchGraph(subject!),
    enabled: Boolean(subject),
  });

  return (
    <div className="flex h-full min-h-0">
      <div className="min-w-0 flex-1 border-r border-hairline">
        <div className="flex flex-wrap gap-1 border-b border-hairline px-2 py-1">
          {["token", "wallet", "creator", "cohort", "funding", "cosell"].map((m) => (
            <button
              key={m}
              type="button"
              className={`rounded px-2 py-0.5 text-label ${mode === m ? "bg-amber/15 text-amber" : "text-muted"}`}
              onClick={() => setMode(m)}
            >
              {m}
            </button>
          ))}
        </div>
        {!subject ? (
          <EmptyState title="No graph subject" reason="Select a launch from Radar to load GET /launches/{id}/graph." />
        ) : (
          <GraphCanvas graph={q.data} />
        )}
      </div>
      <aside className="w-[320px] overflow-auto bg-surface1 p-3">
        <div className="text-header">Inspector</div>
        <p className="mt-2 text-body text-muted">
          Cluster colouring names the algorithm/version when present. Edge thickness uses transformed weight, not raw
          transfer value. Caps: {GRAPH_NODE_CAP} nodes / {GRAPH_EDGE_CAP} edges.
        </p>
        {q.data ? (
          <dl className="mt-3 space-y-1 text-body">
            <div>Nodes {q.data.nodes.length}</div>
            <div>Render edges {aggregateGraphEdges(q.data.edges).length}</div>
            <div>Raw evidence {asNumber(q.data.raw_evidence_count) ?? q.data.edges.length}</div>
            <div>Hidden {q.data.hidden_nodes}</div>
            <div className="text-muted">as-of {q.data.as_of ?? "—"}</div>
          </dl>
        ) : (
          <p className="mt-3 text-unknown">— no graph payload</p>
        )}
      </aside>
    </div>
  );
}

export function GraphCanvas({ graph }: { graph: GraphResponse | undefined }) {
  const ref = useRef<HTMLDivElement>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [renderError, setRenderError] = useState<string | null>(null);
  const nodes = (graph?.nodes ?? []).slice(0, GRAPH_NODE_CAP);
  const edges = useMemo(
    () => aggregateGraphEdges(graph?.edges).slice(0, GRAPH_EDGE_CAP),
    [graph],
  );

  useEffect(() => {
    setSelectedKey(null);
    setRenderError(null);
  }, [graph]);

  useEffect(() => {
    const nodeList = (graph?.nodes ?? []).slice(0, GRAPH_NODE_CAP);
    const edgeList = aggregateGraphEdges(graph?.edges).slice(0, GRAPH_EDGE_CAP);
    if (!ref.current || !nodeList.length) return;
    const g = new Graph();
    try {
      nodeList.forEach((n, i) => {
        const angle = (i / Math.max(nodeList.length, 1)) * Math.PI * 2;
        if (!n.id || g.hasNode(n.id)) return;
        g.addNode(n.id, {
          x: Math.cos(angle),
          y: Math.sin(angle),
          size: 6,
          label: n.label ?? n.id.slice(0, 6),
          color: "#F3A83B",
        });
      });
      edgeList.forEach((e) => {
        if (!e.source || !e.target || !g.hasNode(e.source) || !g.hasNode(e.target)) return;
        const key = `${e.source}->${e.target}`;
        if (g.hasEdge(key) || g.hasEdge(e.source, e.target)) return;
        const weight = Number.isFinite(e.weight) ? Math.max(0, e.weight ?? 1) : 1;
        const count = asNumber(e.count) ?? 1;
        g.addEdgeWithKey(key, e.source, e.target, {
          size: Math.min(8, 0.6 + Math.log1p(weight) + Math.log1p(count)),
          color: e.inferred ? "#8B7E9E" : "#63A7FF",
          kind: e.kind ?? "",
          mix: e.relationship_mix,
          count,
        });
      });
      const sigma = new Sigma(g, ref.current, {
        renderLabels: true,
        labelColor: { color: "#E7EDF3" },
        defaultEdgeColor: "#26303A",
      });
      const onEdge = (payload: { edge: string }) => {
        setSelectedKey(payload.edge);
      };
      sigma.on("clickEdge", onEdge);
      return () => {
        sigma.off("clickEdge", onEdge);
        sigma.kill();
      };
    } catch (error) {
      setRenderError(error instanceof Error ? error.message : "graph renderer failed");
      return undefined;
    }
  }, [graph]);

  if (renderError) {
    return <EmptyState title="Graph unavailable" reason={renderError} />;
  }
  if (!nodes.length) {
    return (
      <EmptyState
        title="No graph"
        reason={
          graph
            ? `API returned 0 nodes (hidden ${graph.hidden_nodes}). Neighbourhood is not invented.`
            : "Graph snapshot not loaded."
        }
      />
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div ref={ref} className="min-h-[240px] flex-1 bg-canvas" />
      <GraphEdgeList edges={edges} selectedKey={selectedKey} onSelect={setSelectedKey} />
    </div>
  );
}

function edgeString(e: GraphEdge, key: "event_id" | "signature" | "kind"): string | null {
  const v = e[key];
  return typeof v === "string" && v.trim() ? v : null;
}

function edgeKey(e: GraphEdge, _index: number): string {
  return `${e.source}->${e.target}`;
}

function GraphEdgeList({
  edges,
  selectedKey,
  onSelect,
}: {
  edges: GraphEdge[];
  selectedKey: string | null;
  onSelect: (key: string) => void;
}) {
  if (!edges.length) {
    return (
      <div className="border-t border-hairline p-2 text-label text-unknown">
        No edges in this graph payload.
      </div>
    );
  }
  const selected = edges.find((e, i) => edgeKey(e, i) === selectedKey);
  return (
    <div className="max-h-[220px] overflow-auto border-t border-hairline bg-surface1">
      <div className="sticky top-0 z-10 border-b border-hairline px-2 py-1 text-header">
        Edges · click for supporting event
      </div>
      {selected ? (
        <div className="border-b border-hairline px-2 py-1 font-mono text-label">
          {asString(selected.kind)} · mix {JSON.stringify(selected.relationship_mix ?? [])} · count{" "}
          {asString(selected.count, "1")} · evidence{" "}
          {asArray(selected.evidence).length}
          <div className="mt-1 max-h-16 overflow-auto text-muted">
            {asArray(selected.evidence).map((item, i) => {
              const rec = item && typeof item === "object" ? (item as Record<string, unknown>) : {};
              return (
                <div key={i}>
                  event {asString(rec.event_id)} · sig {asString(rec.signature)}
                </div>
              );
            })}
          </div>
        </div>
      ) : (
        <p className="px-2 py-1 text-label text-muted">
          Canvas edge clicks are unreliable; use the list. Supporting id is event_id from the payload.
        </p>
      )}
      <table className="w-full text-left text-label">
        <thead className="text-muted">
          <tr>
            <th className="px-2 py-1 font-normal">kind</th>
            <th className="px-2 py-1 font-normal">source</th>
            <th className="px-2 py-1 font-normal">target</th>
            <th className="px-2 py-1 font-normal">event_id</th>
            <th className="px-2 py-1 font-normal">signature</th>
          </tr>
        </thead>
        <tbody>
          {edges.map((e, i) => {
            const key = edgeKey(e, i);
            const eventId = edgeString(e, "event_id");
            const sig = edgeString(e, "signature");
            return (
              <tr
                key={key}
                className={`cursor-pointer hover:bg-surface2 ${selectedKey === key ? "bg-amber/10" : ""}`}
                onClick={() => onSelect(key)}
              >
                <td className="px-2 py-1">{e.kind ?? "—"}</td>
                <td className="px-2 py-1 font-mono" title={e.source}>
                  {truncatePubkey(e.source)}
                </td>
                <td className="px-2 py-1 font-mono" title={e.target}>
                  {truncatePubkey(e.target)}
                </td>
                <td className="px-2 py-1 font-mono" title={eventId ?? undefined}>
                  {eventId ? truncatePubkey(eventId, 6, 6) : "—"}
                </td>
                <td className="px-2 py-1 font-mono" title={sig ?? undefined}>
                  {sig ? truncatePubkey(sig, 6, 6) : "—"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
