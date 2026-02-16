# Yena Project Plan

## Objective

Build Yena as a local-first memory control plane that provides portable memory, strict governance, and policy-safe retrieval for any agent.

## Product Scope

- Ingest interaction activity and historical datasets.
- Preserve immutable evidence with provenance.
- Compile memory into structured canonical items.
- Expose policy-filtered memory through MCP.
- Provide user-visible auditability, revocation, retention, and forgetting.

## Recommended v1 Tech Stack

- Core service: Rust (axum + tokio) for reliability and performance.
- Local datastore: SQLite with migrations (`sqlx` or `rusqlite`).
- Search and retrieval: SQLite FTS5 + optional vector extension later.
- Policy engine: Cedar or OPA-style policy layer (start with simple rule DSL).
- MCP interface: JSON-RPC compatible MCP server.
- UI: Lightweight local dashboard (SvelteKit or Next.js) served locally.

## System Modules

1. `ingest-service`: Activity log ingestion and portability jobs.
2. `evidence-store`: Append-only evidence records and provenance links.
3. `memory-compiler`: Proposal pipeline, deduplication, conflict resolution.
4. `policy-engine`: Scope and sensitivity-based projection rules.
5. `mcp-gateway`: Agent-facing retrieval and write APIs.
6. `audit-log`: Every access, share, and redaction event.
7. `control-ui`: Consent toggles, retention settings, forget actions.

## Data Model (v1)

- `evidence_records`
- `memory_proposals`
- `memory_items`
- `memory_item_versions`
- `memory_links` (evidence-to-memory)
- `agent_scopes`
- `policy_rules`
- `retrieval_audit_events`
- `retention_jobs`

## Implementation Phases

### Phase 0: Foundation (Week 1)

- Initialize monorepo structure and CI.
- Define schemas for evidence, memory proposals, and memory items.
- Implement migration pipeline and local config management.
- Add threat model doc and privacy assumptions.

Exit criteria:

- Project builds locally.
- DB migrations run cleanly.
- Baseline docs exist for architecture and security assumptions.

### Phase 1: Evidence Store and Ingestion (Weeks 2-3)

- Build append-only evidence write path.
- Capture activity events from a local agent connector.
- Add first portability job from local files (documents folder import).
- Enforce provenance requirements.

Exit criteria:

- Evidence records are immutable and queryable by source.
- Ingestion pipeline handles retries and idempotency.

### Phase 2: Memory Compiler (Weeks 4-5)

- Implement `MemoryProposal` workflow.
- Add canonicalization and deduplication rules.
- Add supersede/version logic for conflicting facts.
- Add confidence and temporal metadata.

Exit criteria:

- Contradictory facts produce versioned memory state.
- Canonical memory item can trace all supporting evidence.

### Phase 3: Policy Firewall + MCP (Weeks 6-7)

- Implement policy rule evaluation.
- Build projection pipeline with redaction and scope filtering.
- Expose retrieval and proposal endpoints through MCP.
- Add per-agent scope profiles.

Exit criteria:

- Two different agent profiles return different memory projections.
- Redaction events are logged and inspectable.

### Phase 4: User Governance Surface (Weeks 8-9)

- Build local dashboard for consent, retention, and forget controls.
- Implement retention jobs and configurable decay windows.
- Implement forget command to revoke memory and linked evidence visibility.

Exit criteria:

- User can enable/disable sources.
- User can execute forget and observe state transition + audit event.

### Phase 5: Handshake UX + Cross-Agent Validation (Weeks 10-11)

- Implement agent handshake flow and capability disclosure.
- Validate with at least 2 agent clients.
- Add regression tests for retrieval correctness and policy safety.

Exit criteria:

- New agent can connect and retrieve relevant committed context on first run.
- Audit logs clearly show shared vs redacted fields.

### Phase 6: Hardening and Launch (Week 12)

- Performance profiling and indexing improvements.
- Backup/export and recovery flow.
- Security review and release checklist.

Exit criteria:

- Stable local-first release candidate.
- Export/import works and preserves provenance.

## MVP Definition (first usable release)

MVP includes:

- Evidence ingestion from activity logs + one portability source.
- Memory compiler with proposal, dedupe, and supersede.
- MCP retrieval with strict policy filtering.
- Audit log viewer and basic control UI.

MVP excludes (for now):

- Distributed sync.
- Advanced ML extraction and large-scale embeddings.
- Multi-tenant cloud hosting.

## Engineering Backlog (Initial)

1. Scaffold service crates and shared schema package.
2. Add SQLite migrations and repository abstraction.
3. Implement evidence append-only API.
4. Build proposal acceptance/rejection flow.
5. Implement versioned memory item lifecycle.
6. Build policy evaluator and test matrix.
7. Implement MCP server commands (`connect`, `retrieve`, `propose_commit`).
8. Add audit event pipeline.
9. Build dashboard pages (consent, logs, memory browser).
10. Add retention and forget workflows.
11. Add integration tests for contradiction handling.
12. Add end-to-end tests for redaction behavior.

## GitHub Tracking Strategy

- Branching: `main` + short-lived feature branches.
- Commit style: Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`).
- Labels: `area/ingest`, `area/compiler`, `area/policy`, `area/mcp`, `area/ui`, `security`, `privacy`, `good-first-task`.
- Milestones:
  - M1 Foundation
  - M2 Evidence + Ingestion
  - M3 Compiler
  - M4 Firewall + MCP
  - M5 Governance UI
  - M6 MVP Launch

## Risks and Mitigations

- Over-collection risk: Default deny on ingestion sources; explicit consent by source.
- Hallucinated memory commits: Require proposal confirmation thresholds and confidence metadata.
- Policy bypass risk: Enforce policy checks in one gateway path only.
- Data corruption risk: Append-only evidence + migration tests + periodic local backups.

## Definition of Done

A feature is done when:

- Unit and integration tests pass.
- Policy behavior has explicit tests.
- Audit visibility is present for read/write paths.
- Documentation updated in `AGENTS.md` and relevant design docs.
