import type { ReactNode } from "react";

export function EmptyState({
  title,
  reason,
  children,
}: {
  title: string;
  reason: string;
  children?: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-[120px] flex-col items-center justify-center gap-2 px-4 text-center">
      <div className="h-px w-16 bg-gradient-to-r from-transparent via-amber/60 to-transparent" />
      <div className="text-header text-ink">{title}</div>
      <p className="max-w-md text-body text-muted">{reason}</p>
      {children}
    </div>
  );
}
