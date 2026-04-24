CREATE TABLE IF NOT EXISTS memory_item_metadata (
  memory_item_id TEXT PRIMARY KEY,
  scope_kind TEXT NOT NULL DEFAULT 'global',
  repo_path TEXT,
  repo_remote TEXT,
  branch TEXT,
  workspace_path TEXT,
  sensitivity TEXT NOT NULL DEFAULT 'normal',
  freshness TEXT NOT NULL DEFAULT 'stable',
  confidence REAL NOT NULL DEFAULT 1.0,
  decay_policy TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(memory_item_id) REFERENCES memory_items(id)
);

CREATE TABLE IF NOT EXISTS observations (
  id TEXT PRIMARY KEY,
  observation_type TEXT NOT NULL,
  statement TEXT NOT NULL,
  scope_kind TEXT NOT NULL DEFAULT 'global',
  repo_path TEXT,
  repo_remote TEXT,
  branch TEXT,
  workspace_path TEXT,
  proof_count INTEGER NOT NULL DEFAULT 0,
  confidence REAL NOT NULL DEFAULT 1.0,
  freshness TEXT NOT NULL DEFAULT 'new',
  contradiction_count INTEGER NOT NULL DEFAULT 0,
  last_verified_at TEXT,
  valid_from TEXT NOT NULL,
  valid_to TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS observation_memory_links (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL,
  memory_item_id TEXT NOT NULL,
  link_type TEXT NOT NULL DEFAULT 'supporting_memory',
  created_at TEXT NOT NULL,
  FOREIGN KEY(observation_id) REFERENCES observations(id),
  FOREIGN KEY(memory_item_id) REFERENCES memory_items(id)
);

CREATE TABLE IF NOT EXISTS observation_evidence_links (
  id TEXT PRIMARY KEY,
  observation_id TEXT NOT NULL,
  evidence_record_id TEXT NOT NULL,
  link_type TEXT NOT NULL DEFAULT 'supporting_evidence',
  created_at TEXT NOT NULL,
  FOREIGN KEY(observation_id) REFERENCES observations(id),
  FOREIGN KEY(evidence_record_id) REFERENCES evidence_records(id)
);

CREATE TABLE IF NOT EXISTS retrieval_traces (
  id TEXT PRIMARY KEY,
  audit_event_id TEXT,
  agent_id TEXT NOT NULL,
  query_text TEXT NOT NULL,
  scope_json TEXT NOT NULL,
  answer_json TEXT NOT NULL,
  trace_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(audit_event_id) REFERENCES retrieval_audit_events(id)
);

CREATE VIRTUAL TABLE IF NOT EXISTS retrieval_documents_fts USING fts5(
  source_type UNINDEXED,
  source_id UNINDEXED,
  scope_kind UNINDEXED,
  repo_path UNINDEXED,
  repo_remote UNINDEXED,
  branch UNINDEXED,
  title,
  body
);

CREATE INDEX IF NOT EXISTS idx_memory_metadata_scope
  ON memory_item_metadata(scope_kind, repo_path, repo_remote, branch, workspace_path);

CREATE INDEX IF NOT EXISTS idx_memory_metadata_freshness
  ON memory_item_metadata(freshness, confidence);

CREATE INDEX IF NOT EXISTS idx_observations_status_scope
  ON observations(status, scope_kind, repo_path, repo_remote, branch, workspace_path);

CREATE INDEX IF NOT EXISTS idx_observations_freshness
  ON observations(freshness, confidence);

CREATE INDEX IF NOT EXISTS idx_observation_memory_links_observation
  ON observation_memory_links(observation_id);

CREATE INDEX IF NOT EXISTS idx_observation_memory_links_memory
  ON observation_memory_links(memory_item_id);

CREATE INDEX IF NOT EXISTS idx_observation_evidence_links_observation
  ON observation_evidence_links(observation_id);

CREATE INDEX IF NOT EXISTS idx_observation_evidence_links_evidence
  ON observation_evidence_links(evidence_record_id);

CREATE INDEX IF NOT EXISTS idx_retrieval_traces_agent_created
  ON retrieval_traces(agent_id, created_at);

CREATE INDEX IF NOT EXISTS idx_retrieval_traces_audit
  ON retrieval_traces(audit_event_id);
