PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS evidence_records (
  id TEXT PRIMARY KEY,
  source_type TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  content_type TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL,
  ingested_at TEXT NOT NULL,
  checksum TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_proposals (
  id TEXT PRIMARY KEY,
  proposal_type TEXT NOT NULL,
  subject_key TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  confidence REAL NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  resolved_at TEXT
);

CREATE TABLE IF NOT EXISTS memory_items (
  id TEXT PRIMARY KEY,
  memory_type TEXT NOT NULL,
  canonical_key TEXT NOT NULL UNIQUE,
  active_version_id TEXT,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_item_versions (
  id TEXT PRIMARY KEY,
  memory_item_id TEXT NOT NULL,
  version_number INTEGER NOT NULL,
  state TEXT NOT NULL,
  value_json TEXT NOT NULL,
  supersedes_version_id TEXT,
  valid_from TEXT NOT NULL,
  valid_to TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(memory_item_id) REFERENCES memory_items(id)
);

CREATE TABLE IF NOT EXISTS memory_links (
  id TEXT PRIMARY KEY,
  memory_item_version_id TEXT NOT NULL,
  evidence_record_id TEXT NOT NULL,
  link_type TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(memory_item_version_id) REFERENCES memory_item_versions(id),
  FOREIGN KEY(evidence_record_id) REFERENCES evidence_records(id)
);

CREATE TABLE IF NOT EXISTS agent_scopes (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  scope_name TEXT NOT NULL,
  scope_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS policy_rules (
  id TEXT PRIMARY KEY,
  rule_name TEXT NOT NULL,
  rule_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS retrieval_audit_events (
  id TEXT PRIMARY KEY,
  agent_id TEXT NOT NULL,
  request_type TEXT NOT NULL,
  scope_applied TEXT NOT NULL,
  shared_json TEXT NOT NULL,
  redacted_json TEXT,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS retention_jobs (
  id TEXT PRIMARY KEY,
  policy_name TEXT NOT NULL,
  target_type TEXT NOT NULL,
  status TEXT NOT NULL,
  run_at TEXT NOT NULL,
  completed_at TEXT
);
