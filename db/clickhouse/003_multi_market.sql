CREATE DATABASE IF NOT EXISTS pumphound;

-- Short-retention high-frequency venue observations. Normalized long-term
-- identity/state lives in Postgres.
CREATE TABLE IF NOT EXISTS pumphound.venue_observations (
  venue LowCardinality(String),
  native_id String,
  observed_at DateTime64(3, 'UTC'),
  source_ts Nullable(DateTime64(3, 'UTC')),
  kind LowCardinality(String),
  payload String
) ENGINE = MergeTree
PARTITION BY toYYYYMMDD(observed_at)
ORDER BY (venue, native_id, observed_at)
TTL toDateTime(observed_at) + INTERVAL 14 DAY;
