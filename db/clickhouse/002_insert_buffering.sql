-- The collector and core-api each issue one ClickHouse INSERT per NATS message
-- (often just a handful of rows), dozens of times per second. MergeTree creates
-- a new on-disk part per INSERT, so this pattern buried the background merge
-- executor and made the server unresponsive for minutes at a time (observed:
-- MergeTreeBackgroundExecutor lock acquisition stuck for 1-3s repeatedly, CPU
-- pinned at 300-500%). Buffer tables collect writes in memory and flush them to
-- the real MergeTree table as one larger part on a time/row/byte threshold,
-- whichever comes first. Readers keep querying the underlying table directly
-- (it holds everything older than the current buffer window); only inserts move
-- to the *_buffer table.
CREATE TABLE IF NOT EXISTS pumphound.raw_sightings_buffer AS pumphound.raw_sightings
  ENGINE = Buffer(pumphound, raw_sightings, 4, 1, 5, 1000, 50000, 100000, 10000000);

CREATE TABLE IF NOT EXISTS pumphound.canonical_event_revisions_buffer AS pumphound.canonical_event_revisions
  ENGINE = Buffer(pumphound, canonical_event_revisions, 4, 1, 5, 1000, 50000, 100000, 10000000);

CREATE TABLE IF NOT EXISTS pumphound.market_snapshots_buffer AS pumphound.market_snapshots
  ENGINE = Buffer(pumphound, market_snapshots, 4, 1, 5, 1000, 50000, 100000, 10000000);
