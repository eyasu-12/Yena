use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env,
    net::SocketAddr,
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db_path: String,
}

#[derive(Debug, Deserialize)]
struct CreateProposalRequest {
    proposal_type: String,
    subject_key: String,
    memory_type: String,
    value_json: Value,
    #[serde(default)]
    evidence_record_ids: Vec<String>,
    scope: Option<MemoryScopePayload>,
    freshness: Option<String>,
    confidence: f32,
}

impl CreateProposalRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.proposal_type.trim().is_empty() {
            return Err(ApiError::bad_request("proposal_type is required"));
        }
        if self.subject_key.trim().is_empty() {
            return Err(ApiError::bad_request("subject_key is required"));
        }
        if self.memory_type.trim().is_empty() {
            return Err(ApiError::bad_request("memory_type is required"));
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(ApiError::bad_request(
                "confidence must be between 0.0 and 1.0",
            ));
        }
        if self.proposal_type.len() > 128 {
            return Err(ApiError::bad_request("proposal_type exceeds 128 chars"));
        }
        if self.subject_key.len() > 512 {
            return Err(ApiError::bad_request("subject_key exceeds 512 chars"));
        }
        if self.memory_type.len() > 128 {
            return Err(ApiError::bad_request("memory_type exceeds 128 chars"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ProposalPayload {
    memory_type: String,
    value_json: Value,
    evidence_record_ids: Vec<String>,
    #[serde(default)]
    scope: Option<MemoryScopePayload>,
    #[serde(default)]
    freshness: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MemoryScopePayload {
    kind: Option<String>,
    repo_path: Option<String>,
    repo_remote: Option<String>,
    branch: Option<String>,
    workspace_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateProposalResponse {
    id: String,
    status: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct CommitProposalResponse {
    proposal_id: String,
    memory_item_id: String,
    version_id: String,
    superseded_version_id: Option<String>,
    committed_at: String,
}

#[derive(Debug, Serialize)]
struct RejectProposalResponse {
    proposal_id: String,
    status: String,
    resolved_at: String,
}

#[derive(Debug, Deserialize, Clone)]
struct ImportMarkdownRequest {
    source_ref: String,
    content: String,
    source_type: Option<String>,
    scope: Option<MemoryScopePayload>,
    commit: Option<bool>,
    confidence: Option<f32>,
}

#[derive(Debug, Serialize)]
struct ImportMarkdownResponse {
    job_id: String,
    source_ref: String,
    imported_items: usize,
    committed_items: usize,
    skipped_items: usize,
    evidence_record_ids: Vec<String>,
    proposal_ids: Vec<String>,
    memory_item_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ImportJobResponse {
    job_id: String,
    source_type: String,
    source_ref: String,
    status: String,
    commit_requested: bool,
    imported_items: i64,
    committed_items: i64,
    skipped_items: i64,
    created_at: String,
    completed_at: Option<String>,
    items: Vec<ImportJobItemResponse>,
}

#[derive(Debug, Serialize)]
struct ImportJobItemResponse {
    item_id: String,
    source_ref: String,
    section_path: String,
    statement: String,
    memory_type: String,
    status: String,
    evidence_record_id: String,
    proposal_id: Option<String>,
    memory_item_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ForgetImportSourceRequest {
    source_ref: String,
    source_type: Option<String>,
    #[serde(default = "default_true")]
    forget_evidence: bool,
}

#[derive(Debug, Serialize)]
struct ForgetImportSourceResponse {
    source_ref: String,
    source_type: Option<String>,
    matched_memory_items: usize,
    deleted_memory_items: usize,
    deleted_versions: usize,
    deleted_links: usize,
    deleted_proposals: usize,
    deleted_evidence: usize,
}

type ImportJobRow = (
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    Option<String>,
);

#[derive(Debug, Clone)]
struct ParsedMarkdownMemory {
    section_path: String,
    statement: String,
    memory_type: String,
}

#[derive(Debug)]
struct CommittedMemoryIds {
    memory_item_id: String,
    version_id: String,
    superseded_version_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForgetMemoryRequest {
    canonical_key: String,
    #[serde(default = "default_true")]
    forget_evidence: bool,
}

#[derive(Debug, Serialize)]
struct ForgetMemoryResponse {
    canonical_key: String,
    deleted_memory_item_id: String,
    deleted_versions: usize,
    deleted_links: usize,
    deleted_evidence: usize,
}

#[derive(Debug, Serialize)]
struct MemoryViewResponse {
    memory_item_id: String,
    canonical_key: String,
    memory_type: String,
    active_version_id: String,
    value_json: Value,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemoryHistoryResponse {
    memory_item_id: String,
    canonical_key: String,
    memory_type: String,
    versions: Vec<MemoryVersionHistoryEntry>,
}

#[derive(Debug, Serialize)]
struct MemoryVersionHistoryEntry {
    version_id: String,
    version_number: i64,
    state: String,
    value_json: Value,
    supersedes_version_id: Option<String>,
    valid_from: String,
    valid_to: Option<String>,
    created_at: String,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservationViewResponse {
    observation_id: String,
    canonical_key: String,
    observation_type: String,
    statement: String,
    scope_kind: String,
    repo_path: Option<String>,
    repo_remote: Option<String>,
    branch: Option<String>,
    workspace_path: Option<String>,
    proof_count: i64,
    confidence: f32,
    freshness: String,
    contradiction_count: i64,
    last_verified_at: Option<String>,
    valid_from: String,
    valid_to: Option<String>,
    status: String,
    updated_at: String,
    memory_item_ids: Vec<String>,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ObservationHistoryResponse {
    observation_id: String,
    canonical_key: String,
    events: Vec<ObservationEventEntry>,
}

#[derive(Debug, Serialize)]
struct ObservationEventEntry {
    event_id: String,
    event_type: String,
    memory_item_id: Option<String>,
    previous_json: Option<Value>,
    current_json: Value,
    evidence_record_ids: Vec<String>,
    created_at: String,
}

type ObservationViewRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    f32,
    String,
    i64,
    Option<String>,
    String,
    Option<String>,
    String,
    String,
);

#[derive(Debug, Deserialize)]
struct UpsertRetentionPolicyRequest {
    policy_name: String,
    memory_type: Option<String>,
    canonical_prefix: Option<String>,
    max_age_days: i64,
    #[serde(default)]
    forget_evidence: bool,
    #[serde(default = "default_true")]
    enabled: bool,
}

impl UpsertRetentionPolicyRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.policy_name.trim().is_empty() {
            return Err(ApiError::bad_request("policy_name is required"));
        }
        if self.policy_name.len() > 128 {
            return Err(ApiError::bad_request("policy_name exceeds 128 chars"));
        }
        if self.max_age_days < 1 {
            return Err(ApiError::bad_request("max_age_days must be >= 1"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UpsertRetentionPolicyResponse {
    policy_name: String,
    memory_type: Option<String>,
    canonical_prefix: Option<String>,
    max_age_days: i64,
    forget_evidence: bool,
    enabled: bool,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RunRetentionRequest {
    #[serde(default)]
    policy_names: Vec<String>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct RunRetentionResponse {
    run_at: String,
    dry_run: bool,
    policies: Vec<RetentionRunPolicyResult>,
}

#[derive(Debug, Serialize)]
struct RetentionRunPolicyResult {
    policy_name: String,
    job_id: String,
    matched_memory_items: usize,
    deleted_memory_items: usize,
    deleted_versions: usize,
    deleted_links: usize,
    deleted_evidence: usize,
    status: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct RetentionPolicyPayload {
    policy_name: String,
    memory_type: Option<String>,
    canonical_prefix: Option<String>,
    max_age_days: i64,
    #[serde(default)]
    forget_evidence: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct GraphEntityRef {
    entity_type: String,
    canonical_name: String,
}

impl GraphEntityRef {
    fn validate(&self, field_prefix: &str) -> Result<(), ApiError> {
        if self.entity_type.trim().is_empty() {
            return Err(ApiError::bad_request(format!(
                "{}.entity_type is required",
                field_prefix
            )));
        }
        if self.canonical_name.trim().is_empty() {
            return Err(ApiError::bad_request(format!(
                "{}.canonical_name is required",
                field_prefix
            )));
        }
        if self.entity_type.len() > 128 {
            return Err(ApiError::bad_request(format!(
                "{}.entity_type exceeds 128 chars",
                field_prefix
            )));
        }
        if self.canonical_name.len() > 256 {
            return Err(ApiError::bad_request(format!(
                "{}.canonical_name exceeds 256 chars",
                field_prefix
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct CreateGraphRelationshipProposalRequest {
    subject: GraphEntityRef,
    predicate: String,
    object: GraphEntityRef,
    #[serde(default = "empty_json_object")]
    attributes_json: Value,
    #[serde(default)]
    evidence_record_ids: Vec<String>,
    confidence: f32,
}

impl CreateGraphRelationshipProposalRequest {
    fn validate(&self) -> Result<(), ApiError> {
        self.subject.validate("subject")?;
        self.object.validate("object")?;
        if self.predicate.trim().is_empty() {
            return Err(ApiError::bad_request("predicate is required"));
        }
        if self.predicate.len() > 128 {
            return Err(ApiError::bad_request("predicate exceeds 128 chars"));
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(ApiError::bad_request(
                "confidence must be between 0.0 and 1.0",
            ));
        }
        if !self.attributes_json.is_object() {
            return Err(ApiError::bad_request(
                "attributes_json must be a JSON object",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct UpsertGraphEntityAliasRequest {
    entity_type: String,
    alias_name: String,
    canonical_name: String,
}

impl UpsertGraphEntityAliasRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.entity_type.trim().is_empty() {
            return Err(ApiError::bad_request("entity_type is required"));
        }
        if self.alias_name.trim().is_empty() {
            return Err(ApiError::bad_request("alias_name is required"));
        }
        if self.canonical_name.trim().is_empty() {
            return Err(ApiError::bad_request("canonical_name is required"));
        }
        if self.entity_type.len() > 128 {
            return Err(ApiError::bad_request("entity_type exceeds 128 chars"));
        }
        if self.alias_name.len() > 256 {
            return Err(ApiError::bad_request("alias_name exceeds 256 chars"));
        }
        if self.canonical_name.len() > 256 {
            return Err(ApiError::bad_request("canonical_name exceeds 256 chars"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UpsertGraphEntityAliasResponse {
    entity_type: String,
    alias_name: String,
    canonical_entity_id: String,
    canonical_name: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct UpsertGraphPredicateAliasRequest {
    alias_predicate: String,
    canonical_predicate: String,
}

impl UpsertGraphPredicateAliasRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.alias_predicate.trim().is_empty() {
            return Err(ApiError::bad_request("alias_predicate is required"));
        }
        if self.canonical_predicate.trim().is_empty() {
            return Err(ApiError::bad_request("canonical_predicate is required"));
        }
        if self.alias_predicate.len() > 128 {
            return Err(ApiError::bad_request("alias_predicate exceeds 128 chars"));
        }
        if self.canonical_predicate.len() > 128 {
            return Err(ApiError::bad_request(
                "canonical_predicate exceeds 128 chars",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct UpsertGraphPredicateAliasResponse {
    alias_predicate: String,
    canonical_predicate: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct RunGraphCompactionRequest {
    #[serde(default = "default_true")]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct RunGraphCompactionResponse {
    job_id: String,
    dry_run: bool,
    status: String,
    run_at: String,
    entity_alias_rules: usize,
    predicate_alias_rules: usize,
    canonicalized_relationships: usize,
    redirected_relationships: usize,
    merged_versions_created: usize,
    compacted_entities: usize,
}

#[derive(Debug, Deserialize, Serialize)]
struct GraphRelationshipProposalPayload {
    subject: GraphEntityRef,
    predicate: String,
    object: GraphEntityRef,
    attributes_json: Value,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CreateGraphRelationshipProposalResponse {
    proposal_id: String,
    status: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct CommitGraphProposalResponse {
    proposal_id: String,
    relationship_id: String,
    version_id: String,
    superseded_version_id: Option<String>,
    committed_at: String,
}

#[derive(Debug, Serialize)]
struct GraphRelationshipViewResponse {
    relationship_id: String,
    subject: GraphEntityRef,
    predicate: String,
    object: GraphEntityRef,
    active_version_id: String,
    confidence: f32,
    attributes_json: Value,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct GraphRelationshipHistoryResponse {
    relationship_id: String,
    subject: GraphEntityRef,
    predicate: String,
    object: GraphEntityRef,
    versions: Vec<GraphRelationshipVersionEntry>,
}

#[derive(Debug, Serialize)]
struct GraphRelationshipVersionEntry {
    version_id: String,
    version_number: i64,
    state: String,
    confidence: f32,
    attributes_json: Value,
    supersedes_version_id: Option<String>,
    valid_from: String,
    valid_to: Option<String>,
    created_at: String,
    evidence_record_ids: Vec<String>,
}

type GraphRelationshipViewRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    f32,
    String,
    String,
);

#[derive(Debug, Clone)]
struct GraphCompactionCandidate {
    relationship_id: String,
    canonical_key: String,
    subject_entity_id: String,
    subject_entity_type: String,
    subject_canonical_name: String,
    predicate: String,
    object_entity_id: String,
    object_entity_type: String,
    object_canonical_name: String,
    active_version_id: String,
    active_confidence: f32,
    active_attributes_json: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct GraphCompactionPlanEntry {
    candidate: GraphCompactionCandidate,
    target_subject_entity_id: String,
    target_subject_canonical_name: String,
    target_object_entity_id: String,
    target_object_canonical_name: String,
    target_predicate: String,
    target_canonical_key: String,
}

#[derive(Debug, Default)]
struct GraphCompactionCounts {
    canonicalized_relationships: usize,
    redirected_relationships: usize,
    merged_versions_created: usize,
    compacted_entities: usize,
}

#[derive(Debug, Default)]
struct DeleteCounts {
    deleted_versions: usize,
    deleted_links: usize,
    deleted_proposals: usize,
    deleted_evidence: usize,
}

#[derive(Debug, Default)]
struct PendingImportDeleteCounts {
    deleted_proposals: usize,
    deleted_evidence: usize,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorResponse {
            error: self.message,
        });
        (self.status, body).into_response()
    }
}

#[derive(Debug)]
struct ProposalRow {
    id: String,
    proposal_type: String,
    subject_key: String,
    payload_json: String,
    confidence: f32,
    status: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "memory_compiler=info,axum=info".into()),
        )
        .init();

    let db_path = env::var("YENA_DB_PATH").unwrap_or_else(|_| "data/yena.db".to_string());
    ensure_data_dir(&db_path)?;
    init_db(&db_path)?;

    let bind = env::var("YENA_BIND").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid YENA_BIND address: {}", bind))?;

    let state = AppState { db_path };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/import/markdown", post(import_markdown))
        .route("/v1/import/jobs/{id}", get(get_import_job))
        .route("/v1/import/sources/forget", post(forget_import_source))
        .route("/v1/proposals", post(create_proposal))
        .route("/v1/proposals/{id}/commit", post(commit_proposal))
        .route("/v1/proposals/{id}/reject", post(reject_proposal))
        .route("/v1/memory/{canonical_key}", get(get_memory))
        .route(
            "/v1/memory/{canonical_key}/history",
            get(get_memory_history),
        )
        .route("/v1/observations/{canonical_key}", get(get_observation))
        .route(
            "/v1/observations/{canonical_key}/history",
            get(get_observation_history),
        )
        .route("/v1/memory/forget", post(forget_memory))
        .route(
            "/v1/graph/proposals/relationships",
            post(create_graph_relationship_proposal),
        )
        .route(
            "/v1/graph/proposals/{id}/commit",
            post(commit_graph_relationship_proposal),
        )
        .route(
            "/v1/graph/canonicalization/entity-aliases/upsert",
            post(upsert_graph_entity_alias),
        )
        .route(
            "/v1/graph/canonicalization/predicate-aliases/upsert",
            post(upsert_graph_predicate_alias),
        )
        .route("/v1/graph/compaction/run", post(run_graph_compaction))
        .route("/v1/graph/relationships/{id}", get(get_graph_relationship))
        .route(
            "/v1/graph/relationships/{id}/history",
            get(get_graph_relationship_history),
        )
        .route(
            "/v1/retention/policies/upsert",
            post(upsert_retention_policy),
        )
        .route("/v1/retention/jobs/run", post(run_retention_jobs))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("memory-compiler listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn import_markdown(
    State(state): State<AppState>,
    Json(payload): Json<ImportMarkdownRequest>,
) -> Result<Json<ImportMarkdownResponse>, ApiError> {
    validate_import_markdown_request(&payload)?;

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start import tx: {}", e)))?;
    let imported_at = Utc::now().to_rfc3339();
    let response = import_markdown_content(&tx, &payload, &imported_at)?;
    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit markdown import: {}", e)))?;

    Ok(Json(response))
}

async fn get_import_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ImportJobResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;
    Ok(Json(load_import_job(&conn, &id)?))
}

async fn forget_import_source(
    State(state): State<AppState>,
    Json(payload): Json<ForgetImportSourceRequest>,
) -> Result<Json<ForgetImportSourceResponse>, ApiError> {
    let mut conn = open_db(&state.db_path)?;
    let tx = conn.transaction().map_err(|e| {
        ApiError::internal(format!("failed to start import source forget tx: {}", e))
    })?;
    let response = forget_import_source_tx(&tx, payload)?;
    tx.commit().map_err(|e| {
        ApiError::internal(format!("failed to commit import source forget tx: {}", e))
    })?;

    Ok(Json(response))
}

fn forget_import_source_tx(
    conn: &Connection,
    payload: ForgetImportSourceRequest,
) -> Result<ForgetImportSourceResponse, ApiError> {
    let source_ref = payload.source_ref.trim().to_string();
    if source_ref.is_empty() {
        return Err(ApiError::bad_request("source_ref is required"));
    }
    if source_ref.len() > 512 {
        return Err(ApiError::bad_request("source_ref exceeds 512 chars"));
    }

    let source_type = trim_optional(payload.source_type);
    if source_type.as_ref().is_some_and(|value| value.len() > 128) {
        return Err(ApiError::bad_request("source_type exceeds 128 chars"));
    }
    let memory_item_ids = load_import_source_memory_ids(conn, &source_ref, source_type.as_deref())?;
    let mut counts = DeleteCounts::default();
    let mut deleted_memory_items = 0usize;
    for memory_item_id in &memory_item_ids {
        let item_counts =
            delete_memory_item_by_id_tx(conn, memory_item_id, payload.forget_evidence)?;
        counts.deleted_versions += item_counts.deleted_versions;
        counts.deleted_links += item_counts.deleted_links;
        counts.deleted_proposals += item_counts.deleted_proposals;
        counts.deleted_evidence += item_counts.deleted_evidence;
        deleted_memory_items += 1;
    }
    let pending_counts = delete_import_source_pending_proposals(
        conn,
        &source_ref,
        source_type.as_deref(),
        payload.forget_evidence,
    )?;

    Ok(ForgetImportSourceResponse {
        source_ref,
        source_type,
        matched_memory_items: memory_item_ids.len(),
        deleted_memory_items,
        deleted_versions: counts.deleted_versions,
        deleted_links: counts.deleted_links,
        deleted_proposals: counts.deleted_proposals + pending_counts.deleted_proposals,
        deleted_evidence: counts.deleted_evidence + pending_counts.deleted_evidence,
    })
}

async fn create_proposal(
    State(state): State<AppState>,
    Json(payload): Json<CreateProposalRequest>,
) -> Result<(StatusCode, Json<CreateProposalResponse>), ApiError> {
    payload.validate()?;

    let created_at = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();

    let proposal_payload = ProposalPayload {
        memory_type: payload.memory_type,
        value_json: payload.value_json,
        evidence_record_ids: payload.evidence_record_ids,
        scope: payload.scope,
        freshness: payload.freshness,
    };
    let payload_json = serde_json::to_string(&proposal_payload)
        .map_err(|e| ApiError::internal(format!("failed to encode proposal payload: {}", e)))?;

    let conn = open_db(&state.db_path)?;
    conn.execute(
        "
        INSERT INTO memory_proposals (
          id, proposal_type, subject_key, payload_json, confidence, status, created_at, resolved_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, NULL)
        ",
        params![
            id,
            payload.proposal_type,
            payload.subject_key,
            payload_json,
            payload.confidence,
            created_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert proposal: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateProposalResponse {
            id,
            status: "pending".to_string(),
            created_at,
        }),
    ))
}

fn validate_import_markdown_request(payload: &ImportMarkdownRequest) -> Result<(), ApiError> {
    if payload.source_ref.trim().is_empty() {
        return Err(ApiError::bad_request("source_ref is required"));
    }
    if payload.content.trim().is_empty() {
        return Err(ApiError::bad_request("content is required"));
    }
    if payload.source_ref.len() > 512 {
        return Err(ApiError::bad_request("source_ref exceeds 512 chars"));
    }
    if let Some(source_type) = &payload.source_type {
        if source_type.len() > 128 {
            return Err(ApiError::bad_request("source_type exceeds 128 chars"));
        }
    }
    if let Some(confidence) = payload.confidence {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(ApiError::bad_request(
                "confidence must be between 0.0 and 1.0",
            ));
        }
    }
    Ok(())
}

fn import_markdown_content(
    tx: &Transaction<'_>,
    payload: &ImportMarkdownRequest,
    imported_at: &str,
) -> Result<ImportMarkdownResponse, ApiError> {
    let parsed_items = parse_markdown_memory_items(&payload.content);
    if parsed_items.is_empty() {
        return Err(ApiError::bad_request(
            "content did not contain importable markdown memory items",
        ));
    }

    let source_type = payload
        .source_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local_markdown_memory");
    let should_commit = payload.commit.unwrap_or(true);
    let confidence = payload.confidence.unwrap_or(0.74);
    let job_id = Uuid::new_v4().to_string();
    let mut evidence_record_ids = Vec::new();
    let mut proposal_ids = Vec::new();
    let mut memory_item_ids = Vec::new();
    let mut skipped_items = 0usize;
    let source_slug = stable_key_fragment(&payload.source_ref);

    insert_import_job(
        tx,
        &job_id,
        source_type,
        &payload.source_ref,
        should_commit,
        imported_at,
    )?;

    for item in parsed_items {
        let item_checksum = checksum_hex(item.statement.as_bytes());
        let item_ref = format!(
            "{}#{}:{}",
            payload.source_ref,
            stable_key_fragment(&item.section_path),
            &item_checksum[..12]
        );
        let evidence_id = upsert_import_evidence(
            tx,
            source_type,
            &item_ref,
            "text/markdown",
            &item.statement,
            imported_at,
        )?;
        let subject_key = format!(
            "import.markdown.{}.{}.{}",
            source_slug,
            stable_key_fragment(&item.section_path),
            &item_checksum[..12]
        );
        let value_json = json!({
            "statement": item.statement,
            "source_ref": payload.source_ref,
            "section": item.section_path,
            "import_kind": "markdown_memory"
        });
        let evidence_ids = vec![evidence_id.clone()];
        evidence_record_ids.push(evidence_id.clone());

        if let Some(existing_memory_id) = should_commit
            .then(|| matching_active_import_memory_id(tx, &subject_key, &value_json, &evidence_ids))
            .transpose()?
            .flatten()
        {
            skipped_items += 1;
            insert_import_job_item(
                tx,
                &job_id,
                &payload.source_ref,
                &item,
                "skipped",
                &evidence_id,
                None,
                Some(&existing_memory_id),
                imported_at,
            )?;
            continue;
        }

        let proposal_id = insert_import_proposal(
            tx,
            &subject_key,
            &item.memory_type,
            &value_json,
            &evidence_ids,
            payload.scope.as_ref(),
            confidence,
            should_commit,
            imported_at,
        )?;
        if should_commit {
            let committed = commit_memory_item(
                tx,
                &item.memory_type,
                &subject_key,
                &value_json,
                &evidence_ids,
                payload.scope.as_ref(),
                Some("stable"),
                confidence,
                imported_at,
            )?;
            insert_import_job_item(
                tx,
                &job_id,
                &payload.source_ref,
                &item,
                "committed",
                &evidence_id,
                Some(&proposal_id),
                Some(&committed.memory_item_id),
                imported_at,
            )?;
            memory_item_ids.push(committed.memory_item_id);
        } else {
            insert_import_job_item(
                tx,
                &job_id,
                &payload.source_ref,
                &item,
                "pending",
                &evidence_id,
                Some(&proposal_id),
                None,
                imported_at,
            )?;
        }
        proposal_ids.push(proposal_id);
    }

    complete_import_job(
        tx,
        &job_id,
        evidence_record_ids.len(),
        memory_item_ids.len(),
        skipped_items,
        imported_at,
    )?;

    Ok(ImportMarkdownResponse {
        job_id,
        source_ref: payload.source_ref.clone(),
        imported_items: evidence_record_ids.len(),
        committed_items: memory_item_ids.len(),
        skipped_items,
        evidence_record_ids,
        proposal_ids,
        memory_item_ids,
    })
}

#[allow(clippy::too_many_arguments)]
fn insert_import_proposal(
    tx: &Transaction<'_>,
    subject_key: &str,
    memory_type: &str,
    value_json: &Value,
    evidence_record_ids: &[String],
    scope: Option<&MemoryScopePayload>,
    confidence: f32,
    committed: bool,
    created_at: &str,
) -> Result<String, ApiError> {
    let proposal_id = Uuid::new_v4().to_string();
    let payload = ProposalPayload {
        memory_type: memory_type.to_string(),
        value_json: value_json.clone(),
        evidence_record_ids: evidence_record_ids.to_vec(),
        scope: scope.cloned(),
        freshness: Some("stable".to_string()),
    };
    let status = if committed { "committed" } else { "pending" };
    let resolved_at = if committed { Some(created_at) } else { None };
    tx.execute(
        "
        INSERT INTO memory_proposals (
          id, proposal_type, subject_key, payload_json, confidence, status, created_at, resolved_at
        ) VALUES (?1, 'markdown_import', ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            proposal_id,
            subject_key,
            serde_json::to_string(&payload).map_err(|e| {
                ApiError::internal(format!("failed to encode import proposal payload: {}", e))
            })?,
            confidence,
            status,
            created_at,
            resolved_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert import proposal: {}", e)))?;
    Ok(proposal_id)
}

fn upsert_import_evidence(
    tx: &Transaction<'_>,
    source_type: &str,
    source_ref: &str,
    content_type: &str,
    content: &str,
    imported_at: &str,
) -> Result<String, ApiError> {
    let checksum = checksum_hex(content.as_bytes());
    if let Some(existing_id) = tx
        .query_row(
            "
            SELECT id
            FROM evidence_records
            WHERE source_type = ?1
              AND source_ref = ?2
              AND checksum = ?3
            LIMIT 1
            ",
            params![source_type, source_ref, &checksum],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup import evidence: {}", e)))?
    {
        return Ok(existing_id);
    }

    let evidence_id = Uuid::new_v4().to_string();
    tx.execute(
        "
        INSERT INTO evidence_records (
          id, source_type, source_ref, content_type, content, created_at, ingested_at, checksum
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)
        ",
        params![
            evidence_id,
            source_type,
            source_ref,
            content_type,
            content,
            imported_at,
            checksum,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert import evidence: {}", e)))?;
    Ok(evidence_id)
}

fn insert_import_job(
    tx: &Transaction<'_>,
    job_id: &str,
    source_type: &str,
    source_ref: &str,
    commit_requested: bool,
    created_at: &str,
) -> Result<(), ApiError> {
    tx.execute(
        "
        INSERT INTO import_jobs (
          id, source_type, source_ref, status, commit_requested, imported_items,
          committed_items, skipped_items, created_at, completed_at
        ) VALUES (?1, ?2, ?3, 'running', ?4, 0, 0, 0, ?5, NULL)
        ",
        params![
            job_id,
            source_type,
            source_ref,
            if commit_requested { 1 } else { 0 },
            created_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert import job: {}", e)))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_import_job_item(
    tx: &Transaction<'_>,
    job_id: &str,
    source_ref: &str,
    item: &ParsedMarkdownMemory,
    status: &str,
    evidence_record_id: &str,
    proposal_id: Option<&str>,
    memory_item_id: Option<&str>,
    created_at: &str,
) -> Result<(), ApiError> {
    tx.execute(
        "
        INSERT INTO import_job_items (
          id, import_job_id, source_ref, section_path, statement, memory_type,
          status, evidence_record_id, proposal_id, memory_item_id, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ",
        params![
            Uuid::new_v4().to_string(),
            job_id,
            source_ref,
            &item.section_path,
            &item.statement,
            &item.memory_type,
            status,
            evidence_record_id,
            proposal_id,
            memory_item_id,
            created_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert import job item: {}", e)))?;
    Ok(())
}

fn complete_import_job(
    tx: &Transaction<'_>,
    job_id: &str,
    imported_items: usize,
    committed_items: usize,
    skipped_items: usize,
    completed_at: &str,
) -> Result<(), ApiError> {
    tx.execute(
        "
        UPDATE import_jobs
        SET status = 'completed',
            imported_items = ?2,
            committed_items = ?3,
            skipped_items = ?4,
            completed_at = ?5
        WHERE id = ?1
        ",
        params![
            job_id,
            imported_items as i64,
            committed_items as i64,
            skipped_items as i64,
            completed_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to complete import job: {}", e)))?;
    Ok(())
}

fn matching_active_import_memory_id(
    tx: &Transaction<'_>,
    subject_key: &str,
    value_json: &Value,
    evidence_record_ids: &[String],
) -> Result<Option<String>, ApiError> {
    let row: Option<(String, String, String)> = tx
        .query_row(
            "
            SELECT mi.id, mi.active_version_id, mv.value_json
            FROM memory_items mi
            JOIN memory_item_versions mv ON mv.id = mi.active_version_id
            WHERE mi.canonical_key = ?1
              AND mi.status = 'active'
            LIMIT 1
            ",
            params![subject_key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup imported memory: {}", e)))?;

    let Some((memory_item_id, active_version_id, active_value_raw)) = row else {
        return Ok(None);
    };
    let active_value: Value = serde_json::from_str(&active_value_raw).map_err(|e| {
        ApiError::internal(format!("failed to decode imported active value: {}", e))
    })?;
    if active_value != *value_json {
        return Ok(None);
    }

    let active_evidence = load_evidence_for_version(tx, &active_version_id)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let requested_evidence = evidence_record_ids.iter().cloned().collect::<BTreeSet<_>>();
    if active_evidence == requested_evidence {
        Ok(Some(memory_item_id))
    } else {
        Ok(None)
    }
}

fn load_import_job(conn: &Connection, job_id: &str) -> Result<ImportJobResponse, ApiError> {
    let job: Option<ImportJobRow> = conn
        .query_row(
            "
            SELECT source_type, source_ref, status, commit_requested, imported_items,
              committed_items, skipped_items, created_at, completed_at
            FROM import_jobs
            WHERE id = ?1
            LIMIT 1
            ",
            params![job_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup import job: {}", e)))?;
    let (
        source_type,
        source_ref,
        status,
        commit_requested,
        imported_items,
        committed_items,
        skipped_items,
        created_at,
        completed_at,
    ) = job.ok_or_else(|| ApiError::not_found("import job not found"))?;

    Ok(ImportJobResponse {
        job_id: job_id.to_string(),
        source_type,
        source_ref,
        status,
        commit_requested: commit_requested != 0,
        imported_items,
        committed_items,
        skipped_items,
        created_at,
        completed_at,
        items: load_import_job_items(conn, job_id)?,
    })
}

fn load_import_job_items(
    conn: &Connection,
    job_id: &str,
) -> Result<Vec<ImportJobItemResponse>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id, source_ref, section_path, statement, memory_type, status,
              evidence_record_id, proposal_id, memory_item_id, created_at
            FROM import_job_items
            WHERE import_job_id = ?1
            ORDER BY created_at ASC, id ASC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare import items: {}", e)))?;
    let rows = stmt
        .query_map(params![job_id], |row| {
            Ok(ImportJobItemResponse {
                item_id: row.get(0)?,
                source_ref: row.get(1)?,
                section_path: row.get(2)?,
                statement: row.get(3)?,
                memory_type: row.get(4)?,
                status: row.get(5)?,
                evidence_record_id: row.get(6)?,
                proposal_id: row.get(7)?,
                memory_item_id: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to query import job items: {}", e)))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read import job items: {}", e)))
}

fn parse_markdown_memory_items(content: &str) -> Vec<ParsedMarkdownMemory> {
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut in_code_fence = false;
    let mut items = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence || line.is_empty() {
            continue;
        }
        if let Some((level, title)) = parse_markdown_heading(line) {
            headings.retain(|(existing_level, _)| *existing_level < level);
            headings.push((level, clean_markdown_text(title)));
            continue;
        }

        let Some(statement) = parse_markdown_statement(line) else {
            continue;
        };
        let section_path = if headings.is_empty() {
            "root".to_string()
        } else {
            headings
                .iter()
                .map(|(_, title)| title.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let memory_type = infer_markdown_memory_type(&section_path, &statement);
        items.push(ParsedMarkdownMemory {
            section_path,
            statement,
            memory_type,
        });
    }

    items
}

fn parse_markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let title = line[level..].trim();
    if title.is_empty() {
        None
    } else {
        Some((level, title))
    }
}

fn parse_markdown_statement(line: &str) -> Option<String> {
    let stripped = strip_markdown_list_marker(line).unwrap_or(line).trim();
    let cleaned = clean_markdown_text(stripped);
    if cleaned.len() < 8 {
        None
    } else {
        Some(cleaned)
    }
}

fn strip_markdown_list_marker(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest);
        }
    }
    let (number, rest) = line.split_once(". ")?;
    if !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

fn clean_markdown_text(value: &str) -> String {
    value
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .trim()
        .to_string()
}

fn infer_markdown_memory_type(section_path: &str, statement: &str) -> String {
    let haystack = format!("{} {}", section_path, statement).to_lowercase();
    if haystack.contains("prefer") || haystack.contains("preference") {
        "preference".to_string()
    } else {
        "project".to_string()
    }
}

async fn commit_proposal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CommitProposalResponse>, ApiError> {
    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    let proposal = load_proposal(&tx, &id)?;
    if proposal.proposal_type == "graph_relationship" {
        return Err(ApiError::bad_request(
            "graph_relationship proposals must be committed via /v1/graph/proposals/{id}/commit",
        ));
    }
    if proposal.status != "pending" {
        return Err(ApiError::conflict(format!(
            "proposal {} has status '{}' and cannot be committed",
            proposal.id, proposal.status
        )));
    }

    let payload: ProposalPayload = serde_json::from_str(&proposal.payload_json)
        .map_err(|e| ApiError::internal(format!("failed to decode payload_json: {}", e)))?;

    let committed_at = Utc::now().to_rfc3339();
    let committed = commit_memory_item(
        &tx,
        &payload.memory_type,
        &proposal.subject_key,
        &payload.value_json,
        &payload.evidence_record_ids,
        payload.scope.as_ref(),
        payload.freshness.as_deref(),
        proposal.confidence,
        &committed_at,
    )?;

    tx.execute(
        "
        UPDATE memory_proposals
        SET status = 'committed', resolved_at = ?2
        WHERE id = ?1
        ",
        params![proposal.id, committed_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to update proposal status: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(CommitProposalResponse {
        proposal_id: id,
        memory_item_id: committed.memory_item_id,
        version_id: committed.version_id,
        superseded_version_id: committed.superseded_version_id,
        committed_at,
    }))
}

#[allow(clippy::too_many_arguments)]
fn commit_memory_item(
    tx: &Transaction<'_>,
    memory_type: &str,
    subject_key: &str,
    value_json: &Value,
    evidence_record_ids: &[String],
    scope: Option<&MemoryScopePayload>,
    freshness: Option<&str>,
    confidence: f32,
    committed_at: &str,
) -> Result<CommittedMemoryIds, ApiError> {
    for evidence_id in evidence_record_ids {
        ensure_evidence_exists(tx, evidence_id)?;
    }

    let (memory_item_id, old_active_version_id) =
        ensure_memory_item(tx, memory_type, subject_key, committed_at)?;
    let version_number = next_version_number(tx, &memory_item_id)?;
    let new_version_id = Uuid::new_v4().to_string();

    if let Some(old_version_id) = &old_active_version_id {
        tx.execute(
            "
            UPDATE memory_item_versions
            SET state = 'superseded', valid_to = ?2
            WHERE id = ?1
            ",
            params![old_version_id, committed_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to supersede version: {}", e)))?;
    }

    tx.execute(
        "
        INSERT INTO memory_item_versions (
          id, memory_item_id, version_number, state, value_json,
          supersedes_version_id, valid_from, valid_to, created_at
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, NULL, ?7)
        ",
        params![
            new_version_id,
            memory_item_id,
            version_number,
            serde_json::to_string(value_json)
                .map_err(|e| ApiError::internal(format!("failed value_json encode: {}", e)))?,
            old_active_version_id,
            committed_at,
            committed_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert memory version: {}", e)))?;

    for evidence_id in evidence_record_ids {
        tx.execute(
            "
            INSERT INTO memory_links (
              id, memory_item_version_id, evidence_record_id, link_type, created_at
            ) VALUES (?1, ?2, ?3, 'supporting_evidence', ?4)
            ",
            params![
                Uuid::new_v4().to_string(),
                new_version_id,
                evidence_id,
                committed_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert memory link: {}", e)))?;
    }

    tx.execute(
        "
        UPDATE memory_items
        SET active_version_id = ?2, status = 'active', updated_at = ?3
        WHERE id = ?1
        ",
        params![memory_item_id, new_version_id, committed_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to update memory item: {}", e)))?;

    upsert_memory_metadata(
        tx,
        &memory_item_id,
        scope,
        freshness,
        confidence,
        committed_at,
    )?;
    upsert_memory_fts_document(
        tx,
        &memory_item_id,
        subject_key,
        memory_type,
        value_json,
        scope,
    )?;
    let observation_evidence_ids = load_memory_evidence_ids(tx, &memory_item_id)?;
    upsert_observation_for_memory(
        tx,
        &memory_item_id,
        subject_key,
        memory_type,
        value_json,
        &observation_evidence_ids,
        scope,
        freshness,
        confidence,
        committed_at,
    )?;

    Ok(CommittedMemoryIds {
        memory_item_id,
        version_id: new_version_id,
        superseded_version_id: old_active_version_id,
    })
}

fn upsert_memory_metadata(
    tx: &Transaction<'_>,
    memory_item_id: &str,
    scope: Option<&MemoryScopePayload>,
    freshness: Option<&str>,
    confidence: f32,
    updated_at: &str,
) -> Result<(), ApiError> {
    let scope_kind = scope
        .and_then(|s| s.kind.as_deref())
        .map(normalize_scope_kind)
        .unwrap_or_else(|| "global".to_string());
    let freshness = freshness
        .map(normalize_freshness)
        .unwrap_or_else(|| "stable".to_string());
    let repo_path = scope.and_then(|s| trim_scope_field(s.repo_path.as_deref()));
    let repo_remote = scope.and_then(|s| trim_scope_field(s.repo_remote.as_deref()));
    let branch = scope.and_then(|s| trim_scope_field(s.branch.as_deref()));
    let workspace_path = scope.and_then(|s| trim_scope_field(s.workspace_path.as_deref()));

    tx.execute(
        "
        INSERT INTO memory_item_metadata (
          memory_item_id, scope_kind, repo_path, repo_remote, branch, workspace_path,
          sensitivity, freshness, confidence, decay_policy, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'normal', ?7, ?8, NULL, ?9, ?9)
        ON CONFLICT(memory_item_id) DO UPDATE SET
          scope_kind = excluded.scope_kind,
          repo_path = excluded.repo_path,
          repo_remote = excluded.repo_remote,
          branch = excluded.branch,
          workspace_path = excluded.workspace_path,
          freshness = excluded.freshness,
          confidence = excluded.confidence,
          updated_at = excluded.updated_at
        ",
        params![
            memory_item_id,
            scope_kind,
            repo_path,
            repo_remote,
            branch,
            workspace_path,
            freshness,
            confidence,
            updated_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to upsert memory metadata: {}", e)))?;
    Ok(())
}

fn upsert_memory_fts_document(
    tx: &Transaction<'_>,
    memory_item_id: &str,
    canonical_key: &str,
    memory_type: &str,
    value_json: &Value,
    scope: Option<&MemoryScopePayload>,
) -> Result<(), ApiError> {
    let scope_kind = scope
        .and_then(|s| s.kind.as_deref())
        .map(normalize_scope_kind)
        .unwrap_or_else(|| "global".to_string());
    let repo_path = scope.and_then(|s| trim_scope_field(s.repo_path.as_deref()));
    let repo_remote = scope.and_then(|s| trim_scope_field(s.repo_remote.as_deref()));
    let branch = scope.and_then(|s| trim_scope_field(s.branch.as_deref()));
    let body = format!(
        "{} {} {}",
        memory_type,
        canonical_key,
        serde_json::to_string(value_json)
            .map_err(|e| ApiError::internal(format!("failed to encode memory FTS body: {}", e)))?
    );

    upsert_retrieval_document(
        tx,
        "memory_item",
        memory_item_id,
        &scope_kind,
        repo_path.as_deref(),
        repo_remote.as_deref(),
        branch.as_deref(),
        canonical_key,
        &body,
    )
}

fn load_memory_evidence_ids(
    conn: &Connection,
    memory_item_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT DISTINCT ml.evidence_record_id
            FROM memory_item_versions mv
            JOIN memory_links ml ON ml.memory_item_version_id = mv.id
            WHERE mv.memory_item_id = ?1
            ORDER BY ml.evidence_record_id
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to prepare memory evidence query: {}", e))
        })?;

    let rows = stmt
        .query_map(params![memory_item_id], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to query memory evidence: {}", e)))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read memory evidence: {}", e)))
}

fn load_observation_memory_ids(
    conn: &Connection,
    observation_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT memory_item_id
            FROM observation_memory_links
            WHERE observation_id = ?1
            ORDER BY memory_item_id
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to prepare observation memory query: {}", e))
        })?;

    let rows = stmt
        .query_map(params![observation_id], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to query observation memory: {}", e)))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read observation memory: {}", e)))
}

fn load_observation_evidence_ids(
    conn: &Connection,
    observation_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT evidence_record_id
            FROM observation_evidence_links
            WHERE observation_id = ?1
            ORDER BY evidence_record_id
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to prepare observation evidence query: {}",
                e
            ))
        })?;

    let rows = stmt
        .query_map(params![observation_id], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to query observation evidence: {}", e)))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read observation evidence: {}", e)))
}

#[allow(clippy::too_many_arguments)]
fn upsert_observation_for_memory(
    tx: &Transaction<'_>,
    memory_item_id: &str,
    canonical_key: &str,
    memory_type: &str,
    value_json: &Value,
    evidence_record_ids: &[String],
    scope: Option<&MemoryScopePayload>,
    freshness: Option<&str>,
    confidence: f32,
    updated_at: &str,
) -> Result<(), ApiError> {
    let observation_key = observation_key(memory_type, canonical_key);
    let observation_id = tx
        .query_row(
            "SELECT id FROM observations WHERE canonical_key = ?1 LIMIT 1",
            params![&observation_key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup observation: {}", e)))?
        .unwrap_or_else(|| observation_id_for_canonical_key(&observation_key));
    let statement = statement_from_memory_value(canonical_key, value_json);
    let scope_kind = scope
        .and_then(|s| s.kind.as_deref())
        .map(normalize_scope_kind)
        .unwrap_or_else(|| "global".to_string());
    let freshness = freshness
        .map(normalize_freshness)
        .unwrap_or_else(|| "stable".to_string());
    let repo_path = scope.and_then(|s| trim_scope_field(s.repo_path.as_deref()));
    let repo_remote = scope.and_then(|s| trim_scope_field(s.repo_remote.as_deref()));
    let branch = scope.and_then(|s| trim_scope_field(s.branch.as_deref()));
    let workspace_path = scope.and_then(|s| trim_scope_field(s.workspace_path.as_deref()));
    let proof_count = evidence_record_ids.len() as i64;
    let prior = load_existing_observation(tx, &observation_key)?;
    let semantics = classify_observation_update(
        prior.as_ref(),
        &statement,
        proof_count,
        confidence,
        Some(freshness.as_str()),
    );
    let previous_json = prior.as_ref().map(existing_observation_json);

    let changed = tx
        .execute(
            "
            UPDATE observations
            SET observation_type = ?2,
                statement = ?3,
                scope_kind = ?4,
                repo_path = ?5,
                repo_remote = ?6,
                branch = ?7,
                workspace_path = ?8,
                proof_count = ?9,
                confidence = ?10,
                freshness = ?11,
                contradiction_count = ?12,
                last_verified_at = ?13,
                valid_to = NULL,
                status = 'active',
                updated_at = ?13
            WHERE canonical_key = ?1
            ",
            params![
                &observation_key,
                memory_type,
                &statement,
                scope_kind,
                repo_path,
                repo_remote,
                branch,
                workspace_path,
                proof_count,
                semantics.confidence,
                semantics.freshness,
                semantics.contradiction_count,
                updated_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to update observation: {}", e)))?;

    if changed == 0 {
        tx.execute(
            "
            INSERT INTO observations (
              id, canonical_key, observation_type, statement, scope_kind, repo_path,
              repo_remote, branch, workspace_path, proof_count, confidence, freshness,
              contradiction_count, last_verified_at, valid_from, valid_to, status,
              created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, NULL, 'active', ?14, ?14)
            ",
            params![
                &observation_id,
                &observation_key,
                memory_type,
                &statement,
                scope_kind,
                repo_path,
                repo_remote,
                branch,
                workspace_path,
                proof_count,
                semantics.confidence,
                semantics.freshness,
                semantics.contradiction_count,
                updated_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert observation: {}", e)))?;
    }

    tx.execute(
        "DELETE FROM observation_memory_links WHERE observation_id = ?1",
        params![&observation_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to clear observation memory links: {}", e)))?;
    tx.execute(
        "
        INSERT INTO observation_memory_links (
          id, observation_id, memory_item_id, link_type, created_at
        ) VALUES (?1, ?2, ?3, 'compiled_from_memory', ?4)
        ",
        params![
            format!("observation-memory-link-{}", observation_id),
            &observation_id,
            memory_item_id,
            updated_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to link observation memory: {}", e)))?;

    tx.execute(
        "DELETE FROM observation_evidence_links WHERE observation_id = ?1",
        params![&observation_id],
    )
    .map_err(|e| {
        ApiError::internal(format!("failed to clear observation evidence links: {}", e))
    })?;
    for evidence_id in evidence_record_ids {
        tx.execute(
            "
            INSERT INTO observation_evidence_links (
              id, observation_id, evidence_record_id, link_type, created_at
            ) VALUES (?1, ?2, ?3, 'supporting_evidence', ?4)
            ",
            params![
                format!(
                    "observation-evidence-link-{}-{}",
                    observation_id, evidence_id
                ),
                &observation_id,
                evidence_id,
                updated_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to link observation evidence: {}", e)))?;
    }

    let body = format!(
        "{} {} {}",
        memory_type,
        canonical_key,
        serde_json::to_string(value_json).map_err(|e| ApiError::internal(format!(
            "failed to encode observation FTS body: {}",
            e
        )))?
    );
    upsert_retrieval_document(
        tx,
        "observation",
        &observation_id,
        &scope_kind,
        repo_path.as_deref(),
        repo_remote.as_deref(),
        branch.as_deref(),
        canonical_key,
        &format!("{} {}", statement, body),
    )?;
    insert_observation_event(
        tx,
        &observation_id,
        &observation_key,
        &semantics.event_type,
        memory_item_id,
        previous_json,
        observation_event_json(
            &statement,
            proof_count,
            semantics.confidence,
            &semantics.freshness,
            semantics.contradiction_count,
        ),
        evidence_record_ids,
        updated_at,
    )?;

    Ok(())
}

#[derive(Debug, Clone)]
struct ExistingObservation {
    statement: String,
    proof_count: i64,
    confidence: f32,
    freshness: String,
    contradiction_count: i64,
}

#[derive(Debug, Clone)]
struct ObservationSemantics {
    event_type: String,
    freshness: String,
    confidence: f32,
    contradiction_count: i64,
}

fn load_existing_observation(
    tx: &Transaction<'_>,
    observation_key: &str,
) -> Result<Option<ExistingObservation>, ApiError> {
    tx.query_row(
        "
        SELECT statement, proof_count, confidence, freshness, contradiction_count
        FROM observations
        WHERE canonical_key = ?1
        LIMIT 1
        ",
        params![observation_key],
        |row| {
            Ok(ExistingObservation {
                statement: row.get(0)?,
                proof_count: row.get(1)?,
                confidence: row.get(2)?,
                freshness: row.get(3)?,
                contradiction_count: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| ApiError::internal(format!("failed to load existing observation: {}", e)))
}

fn classify_observation_update(
    prior: Option<&ExistingObservation>,
    next_statement: &str,
    next_proof_count: i64,
    next_confidence: f32,
    requested_freshness: Option<&str>,
) -> ObservationSemantics {
    let requested = requested_freshness
        .map(normalize_freshness)
        .unwrap_or_else(|| "stable".to_string());
    let Some(prior) = prior else {
        return ObservationSemantics {
            event_type: "created".to_string(),
            freshness: requested,
            confidence: next_confidence,
            contradiction_count: 0,
        };
    };

    let similarity = statement_similarity(&prior.statement, next_statement);
    if similarity < 0.25 {
        return ObservationSemantics {
            event_type: "contradicted".to_string(),
            freshness: "weakening".to_string(),
            confidence: next_confidence.min(prior.confidence),
            contradiction_count: prior.contradiction_count + 1,
        };
    }
    if requested == "stale" || requested == "weakening" || next_confidence < prior.confidence {
        return ObservationSemantics {
            event_type: "weakened".to_string(),
            freshness: "weakening".to_string(),
            confidence: next_confidence.min(prior.confidence),
            contradiction_count: prior.contradiction_count,
        };
    }
    if similarity >= 0.45 || next_proof_count > prior.proof_count {
        return ObservationSemantics {
            event_type: "strengthened".to_string(),
            freshness: "strengthening".to_string(),
            confidence: next_confidence.max(prior.confidence),
            contradiction_count: prior.contradiction_count,
        };
    }

    ObservationSemantics {
        event_type: "updated".to_string(),
        freshness: requested,
        confidence: next_confidence,
        contradiction_count: prior.contradiction_count,
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_observation_event(
    tx: &Transaction<'_>,
    observation_id: &str,
    canonical_key: &str,
    event_type: &str,
    memory_item_id: &str,
    previous_json: Option<Value>,
    current_json: Value,
    evidence_record_ids: &[String],
    created_at: &str,
) -> Result<(), ApiError> {
    tx.execute(
        "
        INSERT INTO observation_events (
          id, observation_id, canonical_key, event_type, memory_item_id,
          previous_json, current_json, evidence_record_ids_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ",
        params![
            Uuid::new_v4().to_string(),
            observation_id,
            canonical_key,
            event_type,
            memory_item_id,
            previous_json.map(|value| value.to_string()),
            current_json.to_string(),
            serde_json::to_string(evidence_record_ids).map_err(|e| {
                ApiError::internal(format!(
                    "failed to encode observation event evidence: {}",
                    e
                ))
            })?,
            created_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert observation event: {}", e)))?;
    Ok(())
}

fn existing_observation_json(observation: &ExistingObservation) -> Value {
    observation_event_json(
        &observation.statement,
        observation.proof_count,
        observation.confidence,
        &observation.freshness,
        observation.contradiction_count,
    )
}

fn observation_event_json(
    statement: &str,
    proof_count: i64,
    confidence: f32,
    freshness: &str,
    contradiction_count: i64,
) -> Value {
    json!({
        "statement": statement,
        "proof_count": proof_count,
        "confidence": confidence,
        "freshness": freshness,
        "contradiction_count": contradiction_count,
    })
}

fn statement_similarity(left: &str, right: &str) -> f32 {
    let left_terms = observation_terms(left);
    let right_terms = observation_terms(right);
    if left_terms.is_empty() || right_terms.is_empty() {
        return 0.0;
    }
    let intersection = left_terms.intersection(&right_terms).count() as f32;
    let union = left_terms.union(&right_terms).count() as f32;
    intersection / union
}

fn observation_terms(value: &str) -> BTreeSet<String> {
    value
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .map(|term| term.trim().to_lowercase())
        .filter(|term| term.len() > 2)
        .filter(|term| !observation_stopword(term))
        .collect()
}

fn observation_stopword(term: &str) -> bool {
    matches!(
        term,
        "and" | "are" | "for" | "from" | "the" | "this" | "that" | "uses" | "with"
    )
}

fn observation_id_for_canonical_key(canonical_key: &str) -> String {
    format!("observation-{}", stable_key_fragment(canonical_key))
}

fn observation_key(memory_type: &str, canonical_key: &str) -> String {
    format!("{}:{}", normalize_token(memory_type), canonical_key.trim())
}

fn stable_key_fragment(value: &str) -> String {
    let fragment = value
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if fragment.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        fragment
    }
}

fn checksum_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hex::encode(hasher.finalize())
}

fn statement_from_memory_value(canonical_key: &str, value_json: &Value) -> String {
    value_json
        .get("statement")
        .and_then(Value::as_str)
        .or_else(|| value_json.get("value").and_then(Value::as_str))
        .or_else(|| value_json.get("decision").and_then(Value::as_str))
        .or_else(|| value_json.get("preference").and_then(Value::as_str))
        .or_else(|| value_json.get("convention").and_then(Value::as_str))
        .or_else(|| value_json.get("open_question").and_then(Value::as_str))
        .or_else(|| value_json.get("error").and_then(Value::as_str))
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{} {}", canonical_key, value_json))
}

fn upsert_graph_fts_document(
    tx: &Transaction<'_>,
    relationship_id: &str,
    canonical_key: &str,
    subject: &GraphEntityRef,
    predicate: &str,
    object: &GraphEntityRef,
    attributes_json: &Value,
) -> Result<(), ApiError> {
    let body = format!(
        "{} {} {} {} {} {}",
        subject.entity_type,
        subject.canonical_name,
        predicate,
        object.entity_type,
        object.canonical_name,
        serde_json::to_string(attributes_json)
            .map_err(|e| ApiError::internal(format!("failed to encode graph FTS body: {}", e)))?
    );
    upsert_retrieval_document(
        tx,
        "graph_relationship",
        relationship_id,
        "global",
        None,
        None,
        None,
        canonical_key,
        &body,
    )
}

#[allow(clippy::too_many_arguments)]
fn upsert_retrieval_document(
    tx: &Transaction<'_>,
    source_type: &str,
    source_id: &str,
    scope_kind: &str,
    repo_path: Option<&str>,
    repo_remote: Option<&str>,
    branch: Option<&str>,
    title: &str,
    body: &str,
) -> Result<(), ApiError> {
    tx.execute(
        "
        DELETE FROM retrieval_documents_fts
        WHERE source_type = ?1 AND source_id = ?2
        ",
        params![source_type, source_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to clear retrieval FTS document: {}", e)))?;
    tx.execute(
        "
        INSERT INTO retrieval_documents_fts (
          source_type, source_id, scope_kind, repo_path, repo_remote, branch, title, body
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            source_type,
            source_id,
            scope_kind,
            repo_path,
            repo_remote,
            branch,
            title,
            body,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to index retrieval FTS document: {}", e)))?;
    Ok(())
}

fn normalize_scope_kind(kind: &str) -> String {
    match kind.trim().to_lowercase().as_str() {
        "repo" => "repo",
        "workspace" => "workspace",
        "agent" => "agent",
        "source" => "source",
        _ => "global",
    }
    .to_string()
}

fn normalize_freshness(freshness: &str) -> String {
    match freshness.trim().to_lowercase().as_str() {
        "new" => "new",
        "strengthening" => "strengthening",
        "weakening" => "weakening",
        "stale" => "stale",
        _ => "stable",
    }
    .to_string()
}

fn trim_scope_field(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

async fn reject_proposal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RejectProposalResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM memory_proposals WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed proposal lookup: {}", e)))?;

    let status = status.ok_or_else(|| ApiError::not_found("proposal not found"))?;
    if status != "pending" {
        return Err(ApiError::conflict(format!(
            "proposal has status '{}' and cannot be rejected",
            status
        )));
    }

    let resolved_at = Utc::now().to_rfc3339();
    conn.execute(
        "
        UPDATE memory_proposals
        SET status = 'rejected', resolved_at = ?2
        WHERE id = ?1
        ",
        params![id, resolved_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to reject proposal: {}", e)))?;

    Ok(Json(RejectProposalResponse {
        proposal_id: id,
        status: "rejected".to_string(),
        resolved_at,
    }))
}

async fn create_graph_relationship_proposal(
    State(state): State<AppState>,
    Json(payload): Json<CreateGraphRelationshipProposalRequest>,
) -> Result<(StatusCode, Json<CreateGraphRelationshipProposalResponse>), ApiError> {
    payload.validate()?;

    let created_at = Utc::now().to_rfc3339();
    let proposal_id = Uuid::new_v4().to_string();
    let conn = open_db(&state.db_path)?;
    let canonical_subject_name = canonicalize_graph_entity_name_for_key(
        &conn,
        &payload.subject.entity_type,
        &payload.subject.canonical_name,
    )?;
    let canonical_predicate = canonicalize_graph_predicate(&conn, &payload.predicate)?;
    let canonical_object_name = canonicalize_graph_entity_name_for_key(
        &conn,
        &payload.object.entity_type,
        &payload.object.canonical_name,
    )?;
    let subject_key = format!(
        "{}|{}|{}",
        canonical_subject_name, canonical_predicate, canonical_object_name
    );

    let proposal_payload = GraphRelationshipProposalPayload {
        subject: payload.subject,
        predicate: payload.predicate,
        object: payload.object,
        attributes_json: payload.attributes_json,
        evidence_record_ids: payload.evidence_record_ids,
    };

    let payload_json = serde_json::to_string(&proposal_payload).map_err(|e| {
        ApiError::internal(format!("failed to encode graph proposal payload: {}", e))
    })?;

    conn.execute(
        "
        INSERT INTO memory_proposals (
          id, proposal_type, subject_key, payload_json, confidence, status, created_at, resolved_at
        ) VALUES (?1, 'graph_relationship', ?2, ?3, ?4, 'pending', ?5, NULL)
        ",
        params![
            &proposal_id,
            &subject_key,
            &payload_json,
            payload.confidence,
            &created_at
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert graph proposal: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateGraphRelationshipProposalResponse {
            proposal_id,
            status: "pending".to_string(),
            created_at,
        }),
    ))
}

async fn upsert_graph_entity_alias(
    State(state): State<AppState>,
    Json(payload): Json<UpsertGraphEntityAliasRequest>,
) -> Result<Json<UpsertGraphEntityAliasResponse>, ApiError> {
    payload.validate()?;

    let updated_at = Utc::now().to_rfc3339();
    let entity_type = normalize_token(&payload.entity_type);
    let alias_name = normalize_token(&payload.alias_name);
    let canonical_name = normalize_token(&payload.canonical_name);

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    let canonical_entity_id =
        ensure_graph_entity_without_alias(&tx, &entity_type, &canonical_name, &updated_at)?;

    let existing_id: Option<String> = tx
        .query_row(
            "
            SELECT id
            FROM graph_entity_aliases
            WHERE entity_type = ?1 AND alias_name = ?2
            LIMIT 1
            ",
            params![&entity_type, &alias_name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup entity alias: {}", e)))?;

    if let Some(existing_id) = existing_id {
        tx.execute(
            "
            UPDATE graph_entity_aliases
            SET canonical_entity_id = ?2, updated_at = ?3
            WHERE id = ?1
            ",
            params![&existing_id, &canonical_entity_id, &updated_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to update entity alias: {}", e)))?;
    } else {
        tx.execute(
            "
            INSERT INTO graph_entity_aliases (
              id, entity_type, alias_name, canonical_entity_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ",
            params![
                Uuid::new_v4().to_string(),
                &entity_type,
                &alias_name,
                &canonical_entity_id,
                &updated_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert entity alias: {}", e)))?;
    }

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(UpsertGraphEntityAliasResponse {
        entity_type,
        alias_name,
        canonical_entity_id,
        canonical_name,
        updated_at,
    }))
}

async fn upsert_graph_predicate_alias(
    State(state): State<AppState>,
    Json(payload): Json<UpsertGraphPredicateAliasRequest>,
) -> Result<Json<UpsertGraphPredicateAliasResponse>, ApiError> {
    payload.validate()?;

    let updated_at = Utc::now().to_rfc3339();
    let alias_predicate = normalize_token(&payload.alias_predicate);
    let canonical_predicate = normalize_token(&payload.canonical_predicate);
    let conn = open_db(&state.db_path)?;

    let existing_id: Option<String> = conn
        .query_row(
            "
            SELECT id
            FROM graph_predicate_aliases
            WHERE alias_predicate = ?1
            LIMIT 1
            ",
            params![&alias_predicate],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup predicate alias: {}", e)))?;

    if let Some(existing_id) = existing_id {
        conn.execute(
            "
            UPDATE graph_predicate_aliases
            SET canonical_predicate = ?2, updated_at = ?3
            WHERE id = ?1
            ",
            params![&existing_id, &canonical_predicate, &updated_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to update predicate alias: {}", e)))?;
    } else {
        conn.execute(
            "
            INSERT INTO graph_predicate_aliases (
              id, alias_predicate, canonical_predicate, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?4)
            ",
            params![
                Uuid::new_v4().to_string(),
                &alias_predicate,
                &canonical_predicate,
                &updated_at,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert predicate alias: {}", e)))?;
    }

    Ok(Json(UpsertGraphPredicateAliasResponse {
        alias_predicate,
        canonical_predicate,
        updated_at,
    }))
}

async fn run_graph_compaction(
    State(state): State<AppState>,
    Json(payload): Json<RunGraphCompactionRequest>,
) -> Result<Json<RunGraphCompactionResponse>, ApiError> {
    let run_at = Utc::now().to_rfc3339();
    let job_id = Uuid::new_v4().to_string();
    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    tx.execute(
        "
        INSERT INTO graph_compaction_jobs (
          id, dry_run, status, summary_json, created_at, completed_at
        ) VALUES (?1, ?2, 'running', '{}', ?3, NULL)
        ",
        params![&job_id, if payload.dry_run { 1 } else { 0 }, &run_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert compaction job: {}", e)))?;

    let entity_alias_rules = count_graph_entity_aliases(&tx)?;
    let predicate_alias_rules = count_graph_predicate_aliases(&tx)?;
    let counts = compact_graph_relationships(&tx, payload.dry_run, &run_at)?;

    let summary_json = json!({
        "entity_alias_rules": entity_alias_rules,
        "predicate_alias_rules": predicate_alias_rules,
        "canonicalized_relationships": counts.canonicalized_relationships,
        "redirected_relationships": counts.redirected_relationships,
        "merged_versions_created": counts.merged_versions_created,
        "compacted_entities": counts.compacted_entities,
    });

    tx.execute(
        "
        UPDATE graph_compaction_jobs
        SET status = 'completed', summary_json = ?2, completed_at = ?3
        WHERE id = ?1
        ",
        params![
            &job_id,
            serde_json::to_string(&summary_json).map_err(|e| ApiError::internal(format!(
                "failed to encode compaction summary: {}",
                e
            )))?,
            &run_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to update compaction job: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(RunGraphCompactionResponse {
        job_id,
        dry_run: payload.dry_run,
        status: "completed".to_string(),
        run_at,
        entity_alias_rules,
        predicate_alias_rules,
        canonicalized_relationships: counts.canonicalized_relationships,
        redirected_relationships: counts.redirected_relationships,
        merged_versions_created: counts.merged_versions_created,
        compacted_entities: counts.compacted_entities,
    }))
}

async fn commit_graph_relationship_proposal(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CommitGraphProposalResponse>, ApiError> {
    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    let proposal = load_proposal(&tx, &id)?;
    if proposal.proposal_type != "graph_relationship" {
        return Err(ApiError::bad_request(format!(
            "proposal {} is type '{}' not graph_relationship",
            id, proposal.proposal_type
        )));
    }
    if proposal.status != "pending" {
        return Err(ApiError::conflict(format!(
            "proposal {} has status '{}' and cannot be committed",
            id, proposal.status
        )));
    }

    let payload: GraphRelationshipProposalPayload = serde_json::from_str(&proposal.payload_json)
        .map_err(|e| ApiError::internal(format!("failed to decode graph payload: {}", e)))?;

    for evidence_id in &payload.evidence_record_ids {
        ensure_evidence_exists(&tx, evidence_id)?;
    }

    let committed_at = Utc::now().to_rfc3339();
    let subject_entity_id = ensure_graph_entity(&tx, &payload.subject, &committed_at)?;
    let object_entity_id = ensure_graph_entity(&tx, &payload.object, &committed_at)?;
    let canonical_predicate = canonicalize_graph_predicate(&tx, &payload.predicate)?;
    let canonical_key = build_graph_relationship_key(
        &tx,
        &subject_entity_id,
        &canonical_predicate,
        &object_entity_id,
    )?;

    let (relationship_id, old_active_version_id) = ensure_graph_relationship(
        &tx,
        &canonical_key,
        &subject_entity_id,
        &canonical_predicate,
        &object_entity_id,
        &committed_at,
    )?;

    let new_version_id = Uuid::new_v4().to_string();
    let version_number = next_graph_relationship_version_number(&tx, &relationship_id)?;

    if let Some(old_version_id) = &old_active_version_id {
        tx.execute(
            "
            UPDATE graph_relationship_versions
            SET state = 'superseded', valid_to = ?2
            WHERE id = ?1
            ",
            params![old_version_id, &committed_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to supersede graph version: {}", e)))?;
    }

    tx.execute(
        "
        INSERT INTO graph_relationship_versions (
          id, relationship_id, version_number, state, confidence, attributes_json,
          supersedes_version_id, valid_from, valid_to, created_at
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, NULL, ?7)
        ",
        params![
            &new_version_id,
            &relationship_id,
            version_number,
            proposal.confidence,
            serde_json::to_string(&payload.attributes_json).map_err(|e| ApiError::internal(
                format!("failed to encode attributes_json: {}", e)
            ))?,
            &old_active_version_id,
            &committed_at,
        ],
    )
    .map_err(|e| {
        ApiError::internal(format!(
            "failed to insert graph relationship version: {}",
            e
        ))
    })?;

    for evidence_id in &payload.evidence_record_ids {
        tx.execute(
            "
            INSERT INTO graph_relationship_evidence_links (
              id, relationship_version_id, evidence_record_id, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            ",
            params![
                Uuid::new_v4().to_string(),
                &new_version_id,
                evidence_id,
                &committed_at,
            ],
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to insert relationship evidence link: {}",
                e
            ))
        })?;
    }

    tx.execute(
        "
        UPDATE graph_relationships
        SET active_version_id = ?2, status = 'active', updated_at = ?3
        WHERE id = ?1
        ",
        params![&relationship_id, &new_version_id, &committed_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to update graph relationship: {}", e)))?;

    upsert_graph_fts_document(
        &tx,
        &relationship_id,
        &canonical_key,
        &payload.subject,
        &canonical_predicate,
        &payload.object,
        &payload.attributes_json,
    )?;

    tx.execute(
        "
        UPDATE memory_proposals
        SET status = 'committed', resolved_at = ?2
        WHERE id = ?1
        ",
        params![&proposal.id, &committed_at],
    )
    .map_err(|e| ApiError::internal(format!("failed to update graph proposal status: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(CommitGraphProposalResponse {
        proposal_id: id,
        relationship_id,
        version_id: new_version_id,
        superseded_version_id: old_active_version_id,
        committed_at,
    }))
}

async fn get_graph_relationship(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GraphRelationshipViewResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;

    let row: Option<GraphRelationshipViewRow> = conn
        .query_row(
            "
            SELECT
              gr.id,
              se.entity_type,
              se.canonical_name,
              gr.predicate,
              oe.entity_type,
              oe.canonical_name,
              grv.id,
              grv.confidence,
              grv.attributes_json,
              grv.created_at
            FROM graph_relationships gr
            JOIN graph_entities se ON se.id = gr.subject_entity_id
            JOIN graph_entities oe ON oe.id = gr.object_entity_id
            JOIN graph_relationship_versions grv ON grv.id = gr.active_version_id
            WHERE gr.id = ?1
            LIMIT 1
            ",
            params![&id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup graph relationship: {}", e)))?;

    let (
        relationship_id,
        subject_entity_type,
        subject_canonical_name,
        predicate,
        object_entity_type,
        object_canonical_name,
        active_version_id,
        confidence,
        attributes_raw,
        _created_at,
    ) = row.ok_or_else(|| ApiError::not_found("graph relationship not found"))?;

    let attributes_json: Value = serde_json::from_str(&attributes_raw)
        .map_err(|e| ApiError::internal(format!("failed to decode attributes_json: {}", e)))?;
    let evidence_record_ids =
        load_evidence_for_graph_relationship_version(&conn, &active_version_id)?;

    Ok(Json(GraphRelationshipViewResponse {
        relationship_id,
        subject: GraphEntityRef {
            entity_type: subject_entity_type,
            canonical_name: subject_canonical_name,
        },
        predicate,
        object: GraphEntityRef {
            entity_type: object_entity_type,
            canonical_name: object_canonical_name,
        },
        active_version_id,
        confidence,
        attributes_json,
        evidence_record_ids,
    }))
}

async fn get_graph_relationship_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GraphRelationshipHistoryResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;

    let relationship_row: Option<(String, String, String, String, String, String)> = conn
        .query_row(
            "
            SELECT gr.id, se.entity_type, se.canonical_name, gr.predicate, oe.entity_type, oe.canonical_name
            FROM graph_relationships gr
            JOIN graph_entities se ON se.id = gr.subject_entity_id
            JOIN graph_entities oe ON oe.id = gr.object_entity_id
            WHERE gr.id = ?1
            LIMIT 1
            ",
            params![&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup graph relationship: {}", e)))?;

    let (
        relationship_id,
        subject_entity_type,
        subject_canonical_name,
        predicate,
        object_entity_type,
        object_canonical_name,
    ) = relationship_row.ok_or_else(|| ApiError::not_found("graph relationship not found"))?;

    let mut stmt = conn
        .prepare(
            "
            SELECT id, version_number, state, confidence, attributes_json, supersedes_version_id, valid_from, valid_to, created_at
            FROM graph_relationship_versions
            WHERE relationship_id = ?1
            ORDER BY version_number DESC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare graph history query: {}", e)))?;

    let rows = stmt
        .query_map(params![&relationship_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f32>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| ApiError::internal(format!("failed to execute graph history query: {}", e)))?;

    let mut versions = Vec::new();
    for row in rows {
        let (
            version_id,
            version_number,
            state,
            confidence,
            attributes_raw,
            supersedes_version_id,
            valid_from,
            valid_to,
            created_at,
        ) = row
            .map_err(|e| ApiError::internal(format!("failed to read graph history row: {}", e)))?;

        let attributes_json: Value = serde_json::from_str(&attributes_raw)
            .map_err(|e| ApiError::internal(format!("failed to decode attributes_json: {}", e)))?;
        let evidence_record_ids = load_evidence_for_graph_relationship_version(&conn, &version_id)?;

        versions.push(GraphRelationshipVersionEntry {
            version_id,
            version_number,
            state,
            confidence,
            attributes_json,
            supersedes_version_id,
            valid_from,
            valid_to,
            created_at,
            evidence_record_ids,
        });
    }

    Ok(Json(GraphRelationshipHistoryResponse {
        relationship_id,
        subject: GraphEntityRef {
            entity_type: subject_entity_type,
            canonical_name: subject_canonical_name,
        },
        predicate,
        object: GraphEntityRef {
            entity_type: object_entity_type,
            canonical_name: object_canonical_name,
        },
        versions,
    }))
}

async fn get_memory(
    State(state): State<AppState>,
    Path(canonical_key): Path<String>,
) -> Result<Json<MemoryViewResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;

    let row: Option<(String, String, String, String, String)> = conn
        .query_row(
            "
            SELECT mi.id, mi.canonical_key, mi.memory_type, mv.id, mv.value_json
            FROM memory_items mi
            JOIN memory_item_versions mv ON mv.id = mi.active_version_id
            WHERE mi.canonical_key = ?1
            LIMIT 1
            ",
            params![&canonical_key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup memory item: {}", e)))?;

    let (memory_item_id, canonical_key, memory_type, active_version_id, raw_value) =
        row.ok_or_else(|| ApiError::not_found("memory item not found"))?;

    let value_json: Value = serde_json::from_str(&raw_value)
        .map_err(|e| ApiError::internal(format!("failed to decode value_json: {}", e)))?;

    let evidence_record_ids = load_evidence_for_version(&conn, &active_version_id)?;

    Ok(Json(MemoryViewResponse {
        memory_item_id,
        canonical_key,
        memory_type,
        active_version_id,
        value_json,
        evidence_record_ids,
    }))
}

async fn get_memory_history(
    State(state): State<AppState>,
    Path(canonical_key): Path<String>,
) -> Result<Json<MemoryHistoryResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;

    let item_row: Option<(String, String)> = conn
        .query_row(
            "
            SELECT id, memory_type
            FROM memory_items
            WHERE canonical_key = ?1
            LIMIT 1
            ",
            params![&canonical_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup memory item: {}", e)))?;

    let (memory_item_id, memory_type) =
        item_row.ok_or_else(|| ApiError::not_found("memory item not found"))?;

    let mut stmt = conn
        .prepare(
            "
            SELECT id, version_number, state, value_json, supersedes_version_id, valid_from, valid_to, created_at
            FROM memory_item_versions
            WHERE memory_item_id = ?1
            ORDER BY version_number DESC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare history query: {}", e)))?;

    let rows = stmt
        .query_map(params![&memory_item_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(|e| ApiError::internal(format!("failed to execute history query: {}", e)))?;

    let mut versions = Vec::new();
    for row in rows {
        let (
            version_id,
            version_number,
            state,
            raw_value,
            supersedes_version_id,
            valid_from,
            valid_to,
            created_at,
        ) = row.map_err(|e| ApiError::internal(format!("failed to read history row: {}", e)))?;

        let value_json: Value = serde_json::from_str(&raw_value)
            .map_err(|e| ApiError::internal(format!("failed to decode value_json: {}", e)))?;
        let evidence_record_ids = load_evidence_for_version(&conn, &version_id)?;

        versions.push(MemoryVersionHistoryEntry {
            version_id,
            version_number,
            state,
            value_json,
            supersedes_version_id,
            valid_from,
            valid_to,
            created_at,
            evidence_record_ids,
        });
    }

    Ok(Json(MemoryHistoryResponse {
        memory_item_id,
        canonical_key,
        memory_type,
        versions,
    }))
}

async fn get_observation(
    State(state): State<AppState>,
    Path(canonical_key): Path<String>,
) -> Result<Json<ObservationViewResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;
    let row: Option<ObservationViewRow> = conn
        .query_row(
            "
            SELECT id, canonical_key, observation_type, statement, scope_kind,
              repo_path, repo_remote, branch, workspace_path, proof_count,
              confidence, freshness, contradiction_count, last_verified_at,
              valid_from, valid_to, status, updated_at
            FROM observations
            WHERE canonical_key = ?1
            LIMIT 1
            ",
            params![&canonical_key],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                    row.get(16)?,
                    row.get(17)?,
                ))
            },
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup observation: {}", e)))?;

    let (
        observation_id,
        canonical_key,
        observation_type,
        statement,
        scope_kind,
        repo_path,
        repo_remote,
        branch,
        workspace_path,
        proof_count,
        confidence,
        freshness,
        contradiction_count,
        last_verified_at,
        valid_from,
        valid_to,
        status,
        updated_at,
    ) = row.ok_or_else(|| ApiError::not_found("observation not found"))?;

    Ok(Json(ObservationViewResponse {
        memory_item_ids: load_observation_memory_ids(&conn, &observation_id)?,
        evidence_record_ids: load_observation_evidence_ids(&conn, &observation_id)?,
        observation_id,
        canonical_key,
        observation_type,
        statement,
        scope_kind,
        repo_path,
        repo_remote,
        branch,
        workspace_path,
        proof_count,
        confidence,
        freshness,
        contradiction_count,
        last_verified_at,
        valid_from,
        valid_to,
        status,
        updated_at,
    }))
}

async fn get_observation_history(
    State(state): State<AppState>,
    Path(canonical_key): Path<String>,
) -> Result<Json<ObservationHistoryResponse>, ApiError> {
    let conn = open_db(&state.db_path)?;
    let observation_id: String = conn
        .query_row(
            "SELECT id FROM observations WHERE canonical_key = ?1 LIMIT 1",
            params![&canonical_key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup observation: {}", e)))?
        .ok_or_else(|| ApiError::not_found("observation not found"))?;

    let mut stmt = conn
        .prepare(
            "
            SELECT id, event_type, memory_item_id, previous_json, current_json,
              evidence_record_ids_json, created_at
            FROM observation_events
            WHERE observation_id = ?1
            ORDER BY created_at DESC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare observation history: {}", e)))?;

    let rows = stmt
        .query_map(params![&observation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| ApiError::internal(format!("failed to query observation history: {}", e)))?;

    let mut events = Vec::new();
    for row in rows {
        let (
            event_id,
            event_type,
            memory_item_id,
            previous_raw,
            current_raw,
            evidence_raw,
            created_at,
        ) = row
            .map_err(|e| ApiError::internal(format!("failed to read observation event: {}", e)))?;
        events.push(ObservationEventEntry {
            event_id,
            event_type,
            memory_item_id,
            previous_json: previous_raw
                .map(|raw| serde_json::from_str(&raw))
                .transpose()
                .map_err(|e| {
                    ApiError::internal(format!("failed to decode observation previous_json: {}", e))
                })?,
            current_json: serde_json::from_str(&current_raw).map_err(|e| {
                ApiError::internal(format!("failed to decode observation current_json: {}", e))
            })?,
            evidence_record_ids: serde_json::from_str(&evidence_raw).map_err(|e| {
                ApiError::internal(format!("failed to decode observation evidence ids: {}", e))
            })?,
            created_at,
        });
    }

    Ok(Json(ObservationHistoryResponse {
        observation_id,
        canonical_key,
        events,
    }))
}

async fn forget_memory(
    State(state): State<AppState>,
    Json(payload): Json<ForgetMemoryRequest>,
) -> Result<Json<ForgetMemoryResponse>, ApiError> {
    if payload.canonical_key.trim().is_empty() {
        return Err(ApiError::bad_request("canonical_key is required"));
    }

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    let memory_item_id: Option<String> = tx
        .query_row(
            "
            SELECT id
            FROM memory_items
            WHERE canonical_key = ?1
            LIMIT 1
            ",
            params![&payload.canonical_key],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup memory item: {}", e)))?;

    let memory_item_id =
        memory_item_id.ok_or_else(|| ApiError::not_found("memory item not found"))?;
    let counts = delete_memory_item_by_id_tx(&tx, &memory_item_id, payload.forget_evidence)?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(ForgetMemoryResponse {
        canonical_key: payload.canonical_key,
        deleted_memory_item_id: memory_item_id,
        deleted_versions: counts.deleted_versions,
        deleted_links: counts.deleted_links,
        deleted_evidence: counts.deleted_evidence,
    }))
}

async fn upsert_retention_policy(
    State(state): State<AppState>,
    Json(payload): Json<UpsertRetentionPolicyRequest>,
) -> Result<Json<UpsertRetentionPolicyResponse>, ApiError> {
    payload.validate()?;

    let policy = RetentionPolicyPayload {
        policy_name: payload.policy_name.trim().to_string(),
        memory_type: trim_optional(payload.memory_type),
        canonical_prefix: trim_optional(payload.canonical_prefix),
        max_age_days: payload.max_age_days,
        forget_evidence: payload.forget_evidence,
    };

    let now = Utc::now().to_rfc3339();
    let rule_name = retention_rule_name(&policy.policy_name);
    let rule_json = serde_json::to_string(&policy)
        .map_err(|e| ApiError::internal(format!("failed to encode retention policy: {}", e)))?;

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    tx.execute(
        "DELETE FROM policy_rules WHERE rule_name = ?1",
        params![&rule_name],
    )
    .map_err(|e| ApiError::internal(format!("failed to delete existing policy: {}", e)))?;

    tx.execute(
        "
        INSERT INTO policy_rules (id, rule_name, rule_json, enabled, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ",
        params![
            Uuid::new_v4().to_string(),
            &rule_name,
            &rule_json,
            if payload.enabled { 1 } else { 0 },
            &now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to upsert retention policy: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(UpsertRetentionPolicyResponse {
        policy_name: policy.policy_name,
        memory_type: policy.memory_type,
        canonical_prefix: policy.canonical_prefix,
        max_age_days: policy.max_age_days,
        forget_evidence: policy.forget_evidence,
        enabled: payload.enabled,
        updated_at: now,
    }))
}

async fn run_retention_jobs(
    State(state): State<AppState>,
    Json(payload): Json<RunRetentionRequest>,
) -> Result<Json<RunRetentionResponse>, ApiError> {
    let run_at = Utc::now().to_rfc3339();
    let run_at_dt = DateTime::parse_from_rfc3339(&run_at)
        .map_err(|e| ApiError::internal(format!("failed to parse run time: {}", e)))?
        .with_timezone(&Utc);

    let conn = open_db(&state.db_path)?;
    let policies = load_retention_policies(&conn, &payload.policy_names)?;
    drop(conn);

    let mut results = Vec::new();
    for policy in policies {
        let mut conn = open_db(&state.db_path)?;
        let tx = conn
            .transaction()
            .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

        let job_id = Uuid::new_v4().to_string();
        tx.execute(
            "
            INSERT INTO retention_jobs (id, policy_name, target_type, status, run_at, completed_at)
            VALUES (?1, ?2, 'memory_item', 'running', ?3, NULL)
            ",
            params![&job_id, &policy.policy_name, &run_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert retention job: {}", e)))?;

        let (matched, deleted_items, counts) =
            apply_retention_policy(&tx, &policy, run_at_dt, payload.dry_run)?;

        let status = if payload.dry_run {
            "dry_run".to_string()
        } else {
            "completed".to_string()
        };

        tx.execute(
            "
            UPDATE retention_jobs
            SET status = ?2, completed_at = ?3
            WHERE id = ?1
            ",
            params![&job_id, &status, &run_at],
        )
        .map_err(|e| ApiError::internal(format!("failed to update retention job: {}", e)))?;

        tx.commit()
            .map_err(|e| ApiError::internal(format!("failed to commit retention job: {}", e)))?;

        results.push(RetentionRunPolicyResult {
            policy_name: policy.policy_name,
            job_id,
            matched_memory_items: matched,
            deleted_memory_items: deleted_items,
            deleted_versions: counts.deleted_versions,
            deleted_links: counts.deleted_links,
            deleted_evidence: counts.deleted_evidence,
            status,
        });
    }

    Ok(Json(RunRetentionResponse {
        run_at,
        dry_run: payload.dry_run,
        policies: results,
    }))
}

fn open_db(db_path: &str) -> Result<Connection, ApiError> {
    let conn = Connection::open(db_path)
        .map_err(|e| ApiError::internal(format!("failed to open db: {}", e)))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| ApiError::internal(format!("failed to enable foreign keys: {}", e)))?;
    Ok(conn)
}

fn load_proposal(conn: &Connection, id: &str) -> Result<ProposalRow, ApiError> {
    conn.query_row(
        "
        SELECT id, proposal_type, subject_key, payload_json, confidence, status
        FROM memory_proposals
        WHERE id = ?1
        ",
        params![id],
        |row| {
            Ok(ProposalRow {
                id: row.get(0)?,
                proposal_type: row.get(1)?,
                subject_key: row.get(2)?,
                payload_json: row.get(3)?,
                confidence: row.get(4)?,
                status: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(|e| ApiError::internal(format!("failed to query proposal: {}", e)))?
    .ok_or_else(|| ApiError::not_found("proposal not found"))
}

fn ensure_evidence_exists(conn: &Connection, evidence_id: &str) -> Result<(), ApiError> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM evidence_records WHERE id = ?1",
            params![evidence_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup evidence: {}", e)))?;

    if exists.is_none() {
        return Err(ApiError::bad_request(format!(
            "evidence_record_id not found: {}",
            evidence_id
        )));
    }

    Ok(())
}

fn ensure_memory_item(
    conn: &Connection,
    memory_type: &str,
    subject_key: &str,
    now: &str,
) -> Result<(String, Option<String>), ApiError> {
    let found: Option<(String, Option<String>)> = conn
        .query_row(
            "
            SELECT id, active_version_id
            FROM memory_items
            WHERE canonical_key = ?1
            LIMIT 1
            ",
            params![subject_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to query memory item: {}", e)))?;

    if let Some(pair) = found {
        return Ok(pair);
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO memory_items (
          id, memory_type, canonical_key, active_version_id, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, NULL, 'active', ?4, ?4)
        ",
        params![id, memory_type, subject_key, now],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert memory item: {}", e)))?;

    Ok((id, None))
}

fn next_version_number(conn: &Connection, memory_item_id: &str) -> Result<i64, ApiError> {
    let version: i64 = conn
        .query_row(
            "
            SELECT COALESCE(MAX(version_number), 0) + 1
            FROM memory_item_versions
            WHERE memory_item_id = ?1
            ",
            params![memory_item_id],
            |row| row.get(0),
        )
        .map_err(|e| ApiError::internal(format!("failed to compute version number: {}", e)))?;
    Ok(version)
}

fn load_evidence_for_version(conn: &Connection, version_id: &str) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT evidence_record_id
            FROM memory_links
            WHERE memory_item_version_id = ?1
            ORDER BY created_at ASC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare evidence lookup: {}", e)))?;

    let rows = stmt
        .query_map(params![version_id], |r| r.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to execute evidence lookup: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to read evidence rows: {}", e)))
}

fn load_version_ids_for_item(
    conn: &Connection,
    memory_item_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT id
            FROM memory_item_versions
            WHERE memory_item_id = ?1
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare version lookup: {}", e)))?;

    let rows = stmt
        .query_map(params![memory_item_id], |r| r.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to execute version lookup: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to read version rows: {}", e)))
}

fn load_evidence_ids_for_versions(
    conn: &Connection,
    version_ids: &[String],
) -> Result<BTreeSet<String>, ApiError> {
    let mut evidence_ids = BTreeSet::new();
    for version_id in version_ids {
        for evidence_id in load_evidence_for_version(conn, version_id)? {
            evidence_ids.insert(evidence_id);
        }
    }
    Ok(evidence_ids)
}

fn ensure_graph_entity(
    conn: &Connection,
    entity: &GraphEntityRef,
    now: &str,
) -> Result<String, ApiError> {
    let entity_type = normalize_token(&entity.entity_type);
    let canonical_name = normalize_token(&entity.canonical_name);
    if let Some(canonical_entity_id) =
        resolve_graph_entity_alias_id(conn, &entity_type, &canonical_name)?
    {
        return Ok(canonical_entity_id);
    }

    ensure_graph_entity_without_alias(conn, &entity_type, &canonical_name, now)
}

fn ensure_graph_entity_without_alias(
    conn: &Connection,
    entity_type: &str,
    canonical_name: &str,
    now: &str,
) -> Result<String, ApiError> {
    let entity_type = normalize_token(entity_type);
    let canonical_name = normalize_token(canonical_name);

    let existing: Option<String> = conn
        .query_row(
            "
            SELECT id
            FROM graph_entities
            WHERE entity_type = ?1 AND canonical_name = ?2
            LIMIT 1
            ",
            params![&entity_type, &canonical_name],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup graph entity: {}", e)))?;

    if let Some(id) = existing {
        return Ok(id);
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO graph_entities (
          id, entity_type, canonical_name, attributes_json, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, '{}', 'active', ?4, ?4)
        ",
        params![&id, &entity_type, &canonical_name, now],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert graph entity: {}", e)))?;

    Ok(id)
}

fn load_graph_entity_ref_by_id(
    conn: &Connection,
    entity_id: &str,
) -> Result<GraphEntityRef, ApiError> {
    conn.query_row(
        "
        SELECT entity_type, canonical_name
        FROM graph_entities
        WHERE id = ?1
        LIMIT 1
        ",
        params![entity_id],
        |row| {
            Ok(GraphEntityRef {
                entity_type: row.get(0)?,
                canonical_name: row.get(1)?,
            })
        },
    )
    .optional()
    .map_err(|e| ApiError::internal(format!("failed to lookup graph entity by id: {}", e)))?
    .ok_or_else(|| ApiError::not_found("graph entity not found"))
}

fn resolve_graph_entity_alias_id(
    conn: &Connection,
    entity_type: &str,
    entity_name: &str,
) -> Result<Option<String>, ApiError> {
    let entity_type = normalize_token(entity_type);
    let mut current_name = normalize_token(entity_name);
    let mut resolved_id = None;
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(format!("{}:{}", entity_type, current_name)) {
            return Err(ApiError::internal(format!(
                "graph entity alias cycle detected for {}:{}",
                entity_type, entity_name
            )));
        }

        let alias_target: Option<String> = conn
            .query_row(
                "
                SELECT canonical_entity_id
                FROM graph_entity_aliases
                WHERE entity_type = ?1 AND alias_name = ?2
                LIMIT 1
                ",
                params![&entity_type, &current_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| ApiError::internal(format!("failed to lookup graph alias: {}", e)))?;

        let Some(alias_target) = alias_target else {
            return Ok(resolved_id);
        };

        let canonical_entity = load_graph_entity_ref_by_id(conn, &alias_target)?;
        current_name = canonical_entity.canonical_name;
        resolved_id = Some(alias_target);
    }
}

fn canonicalize_graph_entity_name_for_key(
    conn: &Connection,
    entity_type: &str,
    entity_name: &str,
) -> Result<String, ApiError> {
    if let Some(entity_id) = resolve_graph_entity_alias_id(conn, entity_type, entity_name)? {
        return Ok(load_graph_entity_ref_by_id(conn, &entity_id)?.canonical_name);
    }

    Ok(normalize_token(entity_name))
}

fn canonicalize_graph_predicate(conn: &Connection, predicate: &str) -> Result<String, ApiError> {
    let mut current = normalize_token(predicate);
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(current.clone()) {
            return Err(ApiError::internal(format!(
                "graph predicate alias cycle detected for {}",
                predicate
            )));
        }

        let canonical: Option<String> = conn
            .query_row(
                "
                SELECT canonical_predicate
                FROM graph_predicate_aliases
                WHERE alias_predicate = ?1
                LIMIT 1
                ",
                params![&current],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                ApiError::internal(format!("failed to lookup graph predicate alias: {}", e))
            })?;

        let Some(next) = canonical else {
            return Ok(current);
        };
        current = normalize_token(&next);
    }
}

fn build_graph_relationship_key(
    conn: &Connection,
    subject_entity_id: &str,
    predicate: &str,
    object_entity_id: &str,
) -> Result<String, ApiError> {
    let subject = load_graph_entity_ref_by_id(conn, subject_entity_id)?;
    let object = load_graph_entity_ref_by_id(conn, object_entity_id)?;
    Ok(format!(
        "{}|{}|{}",
        subject.canonical_name,
        normalize_token(predicate),
        object.canonical_name
    ))
}

fn ensure_graph_relationship(
    conn: &Connection,
    canonical_key: &str,
    subject_entity_id: &str,
    predicate: &str,
    object_entity_id: &str,
    now: &str,
) -> Result<(String, Option<String>), ApiError> {
    let existing: Option<(String, Option<String>)> = conn
        .query_row(
            "
            SELECT id, active_version_id
            FROM graph_relationships
            WHERE canonical_key = ?1
            LIMIT 1
            ",
            params![canonical_key],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| ApiError::internal(format!("failed to lookup graph relationship: {}", e)))?;

    if let Some(v) = existing {
        return Ok(v);
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO graph_relationships (
          id, canonical_key, subject_entity_id, predicate, object_entity_id,
          active_version_id, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'active', ?6, ?6)
        ",
        params![
            &id,
            canonical_key,
            subject_entity_id,
            normalize_token(predicate),
            object_entity_id,
            now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert graph relationship: {}", e)))?;

    Ok((id, None))
}

fn count_graph_entity_aliases(conn: &Connection) -> Result<usize, ApiError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM graph_entity_aliases", [], |row| {
            row.get(0)
        })
        .map_err(|e| ApiError::internal(format!("failed to count entity aliases: {}", e)))?;
    Ok(count as usize)
}

fn count_graph_predicate_aliases(conn: &Connection) -> Result<usize, ApiError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM graph_predicate_aliases", [], |row| {
            row.get(0)
        })
        .map_err(|e| ApiError::internal(format!("failed to count predicate aliases: {}", e)))?;
    Ok(count as usize)
}

fn load_graph_compaction_candidates(
    conn: &Connection,
) -> Result<Vec<GraphCompactionCandidate>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT
              gr.id,
              gr.canonical_key,
              gr.subject_entity_id,
              se.entity_type,
              se.canonical_name,
              gr.predicate,
              gr.object_entity_id,
              oe.entity_type,
              oe.canonical_name,
              gr.active_version_id,
              grv.confidence,
              grv.attributes_json,
              gr.updated_at
            FROM graph_relationships gr
            JOIN graph_entities se ON se.id = gr.subject_entity_id
            JOIN graph_entities oe ON oe.id = gr.object_entity_id
            JOIN graph_relationship_versions grv ON grv.id = gr.active_version_id
            WHERE gr.status = 'active'
            ORDER BY gr.updated_at DESC, gr.id ASC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare compaction query: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(GraphCompactionCandidate {
                relationship_id: row.get(0)?,
                canonical_key: row.get(1)?,
                subject_entity_id: row.get(2)?,
                subject_entity_type: row.get(3)?,
                subject_canonical_name: row.get(4)?,
                predicate: row.get(5)?,
                object_entity_id: row.get(6)?,
                object_entity_type: row.get(7)?,
                object_canonical_name: row.get(8)?,
                active_version_id: row.get(9)?,
                active_confidence: row.get(10)?,
                active_attributes_json: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to execute compaction query: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to read compaction rows: {}", e)))
}

fn compact_graph_relationships(
    conn: &Connection,
    dry_run: bool,
    run_at: &str,
) -> Result<GraphCompactionCounts, ApiError> {
    let candidates = load_graph_compaction_candidates(conn)?;
    let mut grouped: BTreeMap<String, Vec<GraphCompactionPlanEntry>> = BTreeMap::new();

    for candidate in candidates {
        let target_subject_entity_id = resolve_graph_entity_alias_id(
            conn,
            &candidate.subject_entity_type,
            &candidate.subject_canonical_name,
        )?
        .unwrap_or_else(|| candidate.subject_entity_id.clone());
        let target_object_entity_id = resolve_graph_entity_alias_id(
            conn,
            &candidate.object_entity_type,
            &candidate.object_canonical_name,
        )?
        .unwrap_or_else(|| candidate.object_entity_id.clone());
        let target_predicate = canonicalize_graph_predicate(conn, &candidate.predicate)?;
        let target_subject_canonical_name =
            load_graph_entity_ref_by_id(conn, &target_subject_entity_id)?.canonical_name;
        let target_object_canonical_name =
            load_graph_entity_ref_by_id(conn, &target_object_entity_id)?.canonical_name;
        let target_canonical_key = format!(
            "{}|{}|{}",
            target_subject_canonical_name, target_predicate, target_object_canonical_name
        );

        grouped
            .entry(target_canonical_key.clone())
            .or_default()
            .push(GraphCompactionPlanEntry {
                candidate,
                target_subject_entity_id,
                target_subject_canonical_name,
                target_object_entity_id,
                target_object_canonical_name,
                target_predicate,
                target_canonical_key,
            });
    }

    let mut counts = GraphCompactionCounts::default();
    let mut touched_entity_ids = BTreeSet::new();

    for plan_entries in grouped.values_mut() {
        plan_entries.sort_by(compare_graph_compaction_priority);
        let group_has_changes = plan_entries.iter().any(|entry| {
            entry.candidate.subject_entity_id != entry.target_subject_entity_id
                || entry.candidate.object_entity_id != entry.target_object_entity_id
                || entry.candidate.predicate != entry.target_predicate
                || entry.candidate.canonical_key != entry.target_canonical_key
        });

        if plan_entries.len() == 1 && !group_has_changes {
            continue;
        }

        let winner = plan_entries[0].clone();

        if plan_entries.len() == 1 {
            counts.canonicalized_relationships += 1;
            touched_entity_ids.extend(changed_entity_ids(&winner));
            if !dry_run {
                update_graph_relationship_to_target(conn, &winner, run_at)?;
            }
            continue;
        }

        counts.redirected_relationships += plan_entries.len() - 1;
        counts.merged_versions_created += 1;
        if winner.candidate.canonical_key != winner.target_canonical_key
            || winner.candidate.subject_entity_id != winner.target_subject_entity_id
            || winner.candidate.object_entity_id != winner.target_object_entity_id
            || winner.candidate.predicate != winner.target_predicate
        {
            counts.canonicalized_relationships += 1;
        }
        touched_entity_ids.extend(changed_entity_ids(&winner));
        for source in plan_entries.iter().skip(1) {
            touched_entity_ids.extend(changed_entity_ids(source));
        }

        if !dry_run {
            merge_graph_compaction_group(conn, plan_entries, run_at)?;
        }
    }

    if !dry_run {
        counts.compacted_entities =
            compact_orphaned_graph_entities(conn, &touched_entity_ids, run_at)?;
    }

    Ok(counts)
}

fn changed_entity_ids(plan: &GraphCompactionPlanEntry) -> Vec<String> {
    let mut ids = Vec::new();
    if plan.candidate.subject_entity_id != plan.target_subject_entity_id {
        ids.push(plan.candidate.subject_entity_id.clone());
    }
    if plan.candidate.object_entity_id != plan.target_object_entity_id {
        ids.push(plan.candidate.object_entity_id.clone());
    }
    ids
}

fn compare_graph_compaction_priority(
    a: &GraphCompactionPlanEntry,
    b: &GraphCompactionPlanEntry,
) -> Ordering {
    b.candidate
        .active_confidence
        .partial_cmp(&a.candidate.active_confidence)
        .unwrap_or(Ordering::Equal)
        .then_with(|| compare_rfc3339_desc(&a.candidate.updated_at, &b.candidate.updated_at))
        .then_with(|| {
            a.candidate
                .relationship_id
                .cmp(&b.candidate.relationship_id)
        })
}

fn compare_rfc3339_desc(a: &str, b: &str) -> Ordering {
    let a_ts = DateTime::parse_from_rfc3339(a)
        .map(|dt| dt.timestamp())
        .unwrap_or_default();
    let b_ts = DateTime::parse_from_rfc3339(b)
        .map(|dt| dt.timestamp())
        .unwrap_or_default();
    b_ts.cmp(&a_ts)
}

fn update_graph_relationship_to_target(
    conn: &Connection,
    plan: &GraphCompactionPlanEntry,
    now: &str,
) -> Result<(), ApiError> {
    conn.execute(
        "
        UPDATE graph_relationships
        SET canonical_key = ?2,
            subject_entity_id = ?3,
            predicate = ?4,
            object_entity_id = ?5,
            updated_at = ?6
        WHERE id = ?1
        ",
        params![
            &plan.candidate.relationship_id,
            &plan.target_canonical_key,
            &plan.target_subject_entity_id,
            &plan.target_predicate,
            &plan.target_object_entity_id,
            now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to update canonical relationship: {}", e)))?;

    Ok(())
}

fn merge_graph_compaction_group(
    conn: &Connection,
    plan_entries: &[GraphCompactionPlanEntry],
    now: &str,
) -> Result<(), ApiError> {
    let winner = plan_entries
        .first()
        .ok_or_else(|| ApiError::internal("missing graph compaction winner"))?;
    let new_version_id = Uuid::new_v4().to_string();
    let new_version_number =
        next_graph_relationship_version_number(conn, &winner.candidate.relationship_id)?;

    let mut merged_attributes = Map::new();
    let mut merged_evidence_ids = BTreeSet::new();
    let merged_confidence = plan_entries
        .iter()
        .map(|entry| entry.candidate.active_confidence)
        .fold(0.0_f32, f32::max);

    for source in plan_entries.iter().skip(1) {
        conn.execute(
            "
            UPDATE graph_relationships
            SET status = 'redirected',
                canonical_key = ?2,
                updated_at = ?3
            WHERE id = ?1
            ",
            params![
                &source.candidate.relationship_id,
                format!(
                    "redirected:{}:{}",
                    source.candidate.canonical_key, source.candidate.relationship_id
                ),
                now,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to redirect relationship: {}", e)))?;
    }

    for entry in plan_entries.iter().rev() {
        let attributes_value: Value = serde_json::from_str(&entry.candidate.active_attributes_json)
            .map_err(|e| {
                ApiError::internal(format!("failed to decode compaction attributes: {}", e))
            })?;
        let object = attributes_value
            .as_object()
            .ok_or_else(|| ApiError::internal("graph attributes_json must be an object"))?;
        for (key, value) in object {
            merged_attributes.insert(key.clone(), value.clone());
        }
        for evidence_id in
            load_evidence_for_graph_relationship_version(conn, &entry.candidate.active_version_id)?
        {
            merged_evidence_ids.insert(evidence_id);
        }
    }

    conn.execute(
        "
        UPDATE graph_relationship_versions
        SET state = 'superseded', valid_to = ?2
        WHERE id = ?1
        ",
        params![&winner.candidate.active_version_id, now],
    )
    .map_err(|e| ApiError::internal(format!("failed to supersede winner version: {}", e)))?;

    conn.execute(
        "
        INSERT INTO graph_relationship_versions (
          id, relationship_id, version_number, state, confidence, attributes_json,
          supersedes_version_id, valid_from, valid_to, created_at
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, NULL, ?7)
        ",
        params![
            &new_version_id,
            &winner.candidate.relationship_id,
            new_version_number,
            merged_confidence,
            serde_json::to_string(&Value::Object(merged_attributes)).map_err(|e| {
                ApiError::internal(format!("failed to encode merged graph attributes: {}", e))
            })?,
            &winner.candidate.active_version_id,
            now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert merged graph version: {}", e)))?;

    for evidence_id in merged_evidence_ids {
        conn.execute(
            "
            INSERT INTO graph_relationship_evidence_links (
              id, relationship_version_id, evidence_record_id, created_at
            ) VALUES (?1, ?2, ?3, ?4)
            ",
            params![
                Uuid::new_v4().to_string(),
                &new_version_id,
                evidence_id,
                now,
            ],
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to insert merged graph evidence: {}", e))
        })?;
    }

    update_graph_relationship_to_target(conn, winner, now)?;
    conn.execute(
        "
        UPDATE graph_relationships
        SET active_version_id = ?2, status = 'active', updated_at = ?3
        WHERE id = ?1
        ",
        params![&winner.candidate.relationship_id, &new_version_id, now],
    )
    .map_err(|e| ApiError::internal(format!("failed to activate merged relationship: {}", e)))?;

    for source in plan_entries.iter().skip(1) {
        conn.execute(
            "
            INSERT OR REPLACE INTO graph_relationship_redirects (
              id, source_relationship_id, target_relationship_id, reason_json, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            params![
                Uuid::new_v4().to_string(),
                &source.candidate.relationship_id,
                &winner.candidate.relationship_id,
                serde_json::to_string(&json!({
                    "source_canonical_key": source.candidate.canonical_key,
                    "target_canonical_key": winner.target_canonical_key,
                    "target_subject_canonical_name": winner.target_subject_canonical_name,
                    "target_object_canonical_name": winner.target_object_canonical_name,
                    "target_predicate": winner.target_predicate,
                }))
                .map_err(|e| ApiError::internal(format!(
                    "failed to encode graph redirect reason: {}",
                    e
                )))?,
                now,
            ],
        )
        .map_err(|e| ApiError::internal(format!("failed to insert graph redirect: {}", e)))?;
    }

    Ok(())
}

fn compact_orphaned_graph_entities(
    conn: &Connection,
    entity_ids: &BTreeSet<String>,
    now: &str,
) -> Result<usize, ApiError> {
    let mut compacted = 0usize;
    for entity_id in entity_ids {
        let active_reference: Option<String> = conn
            .query_row(
                "
                SELECT id
                FROM graph_relationships
                WHERE status = 'active'
                  AND (subject_entity_id = ?1 OR object_entity_id = ?1)
                LIMIT 1
                ",
                params![entity_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                ApiError::internal(format!("failed to lookup active entity refs: {}", e))
            })?;

        if active_reference.is_none() {
            compacted += conn
                .execute(
                    "
                    UPDATE graph_entities
                    SET status = 'compacted', updated_at = ?2
                    WHERE id = ?1 AND status != 'compacted'
                    ",
                    params![entity_id, now],
                )
                .map_err(|e| ApiError::internal(format!("failed to compact entity: {}", e)))?;
        }
    }

    Ok(compacted)
}

fn next_graph_relationship_version_number(
    conn: &Connection,
    relationship_id: &str,
) -> Result<i64, ApiError> {
    let next: i64 = conn
        .query_row(
            "
            SELECT COALESCE(MAX(version_number), 0) + 1
            FROM graph_relationship_versions
            WHERE relationship_id = ?1
            ",
            params![relationship_id],
            |r| r.get(0),
        )
        .map_err(|e| ApiError::internal(format!("failed graph version number query: {}", e)))?;
    Ok(next)
}

fn load_evidence_for_graph_relationship_version(
    conn: &Connection,
    version_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT evidence_record_id
            FROM graph_relationship_evidence_links
            WHERE relationship_version_id = ?1
            ORDER BY created_at ASC
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to prepare graph evidence query: {}", e))
        })?;

    let rows = stmt
        .query_map(params![version_id], |r| r.get::<_, String>(0))
        .map_err(|e| {
            ApiError::internal(format!("failed to execute graph evidence query: {}", e))
        })?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to read graph evidence rows: {}", e)))
}

fn delete_memory_item_by_id_tx(
    conn: &Connection,
    memory_item_id: &str,
    forget_evidence: bool,
) -> Result<DeleteCounts, ApiError> {
    let version_ids = load_version_ids_for_item(conn, memory_item_id)?;
    let linked_evidence_ids = load_evidence_ids_for_versions(conn, &version_ids)?;
    detach_memory_from_retrieval_state(conn, memory_item_id)?;

    let deleted_links = conn
        .execute(
            "
            DELETE FROM memory_links
            WHERE memory_item_version_id IN (
              SELECT id FROM memory_item_versions WHERE memory_item_id = ?1
            )
            ",
            params![memory_item_id],
        )
        .map_err(|e| ApiError::internal(format!("failed to delete memory links: {}", e)))?;

    let deleted_versions = conn
        .execute(
            "DELETE FROM memory_item_versions WHERE memory_item_id = ?1",
            params![memory_item_id],
        )
        .map_err(|e| ApiError::internal(format!("failed to delete memory versions: {}", e)))?;

    let deleted_items = conn
        .execute(
            "DELETE FROM memory_items WHERE id = ?1",
            params![memory_item_id],
        )
        .map_err(|e| ApiError::internal(format!("failed to delete memory item: {}", e)))?;

    if deleted_items == 0 {
        return Err(ApiError::not_found("memory item not found"));
    }

    let mut deleted_proposals = 0usize;
    let mut deleted_evidence = 0usize;
    if forget_evidence {
        for evidence_id in linked_evidence_ids {
            if !evidence_has_active_references(conn, &evidence_id)? {
                let import_proposal_ids =
                    load_import_proposal_ids_for_evidence(conn, &evidence_id)?;
                conn.execute(
                    "DELETE FROM import_job_items WHERE evidence_record_id = ?1",
                    params![&evidence_id],
                )
                .map_err(|e| {
                    ApiError::internal(format!("failed to delete import evidence refs: {}", e))
                })?;
                deleted_proposals += delete_import_proposals_by_id(conn, &import_proposal_ids)?;
                deleted_evidence += conn
                    .execute(
                        "DELETE FROM evidence_records WHERE id = ?1",
                        params![&evidence_id],
                    )
                    .map_err(|e| {
                        ApiError::internal(format!("failed to delete evidence record: {}", e))
                    })?;
            }
        }
    }

    Ok(DeleteCounts {
        deleted_versions,
        deleted_links,
        deleted_proposals,
        deleted_evidence,
    })
}

fn detach_memory_from_retrieval_state(
    conn: &Connection,
    memory_item_id: &str,
) -> Result<(), ApiError> {
    let observation_ids = load_observation_ids_for_memory(conn, memory_item_id)?;

    conn.execute(
        "DELETE FROM retrieval_documents_fts WHERE source_type = 'memory_item' AND source_id = ?1",
        params![memory_item_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to delete memory FTS document: {}", e)))?;
    conn.execute(
        "DELETE FROM memory_item_metadata WHERE memory_item_id = ?1",
        params![memory_item_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to delete memory metadata: {}", e)))?;
    conn.execute(
        "UPDATE import_job_items SET memory_item_id = NULL WHERE memory_item_id = ?1",
        params![memory_item_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to detach import job memory refs: {}", e)))?;
    conn.execute(
        "UPDATE observation_events SET memory_item_id = NULL WHERE memory_item_id = ?1",
        params![memory_item_id],
    )
    .map_err(|e| {
        ApiError::internal(format!(
            "failed to detach observation event memory refs: {}",
            e
        ))
    })?;
    conn.execute(
        "DELETE FROM observation_memory_links WHERE memory_item_id = ?1",
        params![memory_item_id],
    )
    .map_err(|e| ApiError::internal(format!("failed to delete observation memory links: {}", e)))?;

    for observation_id in observation_ids {
        let remaining_links: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM observation_memory_links WHERE observation_id = ?1",
                params![&observation_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to count observation memory links: {}", e))
            })?;
        if remaining_links == 0 {
            conn.execute(
                "DELETE FROM retrieval_documents_fts WHERE source_type = 'observation' AND source_id = ?1",
                params![&observation_id],
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to delete observation FTS document: {}", e))
            })?;
            conn.execute(
                "DELETE FROM observation_events WHERE observation_id = ?1",
                params![&observation_id],
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to delete observation events: {}", e))
            })?;
            conn.execute(
                "DELETE FROM observation_evidence_links WHERE observation_id = ?1",
                params![&observation_id],
            )
            .map_err(|e| {
                ApiError::internal(format!(
                    "failed to delete observation evidence links: {}",
                    e
                ))
            })?;
            conn.execute(
                "DELETE FROM observations WHERE id = ?1",
                params![&observation_id],
            )
            .map_err(|e| ApiError::internal(format!("failed to delete observation: {}", e)))?;
        }
    }

    Ok(())
}

fn load_observation_ids_for_memory(
    conn: &Connection,
    memory_item_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT observation_id
            FROM observation_memory_links
            WHERE memory_item_id = ?1
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to prepare observation id lookup: {}", e))
        })?;
    let rows = stmt
        .query_map(params![memory_item_id], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to query observation ids: {}", e)))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read observation ids: {}", e)))
}

fn evidence_has_active_references(conn: &Connection, evidence_id: &str) -> Result<bool, ApiError> {
    let refs: i64 = conn
        .query_row(
            "
            SELECT
              (SELECT COUNT(*) FROM memory_links WHERE evidence_record_id = ?1) +
              (SELECT COUNT(*) FROM observation_evidence_links WHERE evidence_record_id = ?1) +
              (SELECT COUNT(*) FROM graph_relationship_evidence_links WHERE evidence_record_id = ?1)
            ",
            params![evidence_id],
            |row| row.get(0),
        )
        .map_err(|e| ApiError::internal(format!("failed evidence reference lookup: {}", e)))?;
    Ok(refs > 0)
}

fn load_import_source_memory_ids(
    conn: &Connection,
    source_ref: &str,
    source_type: Option<&str>,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT DISTINCT iji.memory_item_id
            FROM import_job_items iji
            JOIN import_jobs ij ON ij.id = iji.import_job_id
            WHERE iji.source_ref = ?1
              AND (?2 IS NULL OR ij.source_type = ?2)
              AND iji.memory_item_id IS NOT NULL
            ORDER BY iji.memory_item_id
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to prepare import source memory lookup: {}",
                e
            ))
        })?;
    let rows = stmt
        .query_map(params![source_ref, source_type], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|e| {
            ApiError::internal(format!("failed to query import source memories: {}", e))
        })?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read import source memories: {}", e)))
}

fn delete_import_source_pending_proposals(
    conn: &Connection,
    source_ref: &str,
    source_type: Option<&str>,
    forget_evidence: bool,
) -> Result<PendingImportDeleteCounts, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT iji.id, iji.proposal_id, iji.evidence_record_id
            FROM import_job_items iji
            JOIN import_jobs ij ON ij.id = iji.import_job_id
            WHERE iji.source_ref = ?1
              AND (?2 IS NULL OR ij.source_type = ?2)
              AND iji.memory_item_id IS NULL
              AND iji.proposal_id IS NOT NULL
            ORDER BY iji.id
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!(
                "failed to prepare pending import proposal lookup: {}",
                e
            ))
        })?;
    let rows = stmt
        .query_map(params![source_ref, source_type], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| {
            ApiError::internal(format!("failed to query pending import proposals: {}", e))
        })?;
    let pending = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read pending imports: {}", e)))?;

    let mut counts = PendingImportDeleteCounts::default();
    for (item_id, proposal_id, evidence_id) in pending {
        if forget_evidence {
            conn.execute(
                "DELETE FROM import_job_items WHERE id = ?1",
                params![&item_id],
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to delete pending import item: {}", e))
            })?;
        } else {
            conn.execute(
                "
                UPDATE import_job_items
                SET status = 'source_forgotten', proposal_id = NULL
                WHERE id = ?1
                ",
                params![&item_id],
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to detach pending import item: {}", e))
            })?;
        }
        counts.deleted_proposals += conn
            .execute(
                "DELETE FROM memory_proposals WHERE id = ?1",
                params![&proposal_id],
            )
            .map_err(|e| {
                ApiError::internal(format!("failed to delete pending import proposal: {}", e))
            })?;
        if forget_evidence && !evidence_has_active_references(conn, &evidence_id)? {
            counts.deleted_evidence += conn
                .execute(
                    "DELETE FROM evidence_records WHERE id = ?1",
                    params![&evidence_id],
                )
                .map_err(|e| {
                    ApiError::internal(format!("failed to delete pending import evidence: {}", e))
                })?;
        }
    }
    Ok(counts)
}

fn load_import_proposal_ids_for_evidence(
    conn: &Connection,
    evidence_id: &str,
) -> Result<Vec<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT DISTINCT proposal_id
            FROM import_job_items
            WHERE evidence_record_id = ?1
              AND proposal_id IS NOT NULL
            ",
        )
        .map_err(|e| {
            ApiError::internal(format!("failed to prepare import proposal lookup: {}", e))
        })?;
    let rows = stmt
        .query_map(params![evidence_id], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to query import proposals: {}", e)))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| ApiError::internal(format!("failed to read import proposals: {}", e)))
}

fn delete_import_proposals_by_id(
    conn: &Connection,
    proposal_ids: &[String],
) -> Result<usize, ApiError> {
    let mut deleted = 0usize;
    for proposal_id in proposal_ids {
        deleted += conn
            .execute(
                "DELETE FROM memory_proposals WHERE id = ?1",
                params![proposal_id],
            )
            .map_err(|e| ApiError::internal(format!("failed to delete import proposal: {}", e)))?;
    }
    Ok(deleted)
}

fn apply_retention_policy(
    conn: &Connection,
    policy: &RetentionPolicyPayload,
    run_at: DateTime<Utc>,
    dry_run: bool,
) -> Result<(usize, usize, DeleteCounts), ApiError> {
    let cutoff = run_at - Duration::days(policy.max_age_days);

    let mut stmt = conn
        .prepare(
            "
            SELECT id, canonical_key, memory_type, updated_at
            FROM memory_items
            WHERE status = 'active'
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare retention query: {}", e)))?;

    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| ApiError::internal(format!("failed to execute retention query: {}", e)))?;

    let mut candidates = Vec::new();
    for row in rows {
        let (memory_item_id, canonical_key, memory_type, updated_at_raw) =
            row.map_err(|e| ApiError::internal(format!("failed to read retention row: {}", e)))?;

        if let Some(required_type) = &policy.memory_type {
            if &memory_type != required_type {
                continue;
            }
        }

        if let Some(prefix) = &policy.canonical_prefix {
            if !canonical_key.starts_with(prefix) {
                continue;
            }
        }

        let updated_at = DateTime::parse_from_rfc3339(&updated_at_raw)
            .map_err(|e| ApiError::internal(format!("invalid memory updated_at: {}", e)))?
            .with_timezone(&Utc);

        if updated_at <= cutoff {
            candidates.push(memory_item_id);
        }
    }

    let matched_memory_items = candidates.len();

    if dry_run {
        return Ok((matched_memory_items, 0, DeleteCounts::default()));
    }

    let mut counts = DeleteCounts::default();
    let mut deleted_memory_items = 0usize;

    for memory_item_id in candidates {
        let item_counts =
            delete_memory_item_by_id_tx(conn, &memory_item_id, policy.forget_evidence)?;
        deleted_memory_items += 1;
        counts.deleted_versions += item_counts.deleted_versions;
        counts.deleted_links += item_counts.deleted_links;
        counts.deleted_evidence += item_counts.deleted_evidence;
    }

    Ok((matched_memory_items, deleted_memory_items, counts))
}

fn load_retention_policies(
    conn: &Connection,
    requested_policy_names: &[String],
) -> Result<Vec<RetentionPolicyPayload>, ApiError> {
    let requested: BTreeSet<String> = requested_policy_names
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    let mut stmt = conn
        .prepare(
            "
            SELECT rule_json
            FROM policy_rules
            WHERE rule_name LIKE 'retention/%' AND enabled = 1
            ORDER BY updated_at DESC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare policy query: {}", e)))?;

    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to execute policy query: {}", e)))?;

    let mut policies = Vec::new();
    for row in rows {
        let raw_json =
            row.map_err(|e| ApiError::internal(format!("failed to read policy row: {}", e)))?;
        let policy: RetentionPolicyPayload = serde_json::from_str(&raw_json)
            .map_err(|e| ApiError::internal(format!("failed to decode retention policy: {}", e)))?;

        if requested.is_empty() || requested.contains(&policy.policy_name) {
            policies.push(policy);
        }
    }

    if !requested.is_empty() {
        let loaded: BTreeSet<String> = policies.iter().map(|p| p.policy_name.clone()).collect();
        let missing: Vec<String> = requested
            .iter()
            .filter(|name| !loaded.contains(*name))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(ApiError::bad_request(format!(
                "unknown retention policy names: {}",
                missing.join(", ")
            )));
        }
    }

    Ok(policies)
}

fn retention_rule_name(policy_name: &str) -> String {
    format!("retention/{}", policy_name)
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn normalize_token(input: &str) -> String {
    input.trim().to_lowercase()
}

fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn default_true() -> bool {
    true
}

fn ensure_data_dir(db_path: &str) -> anyhow::Result<()> {
    if let Some(parent) = std::path::Path::new(db_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn init_db(db_path: &str) -> anyhow::Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(include_str!("../../../db/migrations/0001_init.sql"))?;
    conn.execute_batch(include_str!("../../../db/migrations/0002_indexes.sql"))?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0003_knowledge_graph.sql"
    ))?;
    apply_graph_confidence_migration(&conn)?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0004_graph_confidence.sql"
    ))?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0005_graph_canonicalization.sql"
    ))?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0006_retrieval_v2_foundation.sql"
    ))?;
    apply_observation_canonical_key_migration(&conn)?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0007_observation_canonical_keys.sql"
    ))?;
    conn.execute_batch(include_str!(
        "../../../db/migrations/0008_observation_events.sql"
    ))?;
    conn.execute_batch(include_str!("../../../db/migrations/0009_import_jobs.sql"))?;
    Ok(())
}

fn apply_graph_confidence_migration(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(graph_relationship_versions)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;

    let mut has_confidence = false;
    for column in columns {
        if column? == "confidence" {
            has_confidence = true;
            break;
        }
    }

    if !has_confidence {
        conn.execute(
            "
            ALTER TABLE graph_relationship_versions
            ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0
            ",
            [],
        )?;
    }

    Ok(())
}

fn apply_observation_canonical_key_migration(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(observations)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;

    let mut has_canonical_key = false;
    for column in columns {
        if column? == "canonical_key" {
            has_canonical_key = true;
            break;
        }
    }

    if !has_canonical_key {
        conn.execute("ALTER TABLE observations ADD COLUMN canonical_key TEXT", [])?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};
    use serde_json::json;

    use super::{
        apply_graph_confidence_migration, apply_observation_canonical_key_migration,
        compact_graph_relationships, delete_memory_item_by_id_tx, ensure_graph_entity,
        ensure_graph_entity_without_alias, forget_import_source_tx, import_markdown_content,
        load_import_job, parse_markdown_memory_items, upsert_observation_for_memory,
        CreateProposalRequest, ForgetImportSourceRequest, GraphEntityRef, ImportMarkdownRequest,
    };

    #[test]
    fn confidence_validation() {
        let req = CreateProposalRequest {
            proposal_type: "fact_update".to_string(),
            subject_key: "pref:language".to_string(),
            memory_type: "preference".to_string(),
            value_json: json!({"value":"Rust"}),
            evidence_record_ids: vec![],
            scope: None,
            freshness: None,
            confidence: 1.1,
        };

        assert!(req.validate().is_err());
    }

    #[test]
    fn markdown_import_parser_extracts_sectioned_memory_items() {
        let items = parse_markdown_memory_items(
            r#"
# Project Memory

## Decisions

- Use SQLite for local-first storage.
- Avoid cloud sync in the MVP.

## Preferences

Prefer Rust for CLI tools.

```text
- ignore fenced code
```
"#,
        );

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].memory_type, "project");
        assert_eq!(items[2].memory_type, "preference");
        assert_eq!(items[2].statement, "Prefer Rust for CLI tools.");
    }

    #[test]
    fn markdown_import_commits_evidence_memory_and_observation() {
        let mut conn = test_conn();
        let imported_at = "2026-04-24T12:00:00+00:00";
        let request = ImportMarkdownRequest {
            source_ref: "AGENTS.md".to_string(),
            content: "# Decisions\n\n- Use SQLite for local-first storage.".to_string(),
            source_type: None,
            scope: None,
            commit: Some(true),
            confidence: Some(0.81),
        };

        let tx = conn.transaction().expect("transaction should start");
        let response =
            import_markdown_content(&tx, &request, imported_at).expect("import should work");
        tx.commit().expect("transaction should commit");

        assert_eq!(response.imported_items, 1);
        assert_eq!(response.committed_items, 1);
        assert_eq!(response.skipped_items, 0);
        assert!(!response.job_id.is_empty());
        assert_eq!(response.evidence_record_ids.len(), 1);
        assert_eq!(response.proposal_ids.len(), 1);
        assert_eq!(response.memory_item_ids.len(), 1);

        let memory_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_items", [], |row| row.get(0))
            .expect("memory count should be queryable");
        assert_eq!(memory_count, 1);

        let observation_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))
            .expect("observation count should be queryable");
        assert_eq!(observation_count, 1);

        let fts_count: i64 = conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM retrieval_documents_fts
                WHERE retrieval_documents_fts MATCH 'sqlite'
                ",
                [],
                |row| row.get(0),
            )
            .expect("FTS count should be queryable");
        assert_eq!(fts_count, 2);

        let import_job = load_import_job(&conn, &response.job_id).expect("job should load");
        assert_eq!(import_job.imported_items, 1);
        assert_eq!(import_job.committed_items, 1);
        assert_eq!(import_job.skipped_items, 0);
        assert_eq!(import_job.items.len(), 1);
        assert_eq!(import_job.items[0].status, "committed");

        let tx = conn
            .transaction()
            .expect("second import transaction should start");
        let repeated_response =
            import_markdown_content(&tx, &request, imported_at).expect("repeat import should work");
        tx.commit().expect("second transaction should commit");
        assert_eq!(repeated_response.imported_items, 1);
        assert_eq!(repeated_response.committed_items, 0);
        assert_eq!(repeated_response.skipped_items, 1);
        let repeated_job =
            load_import_job(&conn, &repeated_response.job_id).expect("repeat job should load");
        assert_eq!(repeated_job.items.len(), 1);
        assert_eq!(repeated_job.items[0].status, "skipped");
        assert!(repeated_job.items[0].memory_item_id.is_some());

        let version_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_item_versions", [], |row| {
                row.get(0)
            })
            .expect("version count should be queryable");
        assert_eq!(version_count, 1);

        let deleted = delete_memory_item_by_id_tx(&conn, &response.memory_item_ids[0], true)
            .expect("imported memory should remain forgettable");
        assert_eq!(deleted.deleted_evidence, 1);
        let remaining_memory_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_items", [], |row| row.get(0))
            .expect("remaining memory count should be queryable");
        assert_eq!(remaining_memory_count, 0);
        let remaining_evidence_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM evidence_records", [], |row| {
                row.get(0)
            })
            .expect("remaining evidence count should be queryable");
        assert_eq!(remaining_evidence_count, 0);
        let remaining_import_item_refs: i64 = conn
            .query_row("SELECT COUNT(*) FROM import_job_items", [], |row| {
                row.get(0)
            })
            .expect("remaining import item count should be queryable");
        assert_eq!(remaining_import_item_refs, 0);
    }

    #[test]
    fn markdown_import_source_forget_deletes_committed_and_pending_imports() {
        let mut conn = test_conn();
        let imported_at = "2026-04-24T12:00:00+00:00";
        let committed_request = ImportMarkdownRequest {
            source_ref: "AGENTS.md".to_string(),
            content:
                "# Decisions\n\n- Use SQLite for local-first storage.\n- Prefer Rust services."
                    .to_string(),
            source_type: None,
            scope: None,
            commit: Some(true),
            confidence: Some(0.82),
        };
        let pending_request = ImportMarkdownRequest {
            source_ref: "AGENTS.md".to_string(),
            content: "# Preferences\n\n- Keep agent memory local-first.".to_string(),
            source_type: None,
            scope: None,
            commit: Some(false),
            confidence: Some(0.72),
        };

        let tx = conn
            .transaction()
            .expect("committed import transaction should start");
        let committed =
            import_markdown_content(&tx, &committed_request, imported_at).expect("import works");
        tx.commit()
            .expect("committed import transaction should commit");
        assert_eq!(committed.committed_items, 2);

        let tx = conn
            .transaction()
            .expect("pending import transaction should start");
        let pending = import_markdown_content(&tx, &pending_request, imported_at)
            .expect("pending import works");
        tx.commit()
            .expect("pending import transaction should commit");
        assert_eq!(pending.committed_items, 0);
        assert_eq!(pending.proposal_ids.len(), 1);

        let response = forget_import_source_tx(
            &conn,
            ForgetImportSourceRequest {
                source_ref: " AGENTS.md ".to_string(),
                source_type: Some("local_markdown_memory".to_string()),
                forget_evidence: true,
            },
        )
        .expect("source forget should work");

        assert_eq!(response.source_ref, "AGENTS.md");
        assert_eq!(response.matched_memory_items, 2);
        assert_eq!(response.deleted_memory_items, 2);
        assert_eq!(response.deleted_proposals, 3);
        assert_eq!(response.deleted_evidence, 3);

        for table in [
            "memory_items",
            "memory_item_versions",
            "memory_links",
            "memory_proposals",
            "evidence_records",
            "observations",
            "observation_events",
            "observation_memory_links",
            "observation_evidence_links",
            "import_job_items",
            "retrieval_documents_fts",
        ] {
            let query = format!("SELECT COUNT(*) FROM {}", table);
            let count: i64 = conn
                .query_row(&query, [], |row| row.get(0))
                .expect("table count should be queryable");
            assert_eq!(count, 0, "{} should be empty after source forget", table);
        }

        let import_job_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM import_jobs", [], |row| row.get(0))
            .expect("import job count should be queryable");
        assert_eq!(import_job_count, 2);
    }

    #[test]
    fn memory_commit_observation_compiler_upserts_observation_links_and_fts() {
        let mut conn = test_conn();
        let now = "2026-04-24T12:00:00+00:00";
        insert_evidence(&conn, "evidence-observation", now);
        conn.execute(
            "
            INSERT INTO memory_items (
              id, memory_type, canonical_key, active_version_id, status, created_at, updated_at
            ) VALUES ('memory-observation', 'decision', 'project.storage', NULL, 'active', ?1, ?1)
            ",
            params![now],
        )
        .expect("memory item should insert");

        let tx = conn.transaction().expect("transaction should start");
        upsert_observation_for_memory(
            &tx,
            "memory-observation",
            "project.storage",
            "decision",
            &json!({"decision":"Yena uses SQLite for local-first storage."}),
            &["evidence-observation".to_string()],
            None,
            Some("stable"),
            0.92,
            now,
        )
        .expect("observation should compile");
        tx.commit().expect("transaction should commit");

        let (canonical_key, statement, proof_count, confidence): (String, String, i64, f32) = conn
            .query_row(
                "
                SELECT canonical_key, statement, proof_count, confidence
                FROM observations
                WHERE id = 'observation-decision-project-storage'
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("observation should be queryable");
        assert_eq!(canonical_key, "decision:project.storage");
        assert_eq!(statement, "Yena uses SQLite for local-first storage.");
        assert_eq!(proof_count, 1);
        assert_eq!(confidence, 0.92);

        let evidence_link_count: i64 = conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM observation_evidence_links
                WHERE observation_id = 'observation-decision-project-storage'
                ",
                [],
                |row| row.get(0),
            )
            .expect("evidence link count should be queryable");
        assert_eq!(evidence_link_count, 1);

        let fts_count: i64 = conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM retrieval_documents_fts
                WHERE source_type = 'observation'
                  AND source_id = 'observation-decision-project-storage'
                  AND retrieval_documents_fts MATCH 'sqlite'
                ",
                [],
                |row| row.get(0),
            )
            .expect("observation FTS document should be queryable");
        assert_eq!(fts_count, 1);

        insert_evidence(&conn, "evidence-observation-2", now);
        let tx = conn.transaction().expect("second transaction should start");
        upsert_observation_for_memory(
            &tx,
            "memory-observation",
            "project.storage",
            "decision",
            &json!({"decision":"Yena still uses SQLite for local-first storage."}),
            &[
                "evidence-observation".to_string(),
                "evidence-observation-2".to_string(),
            ],
            None,
            Some("strengthening"),
            0.95,
            now,
        )
        .expect("observation should strengthen");
        tx.commit().expect("second transaction should commit");

        let (
            observation_count,
            strengthened_proof_count,
            strengthened_confidence,
            strengthened_freshness,
        ): (i64, i64, f32, String) = conn
            .query_row(
                "
                SELECT
                  (SELECT COUNT(*) FROM observations WHERE canonical_key = 'decision:project.storage'),
                  proof_count,
                  confidence,
                  freshness
                FROM observations
                WHERE id = 'observation-decision-project-storage'
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("strengthened observation should be queryable");
        assert_eq!(observation_count, 1);
        assert_eq!(strengthened_proof_count, 2);
        assert_eq!(strengthened_confidence, 0.95);
        assert_eq!(strengthened_freshness, "strengthening");

        insert_evidence(&conn, "evidence-observation-3", now);
        let tx = conn
            .transaction()
            .expect("contradiction transaction should start");
        upsert_observation_for_memory(
            &tx,
            "memory-observation",
            "project.storage",
            "decision",
            &json!({"decision":"The frontend component stack is React."}),
            &[
                "evidence-observation".to_string(),
                "evidence-observation-2".to_string(),
                "evidence-observation-3".to_string(),
            ],
            None,
            Some("stable"),
            0.8,
            now,
        )
        .expect("observation contradiction should compile");
        tx.commit()
            .expect("contradiction transaction should commit");

        let (contradiction_count, weakening_freshness, weakened_confidence): (i64, String, f32) =
            conn.query_row(
                "
                SELECT contradiction_count, freshness, confidence
                FROM observations
                WHERE id = 'observation-decision-project-storage'
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("contradicted observation should be queryable");
        assert_eq!(contradiction_count, 1);
        assert_eq!(weakening_freshness, "weakening");
        assert_eq!(weakened_confidence, 0.8);

        let mut event_types: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "
                    SELECT event_type
                    FROM observation_events
                    WHERE observation_id = 'observation-decision-project-storage'
                    ",
                )
                .expect("event query should prepare");
            stmt.query_map([], |row| row.get::<_, String>(0))
                .expect("event query should run")
                .collect::<Result<Vec<_>, _>>()
                .expect("event rows should read")
        };
        event_types.sort();
        assert_eq!(
            event_types,
            vec![
                "contradicted".to_string(),
                "created".to_string(),
                "strengthened".to_string()
            ]
        );
    }

    #[test]
    fn ensure_graph_entity_resolves_entity_alias_to_canonical_entity() {
        let conn = test_conn();
        let now = "2026-04-23T20:00:00+00:00";
        let canonical_entity_id = ensure_graph_entity_without_alias(&conn, "person", "eyasu", now)
            .expect("canonical entity should be inserted");

        conn.execute(
            "
            INSERT INTO graph_entity_aliases (
              id, entity_type, alias_name, canonical_entity_id, created_at, updated_at
            ) VALUES (?1, 'person', 'eyas', ?2, ?3, ?3)
            ",
            params!["alias-1", &canonical_entity_id, now],
        )
        .expect("alias should be inserted");

        let resolved_id = ensure_graph_entity(
            &conn,
            &GraphEntityRef {
                entity_type: "person".to_string(),
                canonical_name: "Eyas".to_string(),
            },
            now,
        )
        .expect("alias should resolve");

        assert_eq!(resolved_id, canonical_entity_id);
        let entity_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM graph_entities", [], |row| row.get(0))
            .expect("entity count should be queryable");
        assert_eq!(entity_count, 1);
    }

    #[test]
    fn graph_compaction_merges_duplicate_alias_relationships() {
        let conn = test_conn();
        let created_at = "2026-04-23T20:00:00+00:00";
        let run_at = "2026-04-23T20:05:00+00:00";

        let person_id = insert_entity(&conn, "person-eyasu", "person", "eyasu", created_at);
        let rust_id = insert_entity(&conn, "lang-rust", "language", "rust", created_at);
        let rustlang_id = insert_entity(&conn, "lang-rustlang", "language", "rustlang", created_at);

        conn.execute(
            "
            INSERT INTO graph_entity_aliases (
              id, entity_type, alias_name, canonical_entity_id, created_at, updated_at
            ) VALUES (?1, 'language', 'rustlang', ?2, ?3, ?3)
            ",
            params!["entity-alias-1", &rust_id, created_at],
        )
        .expect("entity alias should be inserted");
        conn.execute(
            "
            INSERT INTO graph_predicate_aliases (
              id, alias_predicate, canonical_predicate, created_at, updated_at
            ) VALUES (?1, 'likes', 'prefers', ?2, ?2)
            ",
            params!["predicate-alias-1", created_at],
        )
        .expect("predicate alias should be inserted");

        insert_evidence(&conn, "evidence-a", created_at);
        insert_evidence(&conn, "evidence-b", created_at);

        insert_relationship_with_active_version(
            &conn,
            "rel-canonical",
            "eyasu|prefers|rust",
            &person_id,
            "prefers",
            &rust_id,
            "ver-canonical",
            1,
            0.65,
            json!({"source":"canonical","strength":"medium"}),
            &["evidence-a"],
            "2026-04-23T20:01:00+00:00",
        );
        insert_relationship_with_active_version(
            &conn,
            "rel-alias",
            "eyasu|likes|rustlang",
            &person_id,
            "likes",
            &rustlang_id,
            "ver-alias",
            1,
            0.91,
            json!({"source":"alias","strength":"high"}),
            &["evidence-b"],
            "2026-04-23T20:02:00+00:00",
        );

        let counts =
            compact_graph_relationships(&conn, false, run_at).expect("compaction should work");

        assert_eq!(counts.canonicalized_relationships, 1);
        assert_eq!(counts.redirected_relationships, 1);
        assert_eq!(counts.merged_versions_created, 1);
        assert_eq!(counts.compacted_entities, 1);

        let alias_status: String = conn
            .query_row(
                "SELECT status FROM graph_relationships WHERE id = 'rel-alias'",
                [],
                |row| row.get(0),
            )
            .expect("winner status should be queryable");
        assert_eq!(alias_status, "active");

        let canonical_status: String = conn
            .query_row(
                "SELECT status FROM graph_relationships WHERE id = 'rel-canonical'",
                [],
                |row| row.get(0),
            )
            .expect("source status should be queryable");
        assert_eq!(canonical_status, "redirected");

        let (predicate, object_entity_id, active_version_id): (String, String, String) = conn
            .query_row(
                "
                SELECT predicate, object_entity_id, active_version_id
                FROM graph_relationships
                WHERE id = 'rel-alias'
                ",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("winner relationship should be queryable");
        assert_eq!(predicate, "prefers");
        assert_eq!(object_entity_id, rust_id);

        let active_confidence: f32 = conn
            .query_row(
                "SELECT confidence FROM graph_relationship_versions WHERE id = ?1",
                params![&active_version_id],
                |row| row.get(0),
            )
            .expect("merged confidence should be queryable");
        assert_eq!(active_confidence, 0.91);

        let merged_evidence_count: i64 = conn
            .query_row(
                "
                SELECT COUNT(*)
                FROM graph_relationship_evidence_links
                WHERE relationship_version_id = ?1
                ",
                params![&active_version_id],
                |row| row.get(0),
            )
            .expect("merged evidence count should be queryable");
        assert_eq!(merged_evidence_count, 2);

        let compacted_entity_status: String = conn
            .query_row(
                "SELECT status FROM graph_entities WHERE id = ?1",
                params![&rustlang_id],
                |row| row.get(0),
            )
            .expect("compacted entity status should be queryable");
        assert_eq!(compacted_entity_status, "compacted");
    }

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db should open");
        conn.execute_batch(include_str!("../../../db/migrations/0001_init.sql"))
            .expect("base migration should apply");
        conn.execute_batch(include_str!("../../../db/migrations/0002_indexes.sql"))
            .expect("index migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0003_knowledge_graph.sql"
        ))
        .expect("graph migration should apply");
        apply_graph_confidence_migration(&conn).expect("confidence migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0004_graph_confidence.sql"
        ))
        .expect("confidence index migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0005_graph_canonicalization.sql"
        ))
        .expect("graph canonicalization migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0006_retrieval_v2_foundation.sql"
        ))
        .expect("retrieval v2 foundation migration should apply");
        apply_observation_canonical_key_migration(&conn)
            .expect("observation canonical key migration should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0007_observation_canonical_keys.sql"
        ))
        .expect("observation canonical indexes should apply");
        conn.execute_batch(include_str!(
            "../../../db/migrations/0008_observation_events.sql"
        ))
        .expect("observation event migration should apply");
        conn.execute_batch(include_str!("../../../db/migrations/0009_import_jobs.sql"))
            .expect("import job migration should apply");
        conn
    }

    fn insert_entity(
        conn: &Connection,
        id: &str,
        entity_type: &str,
        canonical_name: &str,
        now: &str,
    ) -> String {
        conn.execute(
            "
            INSERT INTO graph_entities (
              id, entity_type, canonical_name, attributes_json, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, '{}', 'active', ?4, ?4)
            ",
            params![id, entity_type, canonical_name, now],
        )
        .expect("entity should insert");
        id.to_string()
    }

    fn insert_evidence(conn: &Connection, id: &str, now: &str) {
        conn.execute(
            "
            INSERT INTO evidence_records (
              id, source_type, source_ref, content_type, content, created_at, ingested_at, checksum
            ) VALUES (?1, 'test', ?2, 'text/plain', 'content', ?3, ?3, ?4)
            ",
            params![id, format!("ref-{}", id), now, format!("checksum-{}", id)],
        )
        .expect("evidence should insert");
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_relationship_with_active_version(
        conn: &Connection,
        relationship_id: &str,
        canonical_key: &str,
        subject_entity_id: &str,
        predicate: &str,
        object_entity_id: &str,
        version_id: &str,
        version_number: i64,
        confidence: f32,
        attributes_json: serde_json::Value,
        evidence_ids: &[&str],
        now: &str,
    ) {
        conn.execute(
            "
            INSERT INTO graph_relationships (
              id, canonical_key, subject_entity_id, predicate, object_entity_id,
              active_version_id, status, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?7)
            ",
            params![
                relationship_id,
                canonical_key,
                subject_entity_id,
                predicate,
                object_entity_id,
                version_id,
                now,
            ],
        )
        .expect("relationship should insert");

        conn.execute(
            "
            INSERT INTO graph_relationship_versions (
              id, relationship_id, version_number, state, confidence, attributes_json,
              supersedes_version_id, valid_from, valid_to, created_at
            ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, NULL, ?6, NULL, ?6)
            ",
            params![
                version_id,
                relationship_id,
                version_number,
                confidence,
                serde_json::to_string(&attributes_json).expect("attributes should encode"),
                now,
            ],
        )
        .expect("version should insert");

        for evidence_id in evidence_ids {
            conn.execute(
                "
                INSERT INTO graph_relationship_evidence_links (
                  id, relationship_version_id, evidence_record_id, created_at
                ) VALUES (?1, ?2, ?3, ?4)
                ",
                params![
                    format!("link-{}-{}", version_id, evidence_id),
                    version_id,
                    evidence_id,
                    now
                ],
            )
            .expect("evidence link should insert");
        }
    }
}
