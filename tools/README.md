# Yena Tools

These tools are stdlib-only Python scripts for local development workflows.

## Markdown Memory Import

`import_markdown_memory.py` imports explicit local Markdown memory files into the `memory-compiler` service.

The tool reads files locally and sends their content to `POST /v1/import/markdown`. The compiler still does not read arbitrary filesystem paths.

Start the compiler:

```bash
cargo run -p memory-compiler
```

Import committed memories:

```bash
python3 tools/import_markdown_memory.py AGENTS.md CLAUDE.md
```

Create pending proposals for review instead of committing immediately:

```bash
python3 tools/import_markdown_memory.py AGENTS.md --pending
```

Preview request summaries without sending file content:

```bash
python3 tools/import_markdown_memory.py AGENTS.md --dry-run
```

Useful options:

- `--compiler-url http://127.0.0.1:8081`: memory compiler base URL.
- `--source-type local_markdown_memory`: source type recorded in Yena.
- `--confidence 0.74`: confidence attached to imported proposals.
- `--scope repo`: attach current git repo scope when available.
- `--scope none`: import without repo scope.
- `--json`: print full JSON responses.

## Retrieval V2 Query

`retrieve_memory.py` queries imported and compiled memory through the `mcp-gateway` `/v2/retrieve` endpoint.

Start the gateway against the same Yena DB used by the compiler:

```bash
cargo run -p mcp-gateway
```

Ask a repo-scoped question:

```bash
python3 tools/retrieve_memory.py "What database did we choose for this repo?"
```

Ask with trace output:

```bash
python3 tools/retrieve_memory.py "What database did we choose?" --include-trace --json
```

Preview the request without sending it:

```bash
python3 tools/retrieve_memory.py "What database did we choose?" --dry-run
```

Useful options:

- `--gateway-url http://127.0.0.1:8082`: MCP gateway base URL.
- `--agent-id yena-cli`: agent id written to retrieval audit logs.
- `--scope repo`: attach current git repo scope when available.
- `--scope global`: query global memory.
- `--scope custom --scope-json '{"kind":"workspace","workspace_path":"/path"}'`: query an explicit scope.
- `--limit 8`: maximum memories returned.
- `--include-trace`: request redaction-safe trace fields.
- `--json`: print the full JSON response.

## Audit Event Listing

`list_audit_events.py` lists recent retrieval audit events from `mcp-gateway`.

List recent events:

```bash
python3 tools/list_audit_events.py
```

Filter to the local retrieval CLI:

```bash
python3 tools/list_audit_events.py --agent-id yena-cli --request-type retrieve_v2
```

Print full JSON:

```bash
python3 tools/list_audit_events.py --json
```

Useful options:

- `--gateway-url http://127.0.0.1:8082`: MCP gateway base URL.
- `--limit 20`: maximum events returned.
- `--agent-id yena-cli`: filter by agent id.
- `--request-type retrieve_v2`: filter by retrieval type.
- `--dry-run`: print the request without sending it.
- `--json`: print the full JSON response.

## End-to-End Developer Memory Smoke

`dev_memory_smoke.py` verifies the local developer-memory loop against a fresh temporary SQLite DB:

1. Starts `memory-compiler`.
2. Imports a temporary Markdown memory fixture.
3. Starts `mcp-gateway` against the same DB.
4. Queries retrieval v2.
5. Lists the resulting audit event.

Run it:

```bash
python3 tools/dev_memory_smoke.py
```

Print the full report:

```bash
python3 tools/dev_memory_smoke.py --json
```

Useful options:

- `--compiler-bind 127.0.0.1:18081`: compiler bind address.
- `--gateway-bind 127.0.0.1:18082`: gateway bind address.
- `--agent-id yena-smoke`: agent id used for retrieval and audit.
- `--keep-files`: keep the generated DB and Markdown fixture for inspection.
