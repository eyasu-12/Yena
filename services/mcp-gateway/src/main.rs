use std::{collections::BTreeSet, env, net::SocketAddr};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    db_path: String,
}

#[derive(Debug, Deserialize)]
struct UpsertScopeRequest {
    agent_id: String,
    scope_name: String,
    #[serde(default)]
    allowed_memory_types: Vec<String>,
}

impl UpsertScopeRequest {
    fn validate(&self) -> Result<(), ApiError> {
        if self.agent_id.trim().is_empty() {
            return Err(ApiError::bad_request("agent_id is required"));
        }
        if self.scope_name.trim().is_empty() {
            return Err(ApiError::bad_request("scope_name is required"));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct ConnectRequest {
    agent_id: String,
}

#[derive(Debug, Deserialize)]
struct RetrieveRequest {
    agent_id: String,
    limit: Option<usize>,
    canonical_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpsertRedactPolicyRequest {
    keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default = "default_json_value")]
    id: Value,
    method: String,
    #[serde(default = "default_json_value")]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct ToolCallRequest {
    name: String,
    #[serde(default = "default_json_value")]
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct UpsertScopeResponse {
    agent_id: String,
    scope_name: String,
    allowed_memory_types: Vec<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct ConnectResponse {
    connected: bool,
    agent_id: String,
    privacy_mode: &'static str,
    scopes: Vec<String>,
    accessible_memory_types: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemoryProjection {
    memory_item_id: String,
    version_id: String,
    canonical_key: String,
    memory_type: String,
    value_json: Value,
    redacted_fields: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RetrieveResponse {
    agent_id: String,
    returned: usize,
    memories: Vec<MemoryProjection>,
}

#[derive(Debug, Serialize)]
struct PolicyResponse {
    policy_name: &'static str,
    keys: Vec<String>,
    updated_at: String,
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

#[derive(Debug, Deserialize, Serialize)]
struct ScopePayload {
    #[serde(default)]
    allowed_memory_types: Vec<String>,
}

#[derive(Debug)]
struct ScopeRow {
    scope_name: String,
    payload: ScopePayload,
}

#[derive(Debug)]
struct MemoryRow {
    memory_item_id: String,
    version_id: String,
    canonical_key: String,
    memory_type: String,
    value_json: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mcp_gateway=info,axum=info".into()),
        )
        .init();

    let db_path = env::var("YENA_DB_PATH").unwrap_or_else(|_| "data/yena.db".to_string());
    ensure_data_dir(&db_path)?;
    init_db(&db_path)?;

    let bind = env::var("YENA_BIND").unwrap_or_else(|_| "127.0.0.1:8082".to_string());
    let addr: SocketAddr = bind
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid YENA_BIND address: {}", bind))?;

    let state = AppState { db_path };
    let app = Router::new()
        .route("/health", get(health))
        .route("/mcp", post(mcp_rpc))
        .route("/v1/scopes/upsert", post(upsert_scope))
        .route("/v1/policies/redact-keys", post(upsert_redact_policy))
        .route("/v1/connect", post(connect))
        .route("/v1/retrieve", post(retrieve))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("mcp-gateway listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn mcp_rpc(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<Value> {
    if request.jsonrpc != "2.0" {
        return Json(rpc_error(
            request.id,
            -32600,
            "invalid request: jsonrpc must be '2.0'",
            None,
        ));
    }

    let response = match request.method.as_str() {
        "initialize" => rpc_ok(
            request.id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "yena-mcp-gateway",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }),
        ),
        "tools/list" => rpc_ok(
            request.id,
            json!({
                "tools": mcp_tools_catalog()
            }),
        ),
        "tools/call" => match serde_json::from_value::<ToolCallRequest>(request.params) {
            Ok(call) => execute_tool_call(state, request.id, &call.name, call.arguments).await,
            Err(e) => rpc_error(
                request.id,
                -32602,
                format!("invalid params for tools/call: {}", e),
                None,
            ),
        },
        // Direct calls are useful for development/testing and mirror tool names.
        "yena.connect" | "yena.retrieve" | "yena.scope.upsert" | "yena.policy.redact_keys" => {
            execute_tool_call(state, request.id, &request.method, request.params).await
        }
        _ => rpc_error(
            request.id,
            -32601,
            format!("method not found: {}", request.method),
            None,
        ),
    };

    Json(response)
}

async fn execute_tool_call(state: AppState, id: Value, name: &str, args: Value) -> Value {
    let result = match name {
        "yena.connect" => {
            parse_and_execute::<ConnectRequest, ConnectResponse, _, _>(
                state,
                args,
                |state, payload| async move { connect(State(state), Json(payload)).await },
            )
            .await
        }
        "yena.retrieve" => {
            parse_and_execute::<RetrieveRequest, RetrieveResponse, _, _>(
                state,
                args,
                |state, payload| async move { retrieve(State(state), Json(payload)).await },
            )
            .await
        }
        "yena.scope.upsert" => {
            parse_and_execute::<UpsertScopeRequest, UpsertScopeResponse, _, _>(
                state,
                args,
                |state, payload| async move { upsert_scope(State(state), Json(payload)).await },
            )
            .await
        }
        "yena.policy.redact_keys" => parse_and_execute::<
            UpsertRedactPolicyRequest,
            PolicyResponse,
            _,
            _,
        >(state, args, |state, payload| async move {
            upsert_redact_policy(State(state), Json(payload)).await
        })
        .await,
        _ => Err(ApiError::bad_request(format!("unknown tool: {}", name))),
    };

    match result {
        Ok(structured) => rpc_ok(
            id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": structured.to_string()
                    }
                ],
                "structuredContent": structured
            }),
        ),
        Err(err) => rpc_error(
            id,
            -32000,
            err.message,
            Some(json!({"http_status": err.status.as_u16()})),
        ),
    }
}

async fn parse_and_execute<Req, Resp, F, Fut>(
    state: AppState,
    args: Value,
    f: F,
) -> Result<Value, ApiError>
where
    Req: for<'de> Deserialize<'de>,
    Resp: Serialize,
    F: FnOnce(AppState, Req) -> Fut,
    Fut: std::future::Future<Output = Result<Json<Resp>, ApiError>>,
{
    let payload: Req = serde_json::from_value(args)
        .map_err(|e| ApiError::bad_request(format!("invalid tool arguments: {}", e)))?;
    let Json(response) = f(state, payload).await?;
    serde_json::to_value(response)
        .map_err(|e| ApiError::internal(format!("failed to serialize tool response: {}", e)))
}

fn mcp_tools_catalog() -> Value {
    json!([
        {
            "name": "yena.connect",
            "description": "Handshake for an agent and discover accessible scope/memory types.",
            "inputSchema": {
                "type": "object",
                "required": ["agent_id"],
                "properties": {
                    "agent_id": { "type": "string" }
                }
            }
        },
        {
            "name": "yena.retrieve",
            "description": "Retrieve policy-filtered active memories for an agent.",
            "inputSchema": {
                "type": "object",
                "required": ["agent_id"],
                "properties": {
                    "agent_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "canonical_prefix": { "type": "string" }
                }
            }
        },
        {
            "name": "yena.scope.upsert",
            "description": "Create or update an agent scope profile.",
            "inputSchema": {
                "type": "object",
                "required": ["agent_id", "scope_name"],
                "properties": {
                    "agent_id": { "type": "string" },
                    "scope_name": { "type": "string" },
                    "allowed_memory_types": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }
        },
        {
            "name": "yena.policy.redact_keys",
            "description": "Configure top-level JSON keys to redact in retrieval responses.",
            "inputSchema": {
                "type": "object",
                "required": ["keys"],
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }
        }
    ])
}

fn default_json_value() -> Value {
    Value::Null
}

fn rpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>, data: Option<Value>) -> Value {
    let mut error = json!({
        "code": code,
        "message": message.into(),
    });
    if let Some(data_value) = data {
        error["data"] = data_value;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error
    })
}

async fn upsert_scope(
    State(state): State<AppState>,
    Json(payload): Json<UpsertScopeRequest>,
) -> Result<Json<UpsertScopeResponse>, ApiError> {
    payload.validate()?;

    let now = Utc::now().to_rfc3339();
    let scope_payload = ScopePayload {
        allowed_memory_types: payload
            .allowed_memory_types
            .into_iter()
            .filter(|v| !v.trim().is_empty())
            .collect(),
    };

    let scope_json = serde_json::to_string(&scope_payload)
        .map_err(|e| ApiError::internal(format!("failed to encode scope_json: {}", e)))?;

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    tx.execute(
        "DELETE FROM agent_scopes WHERE agent_id = ?1 AND scope_name = ?2",
        params![payload.agent_id, payload.scope_name],
    )
    .map_err(|e| ApiError::internal(format!("failed to clear previous scope: {}", e)))?;

    tx.execute(
        "
        INSERT INTO agent_scopes (
          id, agent_id, scope_name, scope_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ",
        params![
            Uuid::new_v4().to_string(),
            payload.agent_id,
            payload.scope_name,
            scope_json,
            now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert scope: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(UpsertScopeResponse {
        agent_id: payload.agent_id,
        scope_name: payload.scope_name,
        allowed_memory_types: scope_payload.allowed_memory_types,
        updated_at: now,
    }))
}

async fn upsert_redact_policy(
    State(state): State<AppState>,
    Json(payload): Json<UpsertRedactPolicyRequest>,
) -> Result<Json<PolicyResponse>, ApiError> {
    let keys: Vec<String> = payload
        .keys
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();

    let now = Utc::now().to_rfc3339();
    let rule_json = json!({ "keys": keys });

    let mut conn = open_db(&state.db_path)?;
    let tx = conn
        .transaction()
        .map_err(|e| ApiError::internal(format!("failed to start tx: {}", e)))?;

    tx.execute(
        "DELETE FROM policy_rules WHERE rule_name = 'redact_keys'",
        [],
    )
    .map_err(|e| ApiError::internal(format!("failed to clear existing redact policy: {}", e)))?;

    tx.execute(
        "
        INSERT INTO policy_rules (
          id, rule_name, rule_json, enabled, created_at, updated_at
        ) VALUES (?1, 'redact_keys', ?2, 1, ?3, ?3)
        ",
        params![
            Uuid::new_v4().to_string(),
            serde_json::to_string(&rule_json)
                .map_err(|e| ApiError::internal(format!("failed to encode policy json: {}", e)))?,
            now,
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert policy: {}", e)))?;

    tx.commit()
        .map_err(|e| ApiError::internal(format!("failed to commit tx: {}", e)))?;

    Ok(Json(PolicyResponse {
        policy_name: "redact_keys",
        keys: rule_json
            .get("keys")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        updated_at: now,
    }))
}

async fn connect(
    State(state): State<AppState>,
    Json(payload): Json<ConnectRequest>,
) -> Result<Json<ConnectResponse>, ApiError> {
    if payload.agent_id.trim().is_empty() {
        return Err(ApiError::bad_request("agent_id is required"));
    }

    let conn = open_db(&state.db_path)?;
    let scopes = load_scopes(&conn, &payload.agent_id)?;

    let mut memory_types = BTreeSet::new();
    let mut scope_names = Vec::with_capacity(scopes.len());
    for scope in &scopes {
        scope_names.push(scope.scope_name.clone());
        for t in &scope.payload.allowed_memory_types {
            memory_types.insert(t.clone());
        }
    }

    Ok(Json(ConnectResponse {
        connected: true,
        agent_id: payload.agent_id,
        privacy_mode: "strict",
        scopes: scope_names,
        accessible_memory_types: memory_types.into_iter().collect(),
    }))
}

async fn retrieve(
    State(state): State<AppState>,
    Json(payload): Json<RetrieveRequest>,
) -> Result<Json<RetrieveResponse>, ApiError> {
    if payload.agent_id.trim().is_empty() {
        return Err(ApiError::bad_request("agent_id is required"));
    }

    let limit = payload.limit.unwrap_or(20).min(200);

    let conn = open_db(&state.db_path)?;
    let scopes = load_scopes(&conn, &payload.agent_id)?;
    let redaction_keys = load_redaction_keys(&conn)?;

    let scope_names: Vec<String> = scopes.iter().map(|s| s.scope_name.clone()).collect();
    let allowed_memory_types: BTreeSet<String> = scopes
        .iter()
        .flat_map(|s| s.payload.allowed_memory_types.clone())
        .collect();

    let rows = load_active_memories(&conn)?;
    let mut projections = Vec::new();

    for row in rows {
        if !allowed_memory_types.contains(&row.memory_type) {
            continue;
        }

        if let Some(prefix) = &payload.canonical_prefix {
            if !row.canonical_key.starts_with(prefix) {
                continue;
            }
        }

        let value: Value = serde_json::from_str(&row.value_json).map_err(|e| {
            ApiError::internal(format!("failed to decode memory value_json: {}", e))
        })?;
        let (redacted_value, redacted_fields) = apply_redaction(value, &redaction_keys);

        projections.push(MemoryProjection {
            memory_item_id: row.memory_item_id,
            version_id: row.version_id,
            canonical_key: row.canonical_key,
            memory_type: row.memory_type,
            value_json: redacted_value,
            redacted_fields,
        });

        if projections.len() >= limit {
            break;
        }
    }

    let redaction_summary: Vec<Value> = projections
        .iter()
        .filter(|m| !m.redacted_fields.is_empty())
        .map(|m| {
            json!({
                "memory_item_id": m.memory_item_id,
                "redacted_fields": m.redacted_fields,
            })
        })
        .collect();

    let shared_summary = json!({
        "count": projections.len(),
        "memory_item_ids": projections.iter().map(|m| m.memory_item_id.clone()).collect::<Vec<_>>()
    });

    insert_retrieval_audit(
        &conn,
        &payload.agent_id,
        &scope_names,
        &shared_summary,
        &json!({"entries": redaction_summary}),
    )?;

    Ok(Json(RetrieveResponse {
        agent_id: payload.agent_id,
        returned: projections.len(),
        memories: projections,
    }))
}

fn open_db(db_path: &str) -> Result<Connection, ApiError> {
    let conn = Connection::open(db_path)
        .map_err(|e| ApiError::internal(format!("failed to open db: {}", e)))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| ApiError::internal(format!("failed to enable foreign keys: {}", e)))?;
    Ok(conn)
}

fn load_scopes(conn: &Connection, agent_id: &str) -> Result<Vec<ScopeRow>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT scope_name, scope_json
            FROM agent_scopes
            WHERE agent_id = ?1
            ORDER BY scope_name
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare scope query: {}", e)))?;

    let rows = stmt
        .query_map(params![agent_id], |row| {
            let scope_name: String = row.get(0)?;
            let scope_json: String = row.get(1)?;
            let payload: ScopePayload =
                serde_json::from_str(&scope_json).map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(ScopeRow {
                scope_name,
                payload,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to execute scope query: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to parse scopes: {}", e)))
}

fn load_redaction_keys(conn: &Connection) -> Result<BTreeSet<String>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT rule_json
            FROM policy_rules
            WHERE rule_name = 'redact_keys' AND enabled = 1
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare policy query: {}", e)))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| ApiError::internal(format!("failed to execute policy query: {}", e)))?;

    let mut keys = BTreeSet::new();
    for row in rows {
        let raw =
            row.map_err(|e| ApiError::internal(format!("failed to read policy row: {}", e)))?;
        let v: Value = serde_json::from_str(&raw)
            .map_err(|e| ApiError::internal(format!("failed to decode policy json: {}", e)))?;

        if let Some(arr) = v.get("keys").and_then(|v| v.as_array()) {
            for key in arr {
                if let Some(s) = key.as_str() {
                    keys.insert(s.to_string());
                }
            }
        }
    }

    Ok(keys)
}

fn load_active_memories(conn: &Connection) -> Result<Vec<MemoryRow>, ApiError> {
    let mut stmt = conn
        .prepare(
            "
            SELECT
              mi.id,
              mv.id,
              mi.canonical_key,
              mi.memory_type,
              mv.value_json
            FROM memory_items mi
            JOIN memory_item_versions mv ON mv.id = mi.active_version_id
            WHERE mi.status = 'active'
            ORDER BY mi.updated_at DESC
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare memory query: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(MemoryRow {
                memory_item_id: row.get(0)?,
                version_id: row.get(1)?,
                canonical_key: row.get(2)?,
                memory_type: row.get(3)?,
                value_json: row.get(4)?,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to execute memory query: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to parse memory rows: {}", e)))
}

fn apply_redaction(value: Value, keys: &BTreeSet<String>) -> (Value, Vec<String>) {
    if keys.is_empty() {
        return (value, Vec::new());
    }

    let mut redacted = Vec::new();

    let updated = match value {
        Value::Object(mut map) => {
            for key in keys {
                if map.remove(key).is_some() {
                    redacted.push(key.clone());
                }
            }
            Value::Object(map)
        }
        other => other,
    };

    (updated, redacted)
}

fn insert_retrieval_audit(
    conn: &Connection,
    agent_id: &str,
    scope_names: &[String],
    shared_summary: &Value,
    redacted_summary: &Value,
) -> Result<(), ApiError> {
    conn.execute(
        "
        INSERT INTO retrieval_audit_events (
          id, agent_id, request_type, scope_applied, shared_json, redacted_json, created_at
        ) VALUES (?1, ?2, 'retrieve', ?3, ?4, ?5, ?6)
        ",
        params![
            Uuid::new_v4().to_string(),
            agent_id,
            if scope_names.is_empty() {
                "none".to_string()
            } else {
                scope_names.join(",")
            },
            serde_json::to_string(shared_summary).map_err(|e| ApiError::internal(format!(
                "failed to encode shared summary: {}",
                e
            )))?,
            serde_json::to_string(redacted_summary).map_err(|e| ApiError::internal(format!(
                "failed to encode redacted summary: {}",
                e
            )))?,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert retrieval audit: {}", e)))?;

    Ok(())
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
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::apply_redaction;

    #[test]
    fn redaction_removes_top_level_fields() {
        let mut keys = BTreeSet::new();
        keys.insert("email".to_string());
        keys.insert("phone".to_string());

        let input = json!({"name":"Eyasu","email":"a@b.com","phone":"123"});
        let (out, fields) = apply_redaction(input, &keys);

        assert_eq!(out, json!({"name":"Eyasu"}));
        assert_eq!(fields.len(), 2);
    }
}
