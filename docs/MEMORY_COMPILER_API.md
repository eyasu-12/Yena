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

## GET /v1/memory/{canonical_key}

Returns the active memory version and linked evidence IDs.

Response `200`:

```json
{
  "memory_item_id": "uuid",
  "canonical_key": "pref:cli_language",
  "memory_type": "preference",
  "active_version_id": "uuid",
  "value_json": { "value": "Rust" },
  "evidence_record_ids": ["uuid"]
}
```

## GET /v1/memory/{canonical_key}/history

Returns full version history for a memory item.

Response `200`:

```json
{
  "memory_item_id": "uuid",
  "canonical_key": "pref:cli_language",
  "memory_type": "preference",
  "versions": [
    {
      "version_id": "uuid",
      "version_number": 2,
      "state": "active",
      "value_json": { "value": "Rust" },
      "supersedes_version_id": "uuid",
      "valid_from": "2026-02-16T01:00:01+00:00",
      "valid_to": null,
      "created_at": "2026-02-16T01:00:01+00:00",
      "evidence_record_ids": ["uuid"]
    }
  ]
}
```

## POST /v1/memory/forget

Hard-removes a memory item and its versions. By default, unreferenced linked evidence records are deleted too.

Request:

```json
{
  "canonical_key": "pref:cli_language",
  "forget_evidence": true
}
```

Response `200`:

```json
{
  "canonical_key": "pref:cli_language",
  "deleted_memory_item_id": "uuid",
  "deleted_versions": 2,
  "deleted_links": 2,
  "deleted_evidence": 1
}
```

## Error Patterns

- `400`: invalid request fields or missing evidence IDs.
- `404`: proposal not found.
- `409`: proposal is not in `pending` state for commit/reject.
- `500`: internal DB/serialization errors.
