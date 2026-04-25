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
