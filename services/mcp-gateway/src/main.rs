use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashMap, VecDeque},
    env,
    net::SocketAddr,
};

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
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;
use yena_model::{RetrievalScope, RetrievalScopeKind};

mod retrieval_v2;

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
struct GraphRetrieveRequest {
    agent_id: String,
    limit: Option<usize>,
    entity_canonical_name: Option<String>,
    #[serde(default)]
    seed_entities: Vec<String>,
    #[serde(default)]
    predicates: Vec<String>,
    #[serde(default)]
    entity_types: Vec<String>,
    min_confidence: Option<f32>,
    max_hops: Option<usize>,
    rank_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RetrieveV2Request {
    agent_id: String,
    query: String,
    limit: Option<usize>,
    #[serde(default)]
    include_trace: bool,
    scope: Option<RetrieveV2ScopeRequest>,
}

#[derive(Debug, Deserialize)]
struct RetrieveV2ScopeRequest {
    kind: Option<String>,
    repo_path: Option<String>,
    repo_remote: Option<String>,
    branch: Option<String>,
    workspace_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListAuditEventsRequest {
    limit: Option<usize>,
    agent_id: Option<String>,
    request_type: Option<String>,
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
struct GraphEntityProjection {
    entity_type: String,
    canonical_name: String,
}

#[derive(Debug, Serialize)]
struct GraphRelationshipProjection {
    relationship_id: String,
    version_id: String,
    subject: GraphEntityProjection,
    predicate: String,
    object: GraphEntityProjection,
    confidence: f32,
    attributes_json: Value,
    redacted_fields: Vec<String>,
    hop_distance: Option<usize>,
    rank_score: f64,
}

#[derive(Debug, Serialize)]
struct GraphRetrieveResponse {
    agent_id: String,
    returned: usize,
    relationships: Vec<GraphRelationshipProjection>,
}

#[derive(Debug, Serialize)]
struct RetrieveV2Response {
    agent_id: String,
    answer_context: yena_model::MemoryAnswerContract,
}

#[derive(Debug, Serialize)]
struct AuditEventView {
    id: String,
    agent_id: String,
    request_type: String,
    scope_applied: String,
    shared_json: Value,
    redacted_json: Option<Value>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ListAuditEventsResponse {
    returned: usize,
    events: Vec<AuditEventView>,
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

#[derive(Debug, Clone)]
struct GraphRelationshipRow {
    relationship_id: String,
    version_id: String,
    subject_entity_type: String,
    subject_canonical_name: String,
    predicate: String,
    object_entity_type: String,
    object_canonical_name: String,
    confidence: f32,
    attributes_json: String,
    updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphRankBy {
    HopThenRecency,
    Recency,
    HopThenConfidenceThenRecency,
    ConfidenceThenRecency,
}

#[derive(Debug)]
struct RankedGraphRow {
    row: GraphRelationshipRow,
    hop_distance: Option<usize>,
    rank_score: f64,
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
        .route("/v2/retrieve", post(retrieve_v2))
        .route("/v1/graph/retrieve", post(graph_retrieve))
        .route("/v1/audit/events/list", post(list_audit_events))
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
        "yena.connect"
        | "yena.retrieve"
        | "yena.retrieve.v2"
        | "yena.graph.retrieve"
        | "yena.audit.list"
        | "yena.scope.upsert"
        | "yena.policy.redact_keys" => {
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
        "yena.retrieve.v2" => {
            parse_and_execute::<RetrieveV2Request, RetrieveV2Response, _, _>(
                state,
                args,
                |state, payload| async move { retrieve_v2(State(state), Json(payload)).await },
            )
            .await
        }
        "yena.graph.retrieve" => {
            parse_and_execute::<GraphRetrieveRequest, GraphRetrieveResponse, _, _>(
                state,
                args,
                |state, payload| async move { graph_retrieve(State(state), Json(payload)).await },
            )
            .await
        }
        "yena.audit.list" => parse_and_execute::<
            ListAuditEventsRequest,
            ListAuditEventsResponse,
            _,
            _,
        >(state, args, |state, payload| async move {
            list_audit_events(State(state), Json(payload)).await
        })
        .await,
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
            "name": "yena.retrieve.v2",
            "description": "Retrieve governed developer memory using the v2 answer contract, abstention, optional traces, and repo/workspace scope.",
            "inputSchema": {
                "type": "object",
                "required": ["agent_id", "query"],
                "properties": {
                    "agent_id": { "type": "string" },
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50 },
                    "include_trace": { "type": "boolean" },
                    "scope": {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["global", "repo", "workspace", "agent", "source"] },
                            "repo_path": { "type": "string" },
                            "repo_remote": { "type": "string" },
                            "branch": { "type": "string" },
                            "workspace_path": { "type": "string" }
                        }
                    }
                }
            }
        },
        {
            "name": "yena.graph.retrieve",
            "description": "Retrieve scoped graph relationships for an agent.",
            "inputSchema": {
                "type": "object",
                "required": ["agent_id"],
                "properties": {
                    "agent_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 200 },
                    "entity_canonical_name": { "type": "string" },
                    "seed_entities": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "predicates": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "entity_types": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "min_confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                    "max_hops": { "type": "integer", "minimum": 1, "maximum": 4 },
                    "rank_by": {
                        "type": "string",
                        "enum": [
                            "hop_then_recency",
                            "recency",
                            "hop_then_confidence_then_recency",
                            "confidence_then_recency"
                        ]
                    }
                }
            }
        },
        {
            "name": "yena.audit.list",
            "description": "List recent audit events for retrieval/redaction visibility.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 },
                    "agent_id": { "type": "string" },
                    "request_type": { "type": "string" }
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

    insert_audit_event(
        &conn,
        "retrieve",
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

async fn retrieve_v2(
    State(state): State<AppState>,
    Json(payload): Json<RetrieveV2Request>,
) -> Result<Json<RetrieveV2Response>, ApiError> {
    if payload.agent_id.trim().is_empty() {
        return Err(ApiError::bad_request("agent_id is required"));
    }
    if payload.query.trim().is_empty() {
        return Err(ApiError::bad_request("query is required"));
    }

    let limit = payload.limit.unwrap_or(8).clamp(1, 50);
    let conn = open_db(&state.db_path)?;
    let scopes = load_scopes(&conn, &payload.agent_id)?;
    let redaction_keys = load_redaction_keys(&conn)?;
    let scope_names: Vec<String> = scopes.iter().map(|s| s.scope_name.clone()).collect();
    let allowed_memory_types: BTreeSet<String> = scopes
        .iter()
        .flat_map(|s| s.payload.allowed_memory_types.clone())
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .collect();
    let requested_scope = payload
        .scope
        .map(scope_from_request)
        .unwrap_or_else(retrieval_v2::global_scope);

    let answer = retrieval_v2::retrieve(
        &conn,
        retrieval_v2::RetrievalV2Input {
            query: payload.query,
            limit,
            include_trace: payload.include_trace,
            scope: requested_scope,
            allowed_memory_types,
            redaction_keys,
        },
    )
    .map_err(|e| ApiError::internal(format!("retrieval v2 failed: {}", e)))?;

    let shared_summary = json!({
        "should_abstain": answer.should_abstain,
        "abstention_reason": answer.abstention_reason,
        "memory_count": answer.memories.len(),
        "memory_types": answer.memories.iter().map(|m| m.memory_type.clone()).collect::<Vec<_>>(),
        "evidence_refs": answer.memories.iter().flat_map(|m| m.evidence_refs.clone()).collect::<Vec<_>>(),
    });
    let redacted_summary = retrieval_v2::safe_trace_summary(&answer);
    let audit_event_id = insert_audit_event(
        &conn,
        "retrieve_v2",
        &payload.agent_id,
        &scope_names,
        &shared_summary,
        &redacted_summary,
    )?;
    insert_retrieval_trace(
        &conn,
        &audit_event_id,
        &payload.agent_id,
        &answer.query,
        &serde_json::to_value(&answer.scope)
            .map_err(|e| ApiError::internal(format!("failed to encode retrieval scope: {}", e)))?,
        &serde_json::to_value(&answer)
            .map_err(|e| ApiError::internal(format!("failed to encode answer: {}", e)))?,
        &redacted_summary,
    )?;

    Ok(Json(RetrieveV2Response {
        agent_id: payload.agent_id,
        answer_context: answer,
    }))
}

async fn graph_retrieve(
    State(state): State<AppState>,
    Json(payload): Json<GraphRetrieveRequest>,
) -> Result<Json<GraphRetrieveResponse>, ApiError> {
    if payload.agent_id.trim().is_empty() {
        return Err(ApiError::bad_request("agent_id is required"));
    }

    let limit = payload.limit.unwrap_or(20).min(200);
    let max_hops = payload.max_hops.unwrap_or(1).clamp(1, 4);
    let rank_by = parse_graph_rank_by(payload.rank_by.as_deref())?;
    let seed_entities = collect_seed_entities(
        payload.entity_canonical_name.as_deref(),
        &payload.seed_entities,
    );
    let predicate_filters = collect_normalized_tokens(&payload.predicates);
    let entity_type_filters = collect_normalized_tokens(&payload.entity_types);
    if let Some(min_confidence) = payload.min_confidence {
        if !(0.0..=1.0).contains(&min_confidence) {
            return Err(ApiError::bad_request(
                "min_confidence must be between 0.0 and 1.0",
            ));
        }
    }

    let conn = open_db(&state.db_path)?;
    let scopes = load_scopes(&conn, &payload.agent_id)?;
    let redaction_keys = load_redaction_keys(&conn)?;

    let scope_names: Vec<String> = scopes.iter().map(|s| s.scope_name.clone()).collect();
    let allowed_memory_types: BTreeSet<String> = scopes
        .iter()
        .flat_map(|s| s.payload.allowed_memory_types.clone())
        .map(|v| v.trim().to_lowercase())
        .collect();

    if !allowed_memory_types.contains("graph") && !allowed_memory_types.contains("relationship") {
        insert_audit_event(
            &conn,
            "graph_retrieve",
            &payload.agent_id,
            &scope_names,
            &json!({"count": 0, "relationship_ids": []}),
            &json!({"entries": []}),
        )?;

        return Ok(Json(GraphRetrieveResponse {
            agent_id: payload.agent_id,
            returned: 0,
            relationships: vec![],
        }));
    }

    let rows = load_active_graph_relationships(&conn)?;
    let filtered_rows = filter_graph_relationships(
        rows,
        &predicate_filters,
        &entity_type_filters,
        payload.min_confidence,
    );
    let ranked_rows = rank_graph_relationships(filtered_rows, &seed_entities, max_hops, rank_by)?;
    let mut projections = Vec::new();

    for ranked in ranked_rows.into_iter().take(limit) {
        let attributes_value: Value =
            serde_json::from_str(&ranked.row.attributes_json).map_err(|e| {
                ApiError::internal(format!("failed to decode graph attributes_json: {}", e))
            })?;
        let (redacted_attributes, redacted_fields) =
            apply_redaction(attributes_value, &redaction_keys);

        projections.push(GraphRelationshipProjection {
            relationship_id: ranked.row.relationship_id,
            version_id: ranked.row.version_id,
            subject: GraphEntityProjection {
                entity_type: ranked.row.subject_entity_type,
                canonical_name: ranked.row.subject_canonical_name,
            },
            predicate: ranked.row.predicate,
            object: GraphEntityProjection {
                entity_type: ranked.row.object_entity_type,
                canonical_name: ranked.row.object_canonical_name,
            },
            confidence: ranked.row.confidence,
            attributes_json: redacted_attributes,
            redacted_fields,
            hop_distance: ranked.hop_distance,
            rank_score: ranked.rank_score,
        });
    }

    let redaction_summary: Vec<Value> = projections
        .iter()
        .filter(|m| !m.redacted_fields.is_empty())
        .map(|m| {
            json!({
                "relationship_id": m.relationship_id,
                "redacted_fields": m.redacted_fields,
            })
        })
        .collect();

    let shared_summary = json!({
        "count": projections.len(),
        "relationship_ids": projections.iter().map(|r| r.relationship_id.clone()).collect::<Vec<_>>(),
        "seed_entities": seed_entities,
        "max_hops": max_hops,
        "rank_by": graph_rank_by_name(rank_by),
        "predicates": predicate_filters.iter().cloned().collect::<Vec<_>>(),
        "entity_types": entity_type_filters.iter().cloned().collect::<Vec<_>>(),
        "min_confidence": payload.min_confidence,
    });

    insert_audit_event(
        &conn,
        "graph_retrieve",
        &payload.agent_id,
        &scope_names,
        &shared_summary,
        &json!({"entries": redaction_summary}),
    )?;

    Ok(Json(GraphRetrieveResponse {
        agent_id: payload.agent_id,
        returned: projections.len(),
        relationships: projections,
    }))
}

async fn list_audit_events(
    State(state): State<AppState>,
    Json(payload): Json<ListAuditEventsRequest>,
) -> Result<Json<ListAuditEventsResponse>, ApiError> {
    let limit = payload.limit.unwrap_or(50).clamp(1, 500) as i64;

    let conn = open_db(&state.db_path)?;
    let mut stmt = conn
        .prepare(
            "
            SELECT id, agent_id, request_type, scope_applied, shared_json, redacted_json, created_at
            FROM retrieval_audit_events
            WHERE (?1 IS NULL OR agent_id = ?1)
              AND (?2 IS NULL OR request_type = ?2)
            ORDER BY created_at DESC
            LIMIT ?3
            ",
        )
        .map_err(|e| ApiError::internal(format!("failed to prepare audit query: {}", e)))?;

    let agent_id = payload
        .agent_id
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let request_type = payload
        .request_type
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let rows = stmt
        .query_map(params![agent_id, request_type, limit], |r| {
            Ok(AuditEventView {
                id: r.get(0)?,
                agent_id: r.get(1)?,
                request_type: r.get(2)?,
                scope_applied: r.get(3)?,
                shared_json: parse_json_or_raw(&r.get::<_, String>(4)?),
                redacted_json: r
                    .get::<_, Option<String>>(5)?
                    .map(|v| parse_json_or_raw(&v)),
                created_at: r.get(6)?,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to execute audit query: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    let events =
        collected.map_err(|e| ApiError::internal(format!("failed to parse audit rows: {}", e)))?;

    Ok(Json(ListAuditEventsResponse {
        returned: events.len(),
        events,
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

fn scope_from_request(scope: RetrieveV2ScopeRequest) -> RetrievalScope {
    RetrievalScope {
        kind: scope
            .kind
            .as_deref()
            .map(retrieval_v2::parse_scope_kind)
            .unwrap_or(RetrievalScopeKind::Global),
        repo_path: trim_optional(scope.repo_path),
        repo_remote: trim_optional(scope.repo_remote),
        branch: trim_optional(scope.branch),
        workspace_path: trim_optional(scope.workspace_path),
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
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

fn load_active_graph_relationships(
    conn: &Connection,
) -> Result<Vec<GraphRelationshipRow>, ApiError> {
    let query = "
        SELECT
          gr.id,
          grv.id,
          se.entity_type,
          se.canonical_name,
          gr.predicate,
          oe.entity_type,
          oe.canonical_name,
          grv.confidence,
          grv.attributes_json,
          gr.updated_at
        FROM graph_relationships gr
        JOIN graph_relationship_versions grv ON grv.id = gr.active_version_id
        JOIN graph_entities se ON se.id = gr.subject_entity_id
        JOIN graph_entities oe ON oe.id = gr.object_entity_id
        WHERE gr.status = 'active'
        ORDER BY gr.updated_at DESC
    ";

    let mut stmt = conn
        .prepare(query)
        .map_err(|e| ApiError::internal(format!("failed to prepare graph query: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(GraphRelationshipRow {
                relationship_id: row.get(0)?,
                version_id: row.get(1)?,
                subject_entity_type: row.get(2)?,
                subject_canonical_name: row.get(3)?,
                predicate: row.get(4)?,
                object_entity_type: row.get(5)?,
                object_canonical_name: row.get(6)?,
                confidence: row.get(7)?,
                attributes_json: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })
        .map_err(|e| ApiError::internal(format!("failed to execute graph query: {}", e)))?;

    let collected: Result<Vec<_>, _> = rows.collect();
    collected.map_err(|e| ApiError::internal(format!("failed to parse graph rows: {}", e)))
}

fn collect_seed_entities(
    entity_canonical_name: Option<&str>,
    seed_entities: &[String],
) -> Vec<String> {
    let mut dedup = BTreeSet::new();
    if let Some(name) = entity_canonical_name {
        let normalized = name.trim().to_lowercase();
        if !normalized.is_empty() {
            dedup.insert(normalized);
        }
    }
    for seed in seed_entities {
        let normalized = seed.trim().to_lowercase();
        if !normalized.is_empty() {
            dedup.insert(normalized);
        }
    }
    dedup.into_iter().collect()
}

fn collect_normalized_tokens(values: &[String]) -> BTreeSet<String> {
    values
        .iter()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_graph_rank_by(value: Option<&str>) -> Result<GraphRankBy, ApiError> {
    match value.map(|v| v.trim().to_lowercase()) {
        None => Ok(GraphRankBy::HopThenConfidenceThenRecency),
        Some(v) if v.is_empty() => Ok(GraphRankBy::HopThenConfidenceThenRecency),
        Some(v) if v == "hop_then_recency" => Ok(GraphRankBy::HopThenRecency),
        Some(v) if v == "recency" => Ok(GraphRankBy::Recency),
        Some(v) if v == "hop_then_confidence_then_recency" => {
            Ok(GraphRankBy::HopThenConfidenceThenRecency)
        }
        Some(v) if v == "confidence_then_recency" => Ok(GraphRankBy::ConfidenceThenRecency),
        Some(v) => Err(ApiError::bad_request(format!(
            "invalid rank_by '{}'; expected one of: hop_then_recency, recency, hop_then_confidence_then_recency, confidence_then_recency",
            v
        ))),
    }
}

fn graph_rank_by_name(rank_by: GraphRankBy) -> &'static str {
    match rank_by {
        GraphRankBy::HopThenRecency => "hop_then_recency",
        GraphRankBy::Recency => "recency",
        GraphRankBy::HopThenConfidenceThenRecency => "hop_then_confidence_then_recency",
        GraphRankBy::ConfidenceThenRecency => "confidence_then_recency",
    }
}

fn filter_graph_relationships(
    rows: Vec<GraphRelationshipRow>,
    predicate_filters: &BTreeSet<String>,
    entity_type_filters: &BTreeSet<String>,
    min_confidence: Option<f32>,
) -> Vec<GraphRelationshipRow> {
    rows.into_iter()
        .filter(|row| {
            if !predicate_filters.is_empty() && !predicate_filters.contains(&row.predicate) {
                return false;
            }

            if !entity_type_filters.is_empty()
                && !entity_type_filters.contains(&row.subject_entity_type)
                && !entity_type_filters.contains(&row.object_entity_type)
            {
                return false;
            }

            if let Some(min_confidence) = min_confidence {
                if row.confidence < min_confidence {
                    return false;
                }
            }

            true
        })
        .collect()
}

fn rank_graph_relationships(
    rows: Vec<GraphRelationshipRow>,
    seed_entities: &[String],
    max_hops: usize,
    rank_by: GraphRankBy,
) -> Result<Vec<RankedGraphRow>, ApiError> {
    let recency_seconds: Vec<i64> = rows
        .iter()
        .map(|row| {
            DateTime::parse_from_rfc3339(&row.updated_at)
                .map(|dt| dt.timestamp())
                .map_err(|e| {
                    ApiError::internal(format!(
                        "invalid graph updated_at '{}': {}",
                        row.updated_at, e
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut ranked = if seed_entities.is_empty() {
        rows.into_iter()
            .enumerate()
            .map(|(idx, row)| RankedGraphRow {
                rank_score: compute_graph_rank_score(
                    rank_by,
                    None,
                    row.confidence,
                    recency_seconds[idx],
                    max_hops,
                ),
                row,
                hop_distance: None,
            })
            .collect::<Vec<_>>()
    } else {
        let mut adjacency: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, row) in rows.iter().enumerate() {
            adjacency
                .entry(row.subject_canonical_name.clone())
                .or_default()
                .push(idx);
            adjacency
                .entry(row.object_canonical_name.clone())
                .or_default()
                .push(idx);
        }

        let mut queue = VecDeque::new();
        let mut entity_depth: HashMap<String, usize> = HashMap::new();
        for seed in seed_entities {
            entity_depth.insert(seed.clone(), 0);
            queue.push_back(seed.clone());
        }

        let mut relationship_hops: HashMap<usize, usize> = HashMap::new();
        while let Some(entity) = queue.pop_front() {
            let depth = match entity_depth.get(&entity) {
                Some(v) => *v,
                None => continue,
            };
            if depth >= max_hops {
                continue;
            }

            let Some(connected) = adjacency.get(&entity) else {
                continue;
            };

            for rel_idx in connected {
                let hop = depth + 1;
                relationship_hops
                    .entry(*rel_idx)
                    .and_modify(|existing| {
                        if hop < *existing {
                            *existing = hop;
                        }
                    })
                    .or_insert(hop);

                let row = &rows[*rel_idx];
                let neighbor = if row.subject_canonical_name == entity {
                    row.object_canonical_name.clone()
                } else {
                    row.subject_canonical_name.clone()
                };

                let should_visit = match entity_depth.get(&neighbor) {
                    Some(existing_depth) => hop < *existing_depth,
                    None => true,
                };
                if should_visit {
                    entity_depth.insert(neighbor.clone(), hop);
                    queue.push_back(neighbor);
                }
            }
        }

        relationship_hops
            .into_iter()
            .map(|(idx, hop_distance)| RankedGraphRow {
                rank_score: compute_graph_rank_score(
                    rank_by,
                    Some(hop_distance),
                    rows[idx].confidence,
                    recency_seconds[idx],
                    max_hops,
                ),
                row: rows[idx].clone(),
                hop_distance: Some(hop_distance),
            })
            .collect::<Vec<_>>()
    };

    ranked.sort_by(|a, b| compare_ranked_graph_rows(a, b, rank_by));

    Ok(ranked)
}

fn compute_graph_rank_score(
    rank_by: GraphRankBy,
    hop_distance: Option<usize>,
    confidence: f32,
    recency_seconds: i64,
    max_hops: usize,
) -> f64 {
    let hop_score = hop_distance
        .map(|hop| (max_hops.saturating_sub(hop) + 1) as f64)
        .unwrap_or(0.0);
    let confidence_score = confidence as f64;
    let recency_score = recency_seconds as f64;

    match rank_by {
        GraphRankBy::HopThenRecency => (hop_score * 1_000_000_000_000.0) + recency_score,
        GraphRankBy::Recency => (recency_score * 10.0) + confidence_score,
        GraphRankBy::HopThenConfidenceThenRecency => {
            (hop_score * 1_000_000_000_000.0) + (confidence_score * 1_000_000_000.0) + recency_score
        }
        GraphRankBy::ConfidenceThenRecency => {
            (confidence_score * 1_000_000_000_000.0) + recency_score
        }
    }
}

fn compare_ranked_graph_rows(
    a: &RankedGraphRow,
    b: &RankedGraphRow,
    rank_by: GraphRankBy,
) -> Ordering {
    match rank_by {
        GraphRankBy::Recency => compare_recency_then_confidence_then_hop(a, b),
        GraphRankBy::HopThenRecency => compare_hop_then_recency_then_confidence(a, b),
        GraphRankBy::HopThenConfidenceThenRecency => compare_hop_then_confidence_then_recency(a, b),
        GraphRankBy::ConfidenceThenRecency => compare_confidence_then_recency_then_hop(a, b),
    }
}

fn compare_hop_then_recency(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    let a_hop = a.hop_distance.unwrap_or(usize::MAX);
    let b_hop = b.hop_distance.unwrap_or(usize::MAX);
    a_hop
        .cmp(&b_hop)
        .then_with(|| compare_updated_at_desc(&a.row.updated_at, &b.row.updated_at))
}

fn compare_recency_then_hop(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    compare_updated_at_desc(&a.row.updated_at, &b.row.updated_at).then_with(|| {
        let a_hop = a.hop_distance.unwrap_or(usize::MAX);
        let b_hop = b.hop_distance.unwrap_or(usize::MAX);
        a_hop.cmp(&b_hop)
    })
}

fn compare_hop_then_recency_then_confidence(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    compare_hop_then_recency(a, b).then_with(|| compare_confidence_desc(a, b))
}

fn compare_recency_then_confidence_then_hop(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    compare_updated_at_desc(&a.row.updated_at, &b.row.updated_at)
        .then_with(|| compare_confidence_desc(a, b))
        .then_with(|| compare_recency_then_hop(a, b))
}

fn compare_hop_then_confidence_then_recency(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    let a_hop = a.hop_distance.unwrap_or(usize::MAX);
    let b_hop = b.hop_distance.unwrap_or(usize::MAX);
    a_hop
        .cmp(&b_hop)
        .then_with(|| compare_confidence_desc(a, b))
        .then_with(|| compare_updated_at_desc(&a.row.updated_at, &b.row.updated_at))
}

fn compare_confidence_then_recency_then_hop(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    compare_confidence_desc(a, b)
        .then_with(|| compare_updated_at_desc(&a.row.updated_at, &b.row.updated_at))
        .then_with(|| compare_recency_then_hop(a, b))
}

fn compare_confidence_desc(a: &RankedGraphRow, b: &RankedGraphRow) -> Ordering {
    b.row
        .confidence
        .partial_cmp(&a.row.confidence)
        .unwrap_or(Ordering::Equal)
}

fn compare_updated_at_desc(a: &str, b: &str) -> Ordering {
    let a_dt = DateTime::parse_from_rfc3339(a)
        .map(|v| v.timestamp())
        .unwrap_or_default();
    let b_dt = DateTime::parse_from_rfc3339(b)
        .map(|v| v.timestamp())
        .unwrap_or_default();
    b_dt.cmp(&a_dt)
}

fn parse_json_or_raw(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| json!({ "raw": raw }))
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

fn insert_audit_event(
    conn: &Connection,
    request_type: &str,
    agent_id: &str,
    scope_names: &[String],
    shared_summary: &Value,
    redacted_summary: &Value,
) -> Result<String, ApiError> {
    let audit_event_id = Uuid::new_v4().to_string();
    conn.execute(
        "
        INSERT INTO retrieval_audit_events (
          id, agent_id, request_type, scope_applied, shared_json, redacted_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ",
        params![
            &audit_event_id,
            agent_id,
            request_type,
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
    .map_err(|e| ApiError::internal(format!("failed to insert audit event: {}", e)))?;

    Ok(audit_event_id)
}

fn insert_retrieval_trace(
    conn: &Connection,
    audit_event_id: &str,
    agent_id: &str,
    query_text: &str,
    scope_json: &Value,
    answer_json: &Value,
    trace_json: &Value,
) -> Result<(), ApiError> {
    conn.execute(
        "
        INSERT INTO retrieval_traces (
          id, audit_event_id, agent_id, query_text, scope_json, answer_json, trace_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ",
        params![
            Uuid::new_v4().to_string(),
            audit_event_id,
            agent_id,
            query_text,
            serde_json::to_string(scope_json).map_err(|e| ApiError::internal(format!(
                "failed to encode trace scope_json: {}",
                e
            )))?,
            serde_json::to_string(answer_json).map_err(|e| ApiError::internal(format!(
                "failed to encode trace answer_json: {}",
                e
            )))?,
            serde_json::to_string(trace_json).map_err(|e| ApiError::internal(format!(
                "failed to encode trace trace_json: {}",
                e
            )))?,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(|e| ApiError::internal(format!("failed to insert retrieval trace: {}", e)))?;

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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;

    use super::{
        apply_redaction, filter_graph_relationships, rank_graph_relationships, GraphRankBy,
        GraphRelationshipRow,
    };

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

    #[test]
    fn graph_filter_respects_predicate_entity_type_and_confidence() {
        let rows = vec![
            graph_row(
                "prefers",
                "person",
                "language",
                0.92,
                "2026-02-16T02:00:00+00:00",
            ),
            graph_row("uses", "person", "tool", 0.55, "2026-02-16T01:00:00+00:00"),
        ];

        let predicates = BTreeSet::from(["prefers".to_string()]);
        let entity_types = BTreeSet::from(["language".to_string()]);
        let filtered = filter_graph_relationships(rows, &predicates, &entity_types, Some(0.8));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].predicate, "prefers");
        assert_eq!(filtered[0].object_entity_type, "language");
    }

    #[test]
    fn confidence_ranking_changes_order_for_same_hop() {
        let rows = vec![
            graph_row(
                "prefers",
                "person",
                "language",
                0.45,
                "2026-02-16T03:00:00+00:00",
            ),
            graph_row(
                "prefers",
                "person",
                "tool",
                0.95,
                "2026-02-16T02:00:00+00:00",
            ),
        ];

        let hop_then_recency = rank_graph_relationships(
            rows.clone(),
            &["eyasu".to_string()],
            1,
            GraphRankBy::HopThenRecency,
        )
        .expect("ranking should succeed");
        assert_eq!(hop_then_recency[0].row.object_entity_type, "language");

        let hop_then_confidence = rank_graph_relationships(
            rows,
            &["eyasu".to_string()],
            1,
            GraphRankBy::HopThenConfidenceThenRecency,
        )
        .expect("ranking should succeed");
        assert_eq!(hop_then_confidence[0].row.object_entity_type, "tool");
    }

    fn graph_row(
        predicate: &str,
        subject_entity_type: &str,
        object_entity_type: &str,
        confidence: f32,
        updated_at: &str,
    ) -> GraphRelationshipRow {
        GraphRelationshipRow {
            relationship_id: format!("rel-{}-{}", predicate, object_entity_type),
            version_id: format!("ver-{}-{}", predicate, object_entity_type),
            subject_entity_type: subject_entity_type.to_string(),
            subject_canonical_name: "eyasu".to_string(),
            predicate: predicate.to_string(),
            object_entity_type: object_entity_type.to_string(),
            object_canonical_name: format!("object-{}", object_entity_type),
            confidence,
            attributes_json: "{}".to_string(),
            updated_at: updated_at.to_string(),
        }
    }
}
