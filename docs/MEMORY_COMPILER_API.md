# Memory Compiler API (v1)

Base URL: `http://127.0.0.1:8081`

## GET /health

Response:

```json
{
  "status": "ok"
}
```

## POST /v1/proposals

Create a pending `MemoryProposal`.

Request:

```json
{
  "proposal_type": "fact_update",
  "subject_key": "pref:cli_language",
  "memory_type": "preference",
  "value_json": { "value": "Rust" },
  "evidence_record_ids": ["uuid"],
  "confidence": 0.92
}
```

Response `201`:

```json
{
  "id": "uuid",
  "status": "pending",
  "created_at": "2026-02-16T01:00:00+00:00"
}
```

## POST /v1/proposals/{id}/commit

Commits a pending proposal into canonical memory state.

Behavior:

- Creates memory item if missing.
- Creates a new active memory version.
- Supersedes previous active version when present.
- Links provided evidence records.
- Marks proposal as `committed`.

Response `200`:

```json
{
  "proposal_id": "uuid",
  "memory_item_id": "uuid",
  "version_id": "uuid",
  "superseded_version_id": "uuid-or-null",
  "committed_at": "2026-02-16T01:00:01+00:00"
}
```

## POST /v1/proposals/{id}/reject

Rejects a pending proposal.

Response `200`:

```json
{
  "proposal_id": "uuid",
  "status": "rejected",
  "resolved_at": "2026-02-16T01:00:02+00:00"
}
```

## Error Patterns

- `400`: invalid request fields or missing evidence IDs.
- `404`: proposal not found.
- `409`: proposal is not in `pending` state for commit/reject.
- `500`: internal DB/serialization errors.
