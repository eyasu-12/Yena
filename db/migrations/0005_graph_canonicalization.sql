CREATE TABLE IF NOT EXISTS graph_entity_aliases (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  alias_name TEXT NOT NULL,
  canonical_entity_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(entity_type, alias_name),
  FOREIGN KEY(canonical_entity_id) REFERENCES graph_entities(id)
);

CREATE TABLE IF NOT EXISTS graph_predicate_aliases (
  id TEXT PRIMARY KEY,
  alias_predicate TEXT NOT NULL UNIQUE,
  canonical_predicate TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS graph_relationship_redirects (
  id TEXT PRIMARY KEY,
  source_relationship_id TEXT NOT NULL UNIQUE,
  target_relationship_id TEXT NOT NULL,
  reason_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(source_relationship_id) REFERENCES graph_relationships(id),
  FOREIGN KEY(target_relationship_id) REFERENCES graph_relationships(id)
);

CREATE TABLE IF NOT EXISTS graph_compaction_jobs (
  id TEXT PRIMARY KEY,
  dry_run INTEGER NOT NULL,
  status TEXT NOT NULL,
  summary_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_graph_entity_aliases_lookup
  ON graph_entity_aliases(entity_type, alias_name);

CREATE INDEX IF NOT EXISTS idx_graph_predicate_aliases_lookup
  ON graph_predicate_aliases(alias_predicate);

CREATE INDEX IF NOT EXISTS idx_graph_relationship_redirects_target
  ON graph_relationship_redirects(target_relationship_id);
