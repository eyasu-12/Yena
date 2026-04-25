# Yena Tools

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
