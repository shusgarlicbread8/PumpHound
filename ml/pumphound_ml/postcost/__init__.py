"""Post-cost dataset construction: joins realized Execution Twin results (and, where the
resolver can find real forward market state, scenario replays of them) into leakage-safe,
failed-entry-aware ML labels. Everything downstream (Loss Filter, Tail Ranker, Exit Hazard v2)
trains on what this package produces, never on decision-time-only legacy labels."""
