-- Bounded high-fidelity tracking for tokens a strategy judged interesting.
-- Broad scout (logs/websocket) continues for all launches. This table only
-- records who promoted a mint, why, and when richer tracking expires.

CREATE TABLE IF NOT EXISTS pumphound.hot_set_memberships (
    mint text PRIMARY KEY,
    launch_id uuid REFERENCES pumphound.launches(launch_id),
    reason_code text NOT NULL,
    promoted_by text NOT NULL,
    priority integer NOT NULL DEFAULT 100,
    fidelity text NOT NULL DEFAULT 'HOT'
        CHECK (fidelity IN ('SCOUT', 'HOT')),
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    first_promoted_at timestamptz NOT NULL DEFAULT now(),
    last_promoted_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    promotion_count integer NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS hot_set_memberships_expires_idx
    ON pumphound.hot_set_memberships (expires_at);
CREATE INDEX IF NOT EXISTS hot_set_memberships_launch_idx
    ON pumphound.hot_set_memberships (launch_id);

CREATE TABLE IF NOT EXISTS pumphound.hot_set_events (
    hot_set_event_id uuid PRIMARY KEY,
    mint text NOT NULL,
    launch_id uuid,
    action text NOT NULL CHECK (action IN ('PROMOTED', 'EXTENDED', 'EXPIRED', 'EVICTED')),
    reason_code text NOT NULL,
    promoted_by text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS hot_set_events_mint_idx
    ON pumphound.hot_set_events (mint, occurred_at DESC);
