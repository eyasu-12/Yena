use std::{collections::BTreeSet, env, net::SocketAddr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
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
            return Err(ApiError::bad_request("confidence must be between 0.0 and 1.0"));
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
        .route("/v1/memory/{canonical_key}/history", get(get_memory_history))
        .route("/v1/memory/forget", post(forget_memory))
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
    let (memory_item_id, old_active_version_id) = ensure_memory_item(&tx, &payload.memory_type, &proposal.subject_key, &committed_at)?;

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

    let memory_item_id = memory_item_id.ok_or_else(|| ApiError::not_found("memory item not found"))?;

    let version_ids = load_version_ids_for_item(&tx, &memory_item_id)?;
    let linked_evidence_ids = load_evidence_ids_for_versions(&tx, &version_ids)?;

    let deleted_links = tx
        .execute(
            "
            DELETE FROM memory_links
            WHERE memory_item_version_id IN (
              SELECT id FROM memory_item_versions WHERE memory_item_id = ?1
            )
            ",
            params![&memory_item_id],
        )
        .map_err(|e| ApiError::internal(format!("failed to delete memory links: {}", e)))?;

    let deleted_versions = tx
        .execute(
            "DELETE FROM memory_item_versions WHERE memory_item_id = ?1",
            params![&memory_item_id],
        )
        .map_err(|e| ApiError::internal(format!("failed to delete memory versions: {}", e)))?;

    tx.execute("DELETE FROM memory_items WHERE id = ?1", params![&memory_item_id])
        .map_err(|e| ApiError::internal(format!("failed to delete memory item: {}", e)))?;

    let mut deleted_evidence = 0usize;
    if payload.forget_evidence {
        for evidence_id in linked_evidence_ids {
            let still_linked: Option<String> = tx
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
                .map_err(|e| ApiError::internal(format!("failed evidence linkage lookup: {}", e)))?;

            if still_linked.is_none() {
                deleted_evidence += tx
                    .execute(
                        "DELETE FROM evidence_records WHERE id = ?1",
                        params![&evidence_id],
                    )
                    .map_err(|e| ApiError::internal(format!("failed to delete evidence record: {}", e)))?;
            }
        }
    }

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(ForgetMemoryResponse {
        canonical_key: payload.canonical_key,
        deleted_memory_item_id: memory_item_id,
        deleted_versions,
        deleted_links,
        deleted_evidence,
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
        SELECT id, subject_key, payload_json, status
        FROM memory_proposals
        WHERE id = ?1
        ",
        params![id],
        |row| {
            Ok(ProposalRow {
                id: row.get(0)?,
                subject_key: row.get(1)?,
                payload_json: row.get(2)?,
                status: row.get(3)?,
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

fn load_version_ids_for_item(conn: &Connection, memory_item_id: &str) -> Result<Vec<String>, ApiError> {
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
