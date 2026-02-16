# AGENTS Work Log

This file tracks major decisions, changes, and execution history for Yena.

## Project Intent

Yena is a local-first memory control plane with immutable evidence, compiled memory, and policy-filtered retrieval for agent portability and privacy.

## Operating Rules

- Keep architecture local-first by default.
- Preserve provenance for every committed memory item.
- Log every retrieval and redaction decision.
- Apply least-privilege scopes for every agent connection.

## Decision Log

### 2026-02-16

- Created foundational docs:
  - `MASTER_VISION.md`
  - `PROJECT_PLAN.md`
  - `AGENTS.md`
- Defined 12-week phased implementation roadmap.
- Established module boundaries (ingest, evidence, compiler, policy, MCP, UI).
- Established MVP boundaries and GitHub issue/milestone strategy.
- Initialized git repository on `main`.
- Created initial commit: `200dc1b`.
- Connected repository to GitHub remote `origin` (`https://github.com/eyasu-12/Yena.git`).
- Added Phase 0 foundation files:
  - `README.md`
  - `docs/ARCHITECTURE.md`
  - `docs/THREAT_MODEL.md`
  - `db/migrations/0001_init.sql`
  - `db/migrations/0002_indexes.sql`
  - `.github/ISSUE_TEMPLATE/feature_request.md`
  - `.github/ISSUE_TEMPLATE/bug_report.md`
- Noted local toolchain gap: Rust/Cargo is not installed yet.
- Added Rust workspace skeleton:
  - `Cargo.toml` workspace manifest
  - `crates/yena-model`
  - `services/ingest-service`
  - `services/memory-compiler`
  - `services/mcp-gateway`
  - `services/policy-engine`
- Validated SQLite migrations against a local test DB.
- Implemented `ingest-service` v1 API scaffold:
  - `GET /health`
  - `POST /v1/evidence` with request validation
  - SHA-256 checksum generation
  - duplicate detection by `(source_type, source_ref, checksum)`
  - SQLite-backed insert into immutable `evidence_records`
- Added unique evidence idempotency index in migration `0002_indexes.sql`.
- Added endpoint-level API contract in `docs/INGEST_API.md`.
- Installed Rust toolchain (`rustup`, `cargo`, `rustc`).
- Verified `ingest-service` build/tests and live API smoke behavior:
  - `POST /v1/evidence` first insert returns `201`
  - duplicate evidence returns `200` with `was_duplicate: true`
- Implemented `memory-compiler` v1 API:
  - `POST /v1/proposals`
  - `POST /v1/proposals/{id}/commit`
  - `POST /v1/proposals/{id}/reject`
- Verified compiler live integration flow:
  - commit creates active memory version
  - second commit supersedes prior version
  - reject transitions proposal to `rejected`
- Added API contract doc `docs/MEMORY_COMPILER_API.md`.
- Implemented `mcp-gateway` v1 API:
  - `POST /v1/scopes/upsert`
  - `POST /v1/policies/redact-keys`
  - `POST /v1/connect`
  - `POST /v1/retrieve`
- Verified live MCP integration flow:
  - agent scope limits retrieval by memory type
  - redact policy removes configured keys from projected memory
  - retrieval writes `retrieval_audit_events` with shared/redacted summaries
- Added API contract doc `docs/MCP_GATEWAY_API.md`.
- Saved external persistent-memory research notes for future core-memory iteration:
  - `docs/PERSISTENT_MEMORY_RESEARCH.md`
- Added MCP JSON-RPC endpoint in `mcp-gateway`:
  - `POST /mcp` with `initialize`, `tools/list`, and `tools/call`
  - tool mapping for `yena.connect`, `yena.retrieve`, `yena.scope.upsert`, `yena.policy.redact_keys`
- Verified live JSON-RPC integration flow end-to-end across ingest/compiler/mcp services.
- Extended `memory-compiler` with query/governance endpoints:
  - `GET /v1/memory/{canonical_key}`
  - `GET /v1/memory/{canonical_key}/history`
  - `POST /v1/memory/forget`
- Verified live memory history and forget behavior:
  - history returns version timeline with evidence links
  - forget removes memory item/versions/links
  - forget removes now-unreferenced linked evidence when `forget_evidence=true`
- Added retention policy execution to align with memory-decay governance:
  - `POST /v1/retention/policies/upsert`
  - `POST /v1/retention/jobs/run`
- Verified live retention behavior:
  - dry run reports matched stale memories without deletion
  - execute run deletes matching stale project memories while preserving non-matching memories
  - retention job status persisted in `retention_jobs`
- Added knowledge graph foundation for smart memory:
  - migration `db/migrations/0003_knowledge_graph.sql`
  - graph entities + relationships + relationship versions + evidence links
- Extended compiler graph APIs:
  - `POST /v1/graph/proposals/relationships`
  - `POST /v1/graph/proposals/{id}/commit`
  - `GET /v1/graph/relationships/{id}`
  - `GET /v1/graph/relationships/{id}/history`
- Verified live graph behavior:
  - graph proposal commit creates relationship version
  - second commit supersedes first version
  - history returns full temporal edge lifecycle with evidence links
- Extended `mcp-gateway` with graph-aware retrieval:
  - `POST /v1/graph/retrieve`
  - MCP tool: `yena.graph.retrieve`
- Verified graph retrieval governance behavior:
  - scope gate requires `graph` (or `relationship`) in allowed memory types
  - redaction policy applies to graph relationship attributes
  - graph retrieval writes audit events with `request_type = graph_retrieve`
- Added audit visibility APIs/tools:
  - `POST /v1/audit/events/list`
  - MCP tool: `yena.audit.list`
- Verified audit listing behavior:
  - returns both `retrieve` and `graph_retrieve` events
  - exposes shared/redacted payload summaries for dashboard-style privacy inspection
- Added graph traversal/ranking controls in MCP graph retrieval:
  - request supports `seed_entities`, `max_hops`, `rank_by`
  - response includes `hop_distance` and `rank_score`
- Verified traversal behavior:
  - `max_hops=1` returns direct neighborhood edges only
  - `max_hops=2` expands to second-hop relationships
  - `rank_by=hop_then_recency` and `rank_by=recency` produce different ordering

## Progress Tracker

- [x] Master vision captured
- [x] Build roadmap drafted
- [x] Repository initialized and pushed to GitHub
- [x] Core service scaffolded
- [x] Schema and migrations implemented
- [x] Evidence ingestion v1 implemented
- [x] Memory compiler v1 implemented
- [~] MCP gateway v1 implemented (JSON-RPC tool surface done; broader MCP spec coverage still pending)
- [~] Governance primitives implemented (forget + retention job APIs complete; governance UI pending)
- [~] Smart memory foundation implemented (graph schema + versioned relationship compiler complete; graph-aware retrieval in MCP pending)
- [~] Smart memory foundation implemented (graph schema + versioned relationship compiler + MCP graph retrieval complete; graph traversal/ranking improvements pending)
- [~] Smart memory foundation implemented (graph traversal/ranking added; advanced multi-hop ranking heuristics still pending)
- [~] Verifiable privacy implemented (audit write/read APIs and MCP access complete; governance dashboard UI pending)
- [ ] Governance UI v1 implemented
- [ ] MVP released

## Open Questions

- Which policy engine should be standardized first: Cedar-style or custom DSL?
- Which first portability source should be prioritized after local files: email or calendar?
- Which two agent clients should be used for cross-agent validation?
