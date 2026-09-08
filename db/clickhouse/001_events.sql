-- PumpHound ClickHouse baseline. Apply with clickhouse-client, not psql.
-- Rows are append-only revisions. Queries resolve revisions explicitly.
CREATE DATABASE IF NOT EXISTS pumphound;

CREATE TABLE IF NOT EXISTS pumphound.raw_sightings (
  sighting_id String,
  batch_id UUID,
  source LowCardinality(String),
  source_cursor String,
  slot UInt64,
  signature String,
  observed_at DateTime64(6, 'UTC'),
  commitment LowCardinality(String),
  payload_sha256 String,
  payload_codec LowCardinality(String),
  raw_payload String,
  raw_partition_uri String
) ENGINE = MergeTree
PARTITION BY toYYYYMMDD(observed_at)
ORDER BY (source, slot, sighting_id);

CREATE TABLE IF NOT EXISTS pumphound.canonical_event_revisions (
  event_id String,
  revision UInt64,
  action Enum8('UPSERT'=1, 'RETRACT'=2),
  protocol LowCardinality(String),
  kind LowCardinality(String),
  program_id String,
  signature String,
  slot UInt64,
  tx_index Nullable(UInt32),
  instruction_path Array(UInt32),
  event_index UInt32,
  block_time Nullable(DateTime64(6, 'UTC')),
  observed_at DateTime64(6, 'UTC'),
  computed_at DateTime64(6, 'UTC'),
  known_at DateTime64(6, 'UTC'),
  commitment LowCardinality(String),
  canonicality LowCardinality(String),
  transaction_succeeded Bool,
  mint String,
  market String,
  trader String,
  quote_mint String,
  base_amount_atomic UInt128,
  quote_amount_atomic UInt128,
  decoder_version String,
  idl_sha256 String,
  payload_json String,
  raw_partition_uri String
) ENGINE = MergeTree
PARTITION BY intDiv(slot, 1000000)
ORDER BY (program_id, slot, event_id, revision);

-- Consumers must select the last revision across all relevant rows and exclude
-- RETRACT/orphaned entries. Do not sum this revision table directly.
-- argMax(tuple(...), revision) is preferred over independent argMax nullable
-- columns, which can accidentally mix values from different revisions.

CREATE TABLE IF NOT EXISTS pumphound.market_snapshots (
  snapshot_id String,
  market String,
  mint String,
  quote_mint String,
  protocol LowCardinality(String),
  slot UInt64,
  tx_index Nullable(UInt32),
  event_index Nullable(UInt32),
  observed_at DateTime64(6, 'UTC'),
  knowledge_cutoff DateTime64(6, 'UTC'),
  computed_at DateTime64(6, 'UTC'),
  canonicality LowCardinality(String),
  virtual_base_atomic UInt128,
  virtual_quote_atomic UInt128,
  real_base_atomic UInt128,
  real_quote_atomic UInt128,
  effective_base_atomic UInt128,
  effective_quote_atomic UInt128,
  config_snapshot_id String,
  quality_json String,
  snapshot_json String
) ENGINE = MergeTree
PARTITION BY intDiv(slot, 1000000)
ORDER BY (market, slot, snapshot_id);

CREATE TABLE IF NOT EXISTS pumphound.feature_snapshots (
  feature_snapshot_id String,
  launch_id UUID,
  mint String,
  observation_window_seconds UInt32,
  cutoff_slot UInt64,
  knowledge_cutoff DateTime64(6, 'UTC'),
  computed_at DateTime64(6, 'UTC'),
  schema_version String,
  values Map(String, Nullable(Float64)),
  dependency_ids Array(String),
  quality_json String,
  simulation_run_id Nullable(UUID)
) ENGINE = MergeTree
PARTITION BY toYYYYMM(knowledge_cutoff)
ORDER BY (schema_version, launch_id, cutoff_slot, feature_snapshot_id);

CREATE TABLE IF NOT EXISTS pumphound.predictions (
  prediction_id String,
  feature_snapshot_id String,
  launch_id UUID,
  model_version String,
  calibrated Bool,
  computed_at DateTime64(6, 'UTC'),
  outputs_json String,
  explanation_json String,
  abstention_reason Nullable(String),
  simulation_run_id Nullable(UUID)
) ENGINE = MergeTree
PARTITION BY toYYYYMM(computed_at)
ORDER BY (launch_id, computed_at, prediction_id);

CREATE TABLE IF NOT EXISTS pumphound.outcome_labels (
  label_id String,
  launch_id UUID,
  feature_snapshot_id String,
  label_version String,
  horizon_seconds UInt32,
  matured_at DateTime64(6, 'UTC'),
  computed_at DateTime64(6, 'UTC'),
  censored Bool,
  ambiguous Bool,
  value Nullable(Float64),
  payload_json String,
  simulation_manifest_sha256 String
) ENGINE = MergeTree
PARTITION BY toYYYYMM(matured_at)
ORDER BY (label_version, launch_id, feature_snapshot_id, label_id);

CREATE TABLE IF NOT EXISTS pumphound.reference_prices (
  observation_id String,
  base_mint String,
  quote_currency LowCardinality(String),
  price_decimal Decimal(38,18),
  confidence_decimal Nullable(Decimal(38,18)),
  source LowCardinality(String),
  source_time DateTime64(6, 'UTC'),
  observed_at DateTime64(6, 'UTC'),
  expires_at DateTime64(6, 'UTC'),
  payload_sha256 String
) ENGINE = MergeTree
PARTITION BY toYYYYMM(source_time)
ORDER BY (base_mint, source_time, observation_id);

CREATE TABLE IF NOT EXISTS pumphound.provider_observations (
  observation_id String,
  provider LowCardinality(String),
  subject_type LowCardinality(String),
  subject_id String,
  source_time Nullable(DateTime64(6, 'UTC')),
  observed_at DateTime64(6, 'UTC'),
  expires_at DateTime64(6, 'UTC'),
  payload_sha256 String,
  payload_json String,
  quality_json String
) ENGINE = MergeTree
PARTITION BY toYYYYMM(observed_at)
ORDER BY (subject_type, subject_id, observed_at, observation_id);

CREATE TABLE IF NOT EXISTS pumphound.portfolio_marks (
  mark_id String,
  portfolio_id UUID,
  simulation_run_id UUID,
  at_time DateTime64(6, 'UTC'),
  slot UInt64,
  currency LowCardinality(String),
  cash_value Nullable(Decimal(38,18)),
  executable_position_value Nullable(Decimal(38,18)),
  equity_value Nullable(Decimal(38,18)),
  realized_pnl Nullable(Decimal(38,18)),
  unrealized_pnl Nullable(Decimal(38,18)),
  quality_json String
) ENGINE = MergeTree
PARTITION BY toYYYYMM(at_time)
ORDER BY (portfolio_id, simulation_run_id, at_time, mark_id);

-- Hourly/daily Parquet exports include partition manifest, row count, hash,
-- decoder/schema version and coverage. Add TTL only after exports are verified.
-- Monetary source of truth remains atomic quantities in PostgreSQL and market
-- snapshots; decimal portfolio marks are derived reporting values.

