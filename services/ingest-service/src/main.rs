use std::{env, net::SocketAddr};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db_path: String,
}

#[derive(Debug, Deserialize)]
struct CreateEvidenceRequest {
    source_type: String,
    source_ref: String,
    content_type: String,
    content: String,
    created_at: Option<String>,
}

impl CreateEvidenceRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.source_type.trim().is_empty() {
            return Err(ApiError::bad_request("source_type is required"));
        }
        if self.source_ref.trim().is_empty() {
            return Err(ApiError::bad_request("source_ref is required"));
        }
        if self.content_type.trim().is_empty() {
            return Err(ApiError::bad_request("content_type is required"));
        }
        if self.content.trim().is_empty() {
            return Err(ApiError::bad_request("content is required"));
        }
        if self.source_type.len() > 128 {
            return Err(ApiError::bad_request("source_type exceeds 128 chars"));
        }
        if self.source_ref.len() > 512 {
            return Err(ApiError::bad_request("source_ref exceeds 512 chars"));
        }
        if self.content_type.len() > 128 {
            return Err(ApiError::bad_request("content_type exceeds 128 chars"));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CreateEvidenceResponse {
    id: String,
    checksum: String,
    created_at: String,
    ingested_at: String,
    was_duplicate: bool,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ingest_service=info,axum=info".into()),
        )
        .init();

    let db_path = env::var("YENA_DB_PATH").unwrap_or_else(|_| "data/yena.db".to_string());
    ensure_data_dir(&db_path)?;
    init_db(&db_path)?;

    let bind = env::var("YENA_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_string());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid YENA_BIND address: {}", bind))?;

    let state = AppState { db_path };
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/evidence", post(create_evidence))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("ingest-service listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn create_evidence(
    State(state): State<AppState>,
    Json(payload): Json<CreateEvidenceRequest>,
) -> Result<(StatusCode, Json<CreateEvidenceResponse>), ApiError> {
    payload.validate()?;

    let created_at = match payload.created_at.as_deref() {
        Some(v) => DateTime::parse_from_rfc3339(v)
            .map_err(|_| ApiError::bad_request("created_at must be RFC3339"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };

    let ingested_at = Utc::now();
    let checksum = checksum_hex(payload.content.as_bytes());

    let conn = Connection::open(&state.db_path)
        .map_err(|e| ApiError::internal(format!("failed to open db: {}", e)))?;

    let duplicate = find_duplicate(&conn, &payload.source_type, &payload.source_ref, &checksum)
        .map_err(|e| ApiError::internal(format!("failed duplicate lookup: {}", e)))?;

    if let Some((id, ingested_at_existing)) = duplicate {
        return Ok((
            StatusCode::OK,
            Json(CreateEvidenceResponse {
                id,
                checksum,
                created_at: created_at.to_rfc3339(),
                ingested_at: ingested_at_existing,
                was_duplicate: true,
            }),
        ));
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO evidence_records (
          id, source_type, source_ref, content_type, content,
          created_at, ingested_at, checksum
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            &id,
            payload.source_type,
            payload.source_ref,
            payload.content_type,
            payload.content,
            created_at.to_rfc3339(),
            ingested_at.to_rfc3339(),
            &checksum,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert evidence: {}", e)))?;

    Ok((
        StatusCode::CREATED,
        Json(CreateEvidenceResponse {
            id,
            checksum,
            created_at: created_at.to_rfc3339(),
            ingested_at: ingested_at.to_rfc3339(),
            was_duplicate: false,
        }),
    ))
}

fn find_duplicate(
    conn: &Connection,
    source_type: &str,
    source_ref: &str,
    checksum: &str,
) -> rusqlite::Result<Option<(String, String)>> {
    let mut stmt = conn.prepare(
        "
        SELECT id, ingested_at
        FROM evidence_records
        WHERE source_type = ?1
          AND source_ref = ?2
          AND checksum = ?3
        LIMIT 1
        ",
    )?;

    let mut rows = stmt.query(params![source_type, source_ref, checksum])?;
    if let Some(row) = rows.next()? {
        return Ok(Some((row.get(0)?, row.get(1)?)));
    }

    Ok(None)
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

fn checksum_hex(input: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::checksum_hex;

    #[test]
    fn checksum_is_stable() {
        let a = checksum_hex(b"hello");
        let b = checksum_hex(b"hello");
        let c = checksum_hex(b"world");

        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
