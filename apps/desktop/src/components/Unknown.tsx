export function Unknown({ reason, className = "" }: { reason: string; className?: string }) {
  return (
    <span className={`text-unknown ${className}`} title={reason}>
      —
    </span>
  );
}
