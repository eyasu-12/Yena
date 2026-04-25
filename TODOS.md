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

### Backend Markdown Import Foundation

**Completed:** `POST /v1/import/markdown` in `memory-compiler`.

**What it does:** Parses Markdown content passed by the caller, creates one immutable evidence record per imported item, creates `markdown_import` proposals, optionally commits them directly into memory items, compiled observations, observation events, and retrieval FTS. Repeated unchanged imports skip duplicate commits. Each import persists a durable job report queryable through `GET /v1/import/jobs/{id}`. Imported sources can be revoked through `POST /v1/import/sources/forget`, which deletes committed imported memories, pending import proposals, retrieval indexes, compiled observations, and now-unreferenced evidence.

### Local Markdown Import CLI

**Completed:** `tools/import_markdown_memory.py`.

**What it does:** Reads explicit local Markdown files, attaches git repo scope when available, and sends content to the compiler import endpoint. It supports committed imports, pending proposal imports, dry-run summaries, configurable compiler URL, source type, confidence, and JSON output.

### Local Retrieval V2 CLI

**Completed:** `tools/retrieve_memory.py`.

**What it does:** Sends governed questions to `mcp-gateway` `/v2/retrieve`, defaults to repo-scoped retrieval when inside a git repo, writes a stable CLI `agent_id` for audit logs, and supports dry-run requests, custom scopes, trace requests, JSON output, and concise human-readable answers.
