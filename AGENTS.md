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

## Progress Tracker

- [x] Master vision captured
- [x] Build roadmap drafted
- [x] Repository initialized and pushed to GitHub
- [x] Core service scaffolded
- [x] Schema and migrations implemented
- [x] Evidence ingestion v1 implemented
- [x] Memory compiler v1 implemented
- [~] MCP gateway v1 implemented (JSON-RPC tool surface done; broader MCP spec coverage still pending)
- [ ] Governance UI v1 implemented
- [ ] MVP released

## Open Questions

- Which policy engine should be standardized first: Cedar-style or custom DSL?
- Which first portability source should be prioritized after local files: email or calendar?
- Which two agent clients should be used for cross-agent validation?
