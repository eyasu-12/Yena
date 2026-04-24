CREATE UNIQUE INDEX IF NOT EXISTS idx_observations_canonical_key
  ON observations(canonical_key)
  WHERE canonical_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_observations_type_canonical_key
  ON observations(observation_type, canonical_key);
