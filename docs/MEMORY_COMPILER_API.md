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

## POST /v1/retention/policies/upsert

Create/update a retention decay policy.

Request:

```json
{
  "policy_name": "project-decay-30d",
  "memory_type": "project",
  "canonical_prefix": "project:",
  "max_age_days": 30,
  "forget_evidence": false,
  "enabled": true
}
```

Response `200`:

```json
{
  "policy_name": "project-decay-30d",
  "memory_type": "project",
  "canonical_prefix": "project:",
  "max_age_days": 30,
  "forget_evidence": false,
  "enabled": true,
  "updated_at": "2026-02-16T02:00:00+00:00"
}
```

## POST /v1/retention/jobs/run

Runs enabled retention policies (or specific policy names).

Request:

```json
{
  "policy_names": ["project-decay-30d"],
  "dry_run": false
}
```

Response `200`:

```json
{
  "run_at": "2026-02-16T02:00:01+00:00",
  "dry_run": false,
  "policies": [
    {
      "policy_name": "project-decay-30d",
      "job_id": "uuid",
      "matched_memory_items": 3,
      "deleted_memory_items": 3,
      "deleted_versions": 5,
      "deleted_links": 5,
      "deleted_evidence": 0,
      "status": "completed"
    }
  ]
}
```

## POST /v1/graph/proposals/relationships

Creates a pending graph relationship proposal.

Request:

```json
{
  "subject": { "entity_type": "person", "canonical_name": "Eyasu" },
  "predicate": "prefers",
  "object": { "entity_type": "language", "canonical_name": "Rust" },
  "attributes_json": { "strength": "high" },
  "evidence_record_ids": ["uuid"],
  "confidence": 0.93
}
```

Response `201`:

```json
{
  "proposal_id": "uuid",
  "status": "pending",
  "created_at": "2026-02-16T02:42:37+00:00"
}
```

## POST /v1/graph/proposals/{id}/commit

Commits a graph relationship proposal into versioned graph state.

Response `200`:

```json
{
  "proposal_id": "uuid",
  "relationship_id": "uuid",
  "version_id": "uuid",
  "superseded_version_id": "uuid-or-null",
  "committed_at": "2026-02-16T02:42:37+00:00"
}
```

## GET /v1/graph/relationships/{id}

Returns active relationship projection.

Response `200`:

```json
{
  "relationship_id": "uuid",
  "subject": { "entity_type": "person", "canonical_name": "eyasu" },
  "predicate": "prefers",
  "object": { "entity_type": "language", "canonical_name": "rust" },
  "active_version_id": "uuid",
  "attributes_json": { "strength": "very_high" },
  "evidence_record_ids": ["uuid"]
}
```

## GET /v1/graph/relationships/{id}/history

Returns relationship version history (active + superseded).

Response `200`:

```json
{
  "relationship_id": "uuid",
  "subject": { "entity_type": "person", "canonical_name": "eyasu" },
  "predicate": "prefers",
  "object": { "entity_type": "language", "canonical_name": "rust" },
  "versions": [
    {
      "version_id": "uuid",
      "version_number": 2,
      "state": "active",
      "attributes_json": { "strength": "very_high" },
      "supersedes_version_id": "uuid",
      "valid_from": "2026-02-16T02:42:37+00:00",
      "valid_to": null,
      "created_at": "2026-02-16T02:42:37+00:00",
      "evidence_record_ids": ["uuid"]
    }
  ]
}
```

## Error Patterns

- `400`: invalid request fields or missing evidence IDs.
- `404`: proposal not found.
- `409`: proposal is not in `pending` state for commit/reject.
- `500`: internal DB/serialization errors.
