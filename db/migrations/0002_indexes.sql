CREATE INDEX IF NOT EXISTS idx_evidence_source_type ON evidence_records(source_type);
CREATE INDEX IF NOT EXISTS idx_evidence_ingested_at ON evidence_records(ingested_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_evidence_idempotency
  ON evidence_records(source_type, source_ref, checksum);

CREATE INDEX IF NOT EXISTS idx_proposals_status ON memory_proposals(status);
CREATE INDEX IF NOT EXISTS idx_proposals_subject_key ON memory_proposals(subject_key);

CREATE INDEX IF NOT EXISTS idx_memory_items_canonical_key ON memory_items(canonical_key);
CREATE INDEX IF NOT EXISTS idx_memory_versions_item_id ON memory_item_versions(memory_item_id);
CREATE INDEX IF NOT EXISTS idx_memory_links_version_id ON memory_links(memory_item_version_id);
CREATE INDEX IF NOT EXISTS idx_memory_links_evidence_id ON memory_links(evidence_record_id);

CREATE INDEX IF NOT EXISTS idx_audit_agent_id ON retrieval_audit_events(agent_id);
CREATE INDEX IF NOT EXISTS idx_audit_created_at ON retrieval_audit_events(created_at);
