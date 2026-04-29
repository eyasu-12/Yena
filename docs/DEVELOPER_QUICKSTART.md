# Developer Quickstart

This guide runs the current local developer-memory workflow end to end:

1. Verify the repo.
2. Import an existing Markdown memory file.
3. Retrieve governed memory.
4. Inspect the audit event.
5. Forget the imported source.

Yena is local-first. These commands use a local SQLite database and localhost services.

## Prerequisites

- Rust toolchain with `cargo`, `rustfmt`, and `clippy`.
- Python 3.
- Optional: `gh` if you want to inspect GitHub Actions runs.

## 1. Verify The Repo

Run the same deterministic checks used by CI:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
PYTHONPYCACHEPREFIX=/tmp/yena_pycache python3 -m unittest \
  tools/test_import_markdown_memory.py \
  tools/test_forget_import_source.py \
  tools/test_retrieve_memory.py \
  tools/test_list_audit_events.py \
  tools/test_dev_memory_smoke.py
```

Optional GitHub CI check:

```bash
gh run list --limit 5
```

## 2. Run The Fast End-To-End Smoke

This starts local services against a temporary DB, imports a Markdown fixture, retrieves memory, lists the audit event, then forgets the imported source.

```bash
python3 tools/dev_memory_smoke.py
```

Expected shape:

```text
Yena dev-memory smoke passed.
import: committed=2 skipped=0 job=...
retrieval: Use SQLite for local-first storage.
audit: events=1 latest=retrieve_v2
forget: deleted_memories=2 deleted_evidence=2
```

## 3. Run Services Manually

Use the same DB for `memory-compiler` and `mcp-gateway`:

```bash
export YENA_DB_PATH=/tmp/yena-quickstart.db
```

Terminal 1:

```bash
YENA_DB_PATH=$YENA_DB_PATH cargo run -p memory-compiler
```

Terminal 2:

```bash
YENA_DB_PATH=$YENA_DB_PATH cargo run -p mcp-gateway
```

## 4. Import Markdown Memory

Preview the import request without sending file content:

```bash
python3 tools/import_markdown_memory.py AGENTS.md --dry-run
```

Import committed memory:

```bash
python3 tools/import_markdown_memory.py AGENTS.md
```

Use pending proposals instead of immediate commit:

```bash
python3 tools/import_markdown_memory.py AGENTS.md --pending
```

## 5. Retrieve Memory

Ask a repo-scoped question:

```bash
python3 tools/retrieve_memory.py "What database did we choose for this repo?"
```

Ask with trace JSON:

```bash
python3 tools/retrieve_memory.py "What database did we choose?" --include-trace --json
```

## 6. Inspect Audit Events

List recent retrieval v2 audit events for the local CLI agent:

```bash
python3 tools/list_audit_events.py --agent-id yena-cli --request-type retrieve_v2
```

Full JSON:

```bash
python3 tools/list_audit_events.py --agent-id yena-cli --request-type retrieve_v2 --json
```

## 7. Forget The Imported Source

Preview the forget request:

```bash
python3 tools/forget_import_source.py AGENTS.md --dry-run
```

Forget the imported source and now-unreferenced evidence:

```bash
python3 tools/forget_import_source.py AGENTS.md
```

Keep evidence when possible:

```bash
python3 tools/forget_import_source.py AGENTS.md --keep-evidence
```

## Current Boundaries

- The tools are local developer workflows, not a packaged product CLI yet.
- `memory-compiler` receives file content from the caller; it does not read arbitrary filesystem paths.
- Retrieval v2 is implemented and benchmarked, but richer ontology-aware graph reasoning is still future work.
- The governance dashboard is not implemented yet; audit visibility is currently available through API and CLI.
