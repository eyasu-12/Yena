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
- `yena.retrieve.v2`
- `yena.graph.retrieve`
- `yena.audit.list`
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

## POST /v2/retrieve

Retrieve governed developer memory using the retrieval v2 answer contract.

This endpoint is the vertical-slice foundation for Yena's core memory engine. It uses a shared retrieval pipeline over active memory items, observations, and graph relationships, then returns either scoped memory answers or a calibrated abstention.

Retrieval v2 blends deterministic term matching with the local SQLite FTS index populated by the memory compiler. FTS hits can surface concise canonical memories when the richer indexed document matches the query.

Request:

```json
{
  "agent_id": "coding-agent",
  "query": "What database did we choose for this repo?",
  "limit": 5,
  "include_trace": true,
  "scope": {
    "kind": "repo",
    "repo_path": "/Users/eyasu/Projects/Yena",
    "repo_remote": "https://github.com/eyasu-12/Yena.git",
    "branch": "main"
  }
}
```

Scope kinds:

- `global`
- `repo`
- `workspace`
- `agent`
- `source`

Response:

```json
{
  "agent_id": "coding-agent",
  "answer_context": {
    "query": "What database did we choose for this repo?",
    "scope": {
      "kind": "repo",
      "repo_path": "/Users/eyasu/Projects/Yena",
      "repo_remote": "https://github.com/eyasu-12/Yena.git",
      "branch": "main"
    },
    "should_abstain": false,
    "memories": [
      {
        "statement": "Yena uses SQLite for local-first storage",
        "memory_type": "project_decision",
        "freshness": "stable",
        "confidence": 0.91,
        "evidence_refs": ["evidence-id"],
        "trace": {
          "candidate_source": "memory_item",
          "candidate_id": "memory-id",
          "matched_terms": ["sqlite"],
          "score_components": {
            "rank_score": 15.91,
            "fts_score": 5,
            "confidence": 0.91,
            "freshness": "stable",
            "evidence_count": 1,
            "lifecycle_event_count": 0,
            "latest_lifecycle_event": null,
            "lifecycle_score_boost": 0.0
          },
          "scope_filter": "repo:/Users/eyasu/Projects/Yena:https://github.com/eyasu-12/Yena.git:main",
          "redactions": [],
          "evidence_refs": ["evidence-id"]
        },
        "redactions": []
      }
    ]
  }
}
```

Abstention response:

```json
{
  "agent_id": "coding-agent",
  "answer_context": {
    "query": "Which auth provider did we choose?",
    "scope": { "kind": "global" },
    "should_abstain": true,
    "abstention_reason": "missing_evidence",
    "abstention_message": "The requested fact is not selected in Yena yet; it remains an open question without supporting evidence.",
    "memories": []
  }
}
```

Abstention reasons:

- `missing_evidence`
- `stale_memory`
- `stale_memory_superseded`
- `contradicted`
- `out_of_scope`
- `low_confidence`

Some abstentions can include supporting memories. For example, stale/superseded decision checks may return the current active memory and its evidence while still setting `should_abstain = true`.

When the selected candidate is a compiled observation, `trace.lifecycle_events` includes a compact redaction-safe lifecycle summary from `observation_events`:

```json
{
  "candidate_source": "observation",
  "candidate_id": "observation-decision-project-architecture-local_first",
  "lifecycle_events": [
    {
      "event_type": "strengthened",
      "created_at": "2026-04-24T12:00:00+00:00",
      "evidence_refs": ["evidence-a", "evidence-b"]
    }
  ]
}
```

Retrieval v2 uses lifecycle events as a trust signal:

- `strengthened` observations receive a small ranking boost.
- `weakened` observations are penalized and abstain as `low_confidence` if they are still the best relevant candidate.
- `contradicted` observations are penalized and abstain as `contradicted` if they remain in the top relevance band.
- Lower-ranked contradicted observations outside the top relevance band do not poison a stronger supported answer.

Every retrieval v2 call writes:

- a `retrieval_audit_events` row with `request_type = retrieve_v2`
- a `retrieval_traces` row linked to the audit event

Trace output is policy-filtered and must not contain redacted raw values.

## POST /v1/graph/retrieve

Retrieve active graph relationships with scope filtering and redaction.

Request:

```json
{
  "agent_id": "graph-agent",
  "seed_entities": ["eyasu"],
  "predicates": ["prefers"],
  "entity_types": ["language"],
  "min_confidence": 0.8,
  "max_hops": 2,
  "rank_by": "hop_then_confidence_then_recency",
  "limit": 10
}
```

Request options:

- `entity_canonical_name`: backward-compatible single seed.
- `seed_entities`: one or more seed entities for neighborhood traversal.
- `predicates`: optional relationship predicate filter.
- `entity_types`: optional entity-type filter; matches either side of a relationship.
- `min_confidence`: optional lower bound for active relationship confidence.
- `max_hops`: traversal depth (1-4, default `1`).
- `rank_by`: `hop_then_confidence_then_recency` (default), `hop_then_recency`, `confidence_then_recency`, or `recency`.

Response:

```json
{
  "agent_id": "graph-agent",
  "returned": 1,
  "relationships": [
    {
      "relationship_id": "uuid",
      "version_id": "uuid",
      "subject": { "entity_type": "person", "canonical_name": "eyasu" },
      "predicate": "prefers",
      "object": { "entity_type": "language", "canonical_name": "rust" },
      "confidence": 0.93,
      "attributes_json": { "source": "manual" },
      "redacted_fields": ["strength"],
      "hop_distance": 1,
      "rank_score": 1002726971430.0791
    }
  ]
}
```

Graph retrieval writes `retrieval_audit_events` with `request_type = graph_retrieve`.
The audit payload includes traversal and filter parameters (`seed_entities`, `max_hops`, `rank_by`, `predicates`, `entity_types`, `min_confidence`).

## POST /v1/audit/events/list

List recent retrieval audit events for verifiable privacy.

Request:

```json
{
  "limit": 50,
  "agent_id": "audit-agent",
  "request_type": "retrieve"
}
```

Response:

```json
{
  "returned": 1,
  "events": [
    {
      "id": "uuid",
      "agent_id": "audit-agent",
      "request_type": "retrieve",
      "scope_applied": "all-context",
      "shared_json": { "count": 1, "memory_item_ids": ["uuid"] },
      "redacted_json": { "entries": [{ "memory_item_id": "uuid", "redacted_fields": ["email"] }] },
      "created_at": "2026-02-16T03:38:43+00:00"
    }
  ]
}
```
