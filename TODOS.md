# TODOS

## Core Memory Engine

### Expand Markdown Import Into Product Workflow

**What:** Build on the backend Markdown import endpoint with a product workflow that turns existing developer memory files into Yena evidence, memory proposals, observations, benchmark cases, and user-visible import reports.

**Why:** The first customer already maintains repo memory files. Yena should make adoption easy by ingesting the status quo instead of asking users to manually re-enter context.

**Context:** The backend compiler endpoint now exists: `POST /v1/import/markdown`. The next layer should add a narrow caller flow around it, then connect imported items to retrieval benchmark cases and later UI/audit visibility. Keep the first version narrow: local Markdown files only, no broad connector framework, no dashboard requirement.

**Effort:** M
**Priority:** P2
**Depends on:** Backend Markdown import endpoint, retrieval v2, observations, policy-filtered retrieval traces, and the developer-memory benchmark.

## Completed

### CI Checks

**Completed:** `.github/workflows/ci.yml`.

**What it does:** Runs Rust formatting, Rust tests, clippy with warnings denied, Python tool compilation, and Python tool unit tests on pushes to `main`, pull requests, and manual workflow dispatch.

### Developer Quickstart

**Completed:** `docs/DEVELOPER_QUICKSTART.md`.

**What it does:** Documents the current local developer path: verify checks, run the end-to-end smoke, start services manually, import Markdown memory, retrieve governed memory, inspect audit events, and forget an imported source.

### Backend Markdown Import Foundation

**Completed:** `POST /v1/import/markdown` in `memory-compiler`.

**What it does:** Parses Markdown content passed by the caller, creates one immutable evidence record per imported item, creates `markdown_import` proposals, optionally commits them directly into memory items, compiled observations, observation events, and retrieval FTS. Repeated unchanged imports skip duplicate commits. Each import persists a durable job report queryable through `GET /v1/import/jobs/{id}`. Imported sources can be revoked through `POST /v1/import/sources/forget`, which deletes committed imported memories, pending import proposals, retrieval indexes, compiled observations, and now-unreferenced evidence.

### Local Markdown Import CLI

**Completed:** `tools/import_markdown_memory.py`.

**What it does:** Reads explicit local Markdown files, attaches git repo scope when available, and sends content to the compiler import endpoint. It supports committed imports, pending proposal imports, dry-run summaries, configurable compiler URL, source type, confidence, and JSON output.

### Local Imported Source Forget CLI

**Completed:** `tools/forget_import_source.py`.

**What it does:** Calls `POST /v1/import/sources/forget` from the terminal, resolving existing file paths with the same source-ref convention as the import CLI. It supports source type filters, all-source-type forget, keep-evidence mode, dry-run requests, and JSON output.

### Local Retrieval V2 CLI

**Completed:** `tools/retrieve_memory.py`.

**What it does:** Sends governed questions to `mcp-gateway` `/v2/retrieve`, defaults to repo-scoped retrieval when inside a git repo, writes a stable CLI `agent_id` for audit logs, and supports dry-run requests, custom scopes, trace requests, JSON output, and concise human-readable answers.

### Local Audit Events CLI

**Completed:** `tools/list_audit_events.py`.

**What it does:** Lists recent retrieval audit events from `mcp-gateway`, supports filtering by agent id and request type, and prints either concise privacy summaries or full JSON. This makes the verifiable-privacy loop visible from the terminal before a dashboard exists.

### End-to-End Developer Memory Smoke

**Completed:** `tools/dev_memory_smoke.py`.

**What it does:** Runs the local import -> retrieval v2 -> audit visibility -> source forget loop against a fresh temporary DB. It starts `memory-compiler`, imports a Markdown fixture, starts `mcp-gateway` on the same DB, verifies retrieval returns the expected SQLite memory, verifies a `retrieve_v2` audit event is visible, then revokes the imported source and verifies deletion counts.
