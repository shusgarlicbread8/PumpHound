import { useQuery } from "@tanstack/react-query";
import { EmptyState } from "../../components/EmptyState";
import { api } from "../../lib/api";
import { useUi } from "../../stores/ui";
import { PaperTicket } from "../token/PaperTicket";

export function OrderTicket({ side }: { side: "buy" | "sell" }) {
  const launchId = useUi((s) => s.selectedLaunchId);
  const detail = useQuery({
    queryKey: ["launch", launchId],
    queryFn: () => api.launch(launchId!),
    enabled: Boolean(launchId),
  });
  if (!launchId) {
    return <EmptyState title="Select a token" reason="Paper tickets require a launch selected from Radar." />;
  }
  if (detail.isLoading) return <div className="p-3 text-muted">Loading market…</div>;
  if (!detail.data) return <EmptyState title="Launch missing" reason={detail.error?.message ?? "—"} />;
  return <PaperTicket detail={detail.data} side={side} />;
}
