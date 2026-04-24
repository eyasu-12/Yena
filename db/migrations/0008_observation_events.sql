CREATE TABLE IF NOT EXISTS observation_events (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL,
  canonical_key TEXT NOT NULL,
  event_type TEXT NOT NULL,
  memory_item_id TEXT,
  previous_json TEXT,
  current_json TEXT NOT NULL,
  evidence_record_ids_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(observation_id) REFERENCES observations(id)
);

CREATE INDEX IF NOT EXISTS idx_observation_events_observation_created
  ON observation_events(observation_id, created_at);

CREATE INDEX IF NOT EXISTS idx_observation_events_canonical_created
  ON observation_events(canonical_key, created_at);
