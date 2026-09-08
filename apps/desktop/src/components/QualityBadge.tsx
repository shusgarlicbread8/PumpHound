import { qualityReason } from "../lib/format";
import type { DataQuality } from "../lib/types";

export function QualityBadge({
  quality,
  className = "",
}: {
  quality?: Partial<DataQuality> | null;
  className?: string;
}) {
  if (!quality) {
    return (
      <span
        className={`inline-flex items-center gap-1 rounded border border-hairline px-1.5 py-0.5 text-label text-unknown ${className}`}
        title="quality not attached"
      >
        <span aria-hidden>■</span>
        UNAVAILABLE
      </span>
    );
  }
  const grade = quality.grade ?? "UNAVAILABLE";
  const tone =
    grade === "A"
      ? "text-positive border-positive/40"
      : grade === "B"
        ? "text-info border-info/40"
        : grade === "C"
          ? "text-amber border-amber/40"
          : "text-unknown border-unknown/40";
  const full = qualityReason(quality as DataQuality);
  return (
    <span
      className={`inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-label ${tone} ${className}`}
      title={full}
    >
      <span aria-hidden>{grade === "A" ? "●" : grade === "UNAVAILABLE" ? "■" : "▲"}</span>
      {grade}
      {quality.coverage && quality.coverage !== "complete_for_scope" ? (
        <span className="text-muted">· {quality.coverage.replaceAll("_", " ")}</span>
      ) : null}
    </span>
  );
}
