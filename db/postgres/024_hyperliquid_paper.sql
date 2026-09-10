BEGIN;
CREATE SCHEMA IF NOT EXISTS pumphound;
SET search_path TO pumphound, public;

CREATE TABLE IF NOT EXISTS hl_paper_orders (
  order_id uuid PRIMARY KEY,
  experiment_id text,
  book text,
  native_id text NOT NULL,
  side text NOT NULL,
  intent jsonb NOT NULL,
  fill jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS hl_paper_positions (
  position_id uuid PRIMARY KEY,
  order_id uuid,
  native_id text NOT NULL,
  qty numeric,
  entry_px numeric,
  mark_px numeric,
  funding_pnl numeric,
  realized_pnl numeric,
  status text NOT NULL,
  extra jsonb NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS hl_paper_traces (
  trace_id uuid PRIMARY KEY,
  order_id uuid,
  payload jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

INSERT INTO schema_versions(version, content_sha256)
VALUES ('024_hyperliquid_paper', 'pending')
ON CONFLICT (version) DO NOTHING;
COMMIT;
