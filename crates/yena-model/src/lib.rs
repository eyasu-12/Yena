use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalScopeKind {
    Global,
    Repo,
    Workspace,
    Agent,
    Source,
}

impl RetrievalScopeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Repo => "repo",
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Source => "source",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalScope {
    pub kind: RetrievalScopeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryFreshness {
    New,
    Stable,
    Strengthening,
    Weakening,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbstentionReason {
    MissingEvidence,
    StaleMemory,
    StaleMemorySuperseded,
    Contradicted,
    OutOfScope,
    LowConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalTraceLifecycleEvent {
    pub event_type: String,
    pub created_at: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalTrace {
    pub candidate_source: String,
    pub candidate_id: String,
    pub matched_terms: Vec<String>,
    pub score_components: Value,
    pub scope_filter: String,
    pub redactions: Vec<String>,
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycle_events: Vec<RetrievalTraceLifecycleEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAnswer {
    pub statement: String,
    pub memory_type: String,
    pub freshness: MemoryFreshness,
    pub confidence: f32,
    pub evidence_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<RetrievalTrace>,
    pub redactions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAnswerContract {
    pub query: String,
    pub scope: RetrievalScope,
    pub should_abstain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_reason: Option<AbstentionReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_message: Option<String>,
    pub memories: Vec<MemoryAnswer>,
}
