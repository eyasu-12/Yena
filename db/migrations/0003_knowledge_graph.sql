CREATE TABLE IF NOT EXISTS graph_entities (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL,
  canonical_name TEXT NOT NULL,
  attributes_json TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(entity_type, canonical_name)
);

CREATE TABLE IF NOT EXISTS graph_relationships (
  id TEXT PRIMARY KEY,
  canonical_key TEXT NOT NULL UNIQUE,
  subject_entity_id TEXT NOT NULL,
  predicate TEXT NOT NULL,
  object_entity_id TEXT NOT NULL,
  active_version_id TEXT,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY(subject_entity_id) REFERENCES graph_entities(id),
  FOREIGN KEY(object_entity_id) REFERENCES graph_entities(id)
);

CREATE TABLE IF NOT EXISTS graph_relationship_versions (
  id TEXT PRIMARY KEY,
  relationship_id TEXT NOT NULL,
  version_number INTEGER NOT NULL,
  state TEXT NOT NULL,
  attributes_json TEXT NOT NULL,
  supersedes_version_id TEXT,
  valid_from TEXT NOT NULL,
  valid_to TEXT,
  created_at TEXT NOT NULL,
  FOREIGN KEY(relationship_id) REFERENCES graph_relationships(id)
);

CREATE TABLE IF NOT EXISTS graph_relationship_evidence_links (
  id TEXT PRIMARY KEY,
  relationship_version_id TEXT NOT NULL,
  evidence_record_id TEXT NOT NULL,
  created_at TEXT NOT NULL,
  FOREIGN KEY(relationship_version_id) REFERENCES graph_relationship_versions(id),
  FOREIGN KEY(evidence_record_id) REFERENCES evidence_records(id)
);

CREATE INDEX IF NOT EXISTS idx_graph_entities_type_name
  ON graph_entities(entity_type, canonical_name);

CREATE INDEX IF NOT EXISTS idx_graph_relationships_subject
  ON graph_relationships(subject_entity_id);

CREATE INDEX IF NOT EXISTS idx_graph_relationships_object
  ON graph_relationships(object_entity_id);

CREATE INDEX IF NOT EXISTS idx_graph_relationships_predicate
  ON graph_relationships(predicate);

CREATE INDEX IF NOT EXISTS idx_graph_rel_versions_relationship
  ON graph_relationship_versions(relationship_id);

CREATE INDEX IF NOT EXISTS idx_graph_rel_evidence_version
  ON graph_relationship_evidence_links(relationship_version_id);

CREATE INDEX IF NOT EXISTS idx_graph_rel_evidence_record
  ON graph_relationship_evidence_links(evidence_record_id);
