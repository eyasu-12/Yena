# Yena

Yena is a local-first memory control plane for agentic systems.

## What This Repo Contains

- Product vision: `MASTER_VISION.md`
- Build roadmap: `PROJECT_PLAN.md`
- Work log and decisions: `AGENTS.md`
- Architecture details: `docs/ARCHITECTURE.md`
- Security assumptions: `docs/THREAT_MODEL.md`
- Ingest API contract: `docs/INGEST_API.md`
- Memory compiler API contract: `docs/MEMORY_COMPILER_API.md`
- MCP gateway API contract: `docs/MCP_GATEWAY_API.md`
- External memory research notes: `docs/PERSISTENT_MEMORY_RESEARCH.md`
- DB schema migrations: `db/migrations/`
- Local developer tools: `tools/`

## Phase 0 Status

- [x] Master vision and roadmap documented
- [x] Git initialized and connected to GitHub
- [x] Baseline architecture and threat model docs
- [x] Initial SQLite schema migration
- [x] Runtime scaffolding (Rust workspace)
- [ ] CI and tests

## Next Commands

1. Run tests:

```bash
cargo test
```

2. Run services (three terminals):

```bash
cargo run -p ingest-service
cargo run -p memory-compiler
cargo run -p mcp-gateway
```

3. Import an existing Markdown memory file:

```bash
python3 tools/import_markdown_memory.py AGENTS.md --dry-run
python3 tools/import_markdown_memory.py AGENTS.md
```

4. Ask Yena about imported memory:

```bash
python3 tools/retrieve_memory.py "What database did we choose for this repo?"
```

5. Exercise MCP JSON-RPC endpoint:

```bash
curl -X POST http://127.0.0.1:8082/mcp \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc":"2.0",
    "id":1,
    "method":"tools/list",
    "params":{}
  }'
```
