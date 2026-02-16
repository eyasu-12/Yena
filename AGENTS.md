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
- Verified GitHub CLI is installed; authentication is still required before remote creation/push.

## Progress Tracker

- [x] Master vision captured
- [x] Build roadmap drafted
- [ ] Repository initialized and pushed to GitHub (initialized locally, push pending GitHub auth)
- [ ] Core service scaffolded
- [ ] Schema and migrations implemented
- [ ] Evidence ingestion v1 implemented
- [ ] Memory compiler v1 implemented
- [ ] MCP gateway v1 implemented
- [ ] Governance UI v1 implemented
- [ ] MVP released

## Open Questions

- Which policy engine should be standardized first: Cedar-style or custom DSL?
- Which first portability source should be prioritized after local files: email or calendar?
- Which two agent clients should be used for cross-agent validation?
