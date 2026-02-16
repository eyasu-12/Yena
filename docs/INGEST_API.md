# Ingest Service API (v1)

Base URL: `http://127.0.0.1:8080`

## GET /health

Response:

```json
{
  "status": "ok"
}
```

## POST /v1/evidence

Request body:

```json
{
  "source_type": "agent_activity",
  "source_ref": "session-001",
  "content_type": "decision",
  "content": "User prefers Rust for CLI tools",
  "created_at": "2026-02-16T12:00:00Z"
}
```

- `created_at` is optional (RFC3339 when provided).
- Duplicate records are detected using `(source_type, source_ref, checksum)`.

### Success (new insert) - 201

```json
{
  "id": "uuid",
  "checksum": "sha256-hex",
  "created_at": "2026-02-16T12:00:00+00:00",
  "ingested_at": "2026-02-16T12:00:01+00:00",
  "was_duplicate": false
}
```

### Success (duplicate) - 200

```json
{
  "id": "existing-uuid",
  "checksum": "sha256-hex",
  "created_at": "2026-02-16T12:00:00+00:00",
  "ingested_at": "2026-02-16T12:00:01+00:00",
  "was_duplicate": true
}
```

### Validation error - 400

```json
{
  "error": "source_type is required"
}
```
