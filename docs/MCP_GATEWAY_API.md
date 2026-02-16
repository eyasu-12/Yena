# MCP Gateway API (v1)

Base URL: `http://127.0.0.1:8082`

## GET /health

Response:

```json
{ "status": "ok" }
```

## POST /mcp (JSON-RPC)

Protocol endpoint for MCP-style clients.

Supported methods:

- `initialize`
- `tools/list`
- `tools/call`

Supported tools for `tools/call`:

- `yena.connect`
- `yena.retrieve`
- `yena.scope.upsert`
- `yena.policy.redact_keys`

Example request (`tools/call`):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "yena.retrieve",
    "arguments": {
      "agent_id": "coding-agent",
      "limit": 10,
      "canonical_prefix": "pref:"
    }
  }
}
```

## POST /v1/scopes/upsert

Upsert an agent scope profile.

Request:

```json
{
  "agent_id": "coding-agent",
  "scope_name": "project-context",
  "allowed_memory_types": ["preference", "project"]
}
```

## POST /v1/policies/redact-keys

Configure top-level JSON keys to redact at projection time.

Request:

```json
{
  "keys": ["email", "phone"]
}
```

## POST /v1/connect

Agent handshake endpoint.

Request:

```json
{
  "agent_id": "coding-agent"
}
```

Response:

```json
{
  "connected": true,
  "agent_id": "coding-agent",
  "privacy_mode": "strict",
  "scopes": ["project-context"],
  "accessible_memory_types": ["preference"]
}
```

## POST /v1/retrieve

Retrieve active memory projections with scope filtering and redaction.

Request:

```json
{
  "agent_id": "coding-agent",
  "limit": 10,
  "canonical_prefix": "pref:"
}
```

Response:

```json
{
  "agent_id": "coding-agent",
  "returned": 1,
  "memories": [
    {
      "memory_item_id": "uuid",
      "version_id": "uuid",
      "canonical_key": "pref:cli_language",
      "memory_type": "preference",
      "value_json": { "value": "Rust" },
      "redacted_fields": ["email"]
    }
  ]
}
```

Every retrieve call writes a `retrieval_audit_events` row describing scope, shared IDs, and redactions.
