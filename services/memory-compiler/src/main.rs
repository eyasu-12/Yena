use std::{collections::BTreeSet, env, net::SocketAddr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    attributes_json: Value,
    supersedes_version_id: Option<String>,
    valid_from: String,
    valid_to: Option<String>,
    created_at: String,
    evidence_record_ids: Vec<String>,
}

#[derive(Debug, Default)]
struct DeleteCounts {
    deleted_versions: usize,
    deleted_links: usize,
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
        .route("/v1/proposals", post(create_proposal))
        .route("/v1/proposals/{id}/commit", post(commit_proposal))
        .route("/v1/proposals/{id}/reject", post(reject_proposal))
        .route("/v1/memory/{canonical_key}", get(get_memory))
        .route(
            "/v1/memory/{canonical_key}/history",
            get(get_memory_history),
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

    for evidence_id in &payload.evidence_record_ids {
        ensure_evidence_exists(&tx, evidence_id)?;
    }

    let committed_at = Utc::now().to_rfc3339();
    let (memory_item_id, old_active_version_id) = ensure_memory_item(
        &tx,
        &payload.memory_type,
        &proposal.subject_key,
        &committed_at,
    )?;

    let version_number = next_version_number(&tx, &memory_item_id)?;
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
            serde_json::to_string(&payload.value_json)
                .map_err(|e| ApiError::internal(format!("failed value_json encode: {}", e)))?,
            old_active_version_id,
            committed_at,
            committed_at,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert memory version: {}", e)))?;

    for evidence_id in &payload.evidence_record_ids {
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
        memory_item_id,
        version_id: new_version_id,
        superseded_version_id: old_active_version_id,
        committed_at,
    }))
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
    let subject_key = format!(
        "{}|{}|{}",
        normalize_token(&payload.subject.canonical_name),
        normalize_token(&payload.predicate),
        normalize_token(&payload.object.canonical_name)
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

    let conn = open_db(&state.db_path)?;
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

    let canonical_key = format!(
        "{}|{}|{}",
        normalize_token(&payload.subject.canonical_name),
        normalize_token(&payload.predicate),
        normalize_token(&payload.object.canonical_name)
    );

    let (relationship_id, old_active_version_id) = ensure_graph_relationship(
        &tx,
        &canonical_key,
        &subject_entity_id,
        &payload.predicate,
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
          id, relationship_id, version_number, state, attributes_json,
          supersedes_version_id, valid_from, valid_to, created_at
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, NULL, ?6)
        ",
        params![
            &new_version_id,
            &relationship_id,
            version_number,
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

    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    )> = conn
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
            SELECT id, version_number, state, attributes_json, supersedes_version_id, valid_from, valid_to, created_at
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
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
            ))
        })
        .map_err(|e| ApiError::internal(format!("failed to execute graph history query: {}", e)))?;

    let mut versions = Vec::new();
    for row in rows {
        let (
            version_id,
            version_number,
            state,
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
        SELECT id, proposal_type, subject_key, payload_json, status
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
                status: row.get(4)?,
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

    let mut deleted_evidence = 0usize;
    if forget_evidence {
        for evidence_id in linked_evidence_ids {
            let still_linked: Option<String> = conn
                .query_row(
                    "
                    SELECT evidence_record_id
                    FROM memory_links
                    WHERE evidence_record_id = ?1
                    LIMIT 1
                    ",
                    params![&evidence_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| {
                    ApiError::internal(format!("failed evidence linkage lookup: {}", e))
                })?;

            if still_linked.is_none() {
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
        deleted_evidence,
    })
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CreateProposalRequest;
    use serde_json::json;

    #[test]
    fn confidence_validation() {
        let req = CreateProposalRequest {
            proposal_type: "fact_update".to_string(),
            subject_key: "pref:language".to_string(),
            memory_type: "preference".to_string(),
            value_json: json!({"value":"Rust"}),
            evidence_record_ids: vec![],
            confidence: 1.1,
        };

        assert!(req.validate().is_err());
    }
}
