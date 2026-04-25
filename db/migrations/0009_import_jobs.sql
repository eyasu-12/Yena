CREATE TABLE IF NOT EXISTS import_jobs (
  id TEXT PRIMARY KEY,
  source_type TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  status TEXT NOT NULL,
  commit_requested INTEGER NOT NULL,
  imported_items INTEGER NOT NULL,
  committed_items INTEGER NOT NULL,
  skipped_items INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  completed_at TEXT
);

CREATE TABLE IF NOT EXISTS import_job_items (
  id TEXT PRIMARY KEY,
  import_job_id TEXT NOT NULL,
  source_ref TEXT NOT NULL,
  section_path TEXT NOT NULL,
  statement TEXT NOT NULL,
  memory_type TEXT NOT NULL,
  status TEXT NOT NULL,
  evidence_record_id TEXT NOT NULL,
  proposal_id TEXT,
  memory_item_id TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(import_job_id) REFERENCES import_jobs(id),
  FOREIGN KEY(evidence_record_id) REFERENCES evidence_records(id),
  FOREIGN KEY(proposal_id) REFERENCES memory_proposals(id),
  FOREIGN KEY(memory_item_id) REFERENCES memory_items(id)
);

CREATE INDEX IF NOT EXISTS idx_import_jobs_source_created
  ON import_jobs(source_type, source_ref, created_at);

CREATE INDEX IF NOT EXISTS idx_import_job_items_job
  ON import_job_items(import_job_id);
