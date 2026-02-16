use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: Uuid,
    pub source_type: String,
    pub source_ref: String,
    pub content_type: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub ingested_at: DateTime<Utc>,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProposal {
    pub id: Uuid,
    pub proposal_type: String,
    pub subject_key: String,
    pub payload_json: String,
    pub confidence: f32,
    pub status: String,
    pub created_at: DateTime<Utc>,
}
