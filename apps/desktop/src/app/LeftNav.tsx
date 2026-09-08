import { Link, useRouterState } from "@tanstack/react-router";
import { useUi } from "../stores/ui";

export function LeftNav() {
  const expanded = useUi((s) => s.navExpanded);
  const toggle = useUi((s) => s.toggleNav);
  const selected = useUi((s) => s.selectedLaunchId);
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  return (
    <nav
      className="flex shrink-0 flex-col border-r border-hairline bg-surface1 py-1"
      style={{ width: expanded ? 184 : 56 }}
    >
      <button type="button" className="px-2 py-1 text-label text-muted" onClick={toggle}>
        {expanded ? "Collapse" : "☰"}
      </button>
      <Item to="/" label="Home" hint="g h" active={pathname === "/"} expanded={expanded} />
      <Group label="Discover" expanded={expanded} />
      <Item to="/radar" label="Radar" hint="g r" active={pathname.startsWith("/radar")} expanded={expanded} />
      {selected ? (
        <Item
          to="/token/$launchId"
          params={{ launchId: selected }}
          label="Investigate"
          active={pathname.startsWith("/token/")}
          expanded={expanded}
        />
      ) : (
        <Item to="/token" label="Investigate" active={pathname.startsWith("/token")} expanded={expanded} />
      )}
      <Group label="Trade" expanded={expanded} />
      <Item to="/trade" label="Overview" hint="g t" active={pathname.startsWith("/trade")} expanded={expanded} />
      <Item to="/paper" label="Orders" active={pathname.startsWith("/paper")} expanded={expanded} />
      <Item
        to="/portfolio"
        label="Positions"
        active={pathname.startsWith("/portfolio")}
        expanded={expanded}
      />
      <Group label="Learn" expanded={expanded} />
      <Item to="/strategies" label="Strategies" active={pathname.startsWith("/strategies")} expanded={expanded} />
      <Item to="/models" label="Models" hint="g m" active={pathname.startsWith("/models")} expanded={expanded} />
      <Item to="/research" label="Research" active={pathname.startsWith("/research")} expanded={expanded} />
      <Item to="/replay" label="Replay" active={pathname.startsWith("/replay")} expanded={expanded} />
      <Group label="System" expanded={expanded} />
      <Item to="/system" label="Health" hint="g s" active={pathname.startsWith("/system")} expanded={expanded} />
      <Item to="/settings" label="Settings" active={pathname.startsWith("/settings")} expanded={expanded} />
      <Group label="Index" expanded={expanded} />
      <Item to="/wallets" label="Wallets" active={pathname.startsWith("/wallets")} expanded={expanded} />
      <Item to="/creators" label="Creators" active={pathname.startsWith("/creators")} expanded={expanded} />
      <Item to="/graph" label="Graph" active={pathname.startsWith("/graph")} expanded={expanded} />
    </nav>
  );
}

function Group({ label, expanded }: { label: string; expanded: boolean }) {
  if (!expanded) return <div className="my-1 border-t border-hairline" />;
  return <div className="mt-2 px-3 text-[10px] uppercase tracking-wide text-muted">{label}</div>;
}

function Item({
  to,
  label,
  hint,
  active,
  expanded,
  params,
}: {
  to: string;
  label: string;
  hint?: string;
  active: boolean;
  expanded: boolean;
  params?: Record<string, string>;
}) {
  return (
    <Link
      to={to}
      params={params}
      className={`mx-1 flex items-center gap-2 rounded px-2 py-1.5 text-body ${
        active ? "bg-amber/10 text-amber" : "text-muted hover:bg-surface2 hover:text-ink"
      }`}
      title={hint}
    >
      <span className="w-8 shrink-0 text-center text-label">{label.slice(0, 2)}</span>
      {expanded ? <span>{label}</span> : null}
    </Link>
  );
}
