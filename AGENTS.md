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

### 2026-04-24

- Implemented retrieval v2 vertical slice foundation:
  - `db/migrations/0006_retrieval_v2_foundation.sql`
  - `memory_item_metadata`
  - `observations`
  - `observation_memory_links`
  - `observation_evidence_links`
  - `retrieval_traces`
  - `retrieval_documents_fts`
- Added first-class retrieval answer contract types to `crates/yena-model`:
  - `RetrievalScope`
  - `RetrievalScopeKind`
  - `MemoryFreshness`
  - `AbstentionReason`
  - `RetrievalTrace`
  - `MemoryAnswer`
  - `MemoryAnswerContract`
- Added shared retrieval v2 pipeline skeleton in `mcp-gateway`:
  - candidate sources for active memory items, observations, and graph relationships
  - repo/workspace-aware scope filtering
  - rank fusion foundation using query matches, SQLite FTS/BM25 signal, confidence, freshness, and evidence count
  - calibrated abstention for missing evidence, out-of-scope, stale, contradicted, and low-confidence cases
  - optional trace output with redaction-safe fields only
- Extended memory compiler commits for retrieval v2 indexing:
  - `POST /v1/proposals` accepts optional scope and freshness metadata
  - committed memories upsert `memory_item_metadata`
  - committed memories and graph relationships upsert local `retrieval_documents_fts` documents
- Extended retrieval v2 scoring:
  - loads local SQLite FTS scores from `retrieval_documents_fts`
  - can retrieve concise memory answers through richer indexed documents
  - abstains when matching candidates have no evidence references
- Exposed retrieval v2 through:
  - `POST /v2/retrieve`
  - MCP tool `yena.retrieve.v2`
- Added retrieval v2 audit/trace persistence:
  - `retrieval_audit_events.request_type = retrieve_v2`
  - linked `retrieval_traces` row per retrieval v2 call
- Added developer-memory benchmark seed artifact:
  - `benchmarks/developer_memory_seed.json`
  - `benchmarks/README.md`
- Added developer-memory benchmark runner:
  - `benchmarks/run_developer_memory_benchmark.py`
  - validates answer kind, inclusions/exclusions, evidence IDs, redactions, and abstention reason against `/v2/retrieve`
- Created ongoing backlog file:
  - `TODOS.md`
  - deferred `AGENTS.md` / `CLAUDE.md` import until after retrieval v2 proves value
- Updated docs:
  - `docs/ARCHITECTURE.md`
  - `docs/MCP_GATEWAY_API.md`
  - `docs/MEMORY_COMPILER_API.md`
- Verified with `cargo fmt`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, benchmark runner validation, and a live compiler-to-gateway FTS smoke test.

### 2026-04-23

- Added graph confidence persistence migration:
  - `db/migrations/0004_graph_confidence.sql`
- Extended graph version storage to preserve proposal confidence at commit time.
- Extended compiler graph read APIs:
  - `GET /v1/graph/relationships/{id}` now returns active version confidence.
  - `GET /v1/graph/relationships/{id}/history` now returns per-version confidence.
- Extended graph retrieval in `mcp-gateway`:
  - request supports `predicates`, `entity_types`, `min_confidence`
  - ranking supports `hop_then_confidence_then_recency` and `confidence_then_recency`
  - default graph ranking now prefers hop distance, then confidence, then recency
  - response now includes relationship `confidence`
- Added unit coverage for graph filtering and confidence-aware ranking.
- Verified live graph confidence behavior:
  - default ranking returns the higher-confidence same-hop edge ahead of a newer low-confidence edge
  - `rank_by=hop_then_recency` still returns the newer same-hop edge first
  - semantic filters (`predicates`, `entity_types`, `min_confidence`) narrow retrieval as expected
- Verified updated workspace with `cargo fmt` and `cargo test`.
- Added graph canonicalization/compaction foundation:
  - `db/migrations/0005_graph_canonicalization.sql`
  - `graph_entity_aliases`
  - `graph_predicate_aliases`
  - `graph_relationship_redirects`
  - `graph_compaction_jobs`
- Extended compiler graph APIs:
  - `POST /v1/graph/canonicalization/entity-aliases/upsert`
  - `POST /v1/graph/canonicalization/predicate-aliases/upsert`
  - `POST /v1/graph/compaction/run`
- Extended graph commit behavior:
  - future graph proposals resolve entity aliases and predicate aliases before commit
  - canonical graph commits now stop fresh duplicate edges from being created once alias rules exist
- Added graph compaction behavior:
  - dry-run summary for planned canonicalization/redirect/merge work
  - active duplicate relationships collapse onto one canonical active relationship
  - surviving relationship receives a merged active version with max confidence and unioned evidence
  - redirected relationships are tracked in `graph_relationship_redirects`
  - orphaned alias entities are marked `compacted`
- Verified live graph compaction behavior:
  - future alias-based proposals now commit directly into canonical graph state
  - pre-existing alias relationships compact into one canonical active relationship
  - compaction reports canonicalized, redirected, merged, and compacted counts correctly
  - MCP graph retrieval returns only the canonical active relationship after compaction

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
- [~] Smart memory foundation implemented (graph schema, versioned relationship compiler, graph retrieval, traversal controls, confidence-aware filters, ranking heuristics, canonicalization rules, and compaction complete; broader reasoning and graph semantics still pending)
- [~] Verifiable privacy implemented (audit write/read APIs and MCP access complete; governance dashboard UI pending)
- [~] Retrieval v2 foundation implemented (answer contract, repo/workspace scope schema, observations schema, trace persistence, abstention behavior, benchmark seeds/runner, local FTS indexing, and FTS-aware rank fusion complete; observation compiler and full benchmark fixture loader still pending)
- [ ] Governance UI v1 implemented
- [ ] MVP released

## Open Questions

- Which policy engine should be standardized first: Cedar-style or custom DSL?
- Which first portability source should be prioritized after local files: email or calendar?
- Which two agent clients should be used for cross-agent validation?
