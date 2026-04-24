# Yena Architecture

## System Goal

Provide cross-agent persistent memory with explicit governance, provenance, and policy-aware projection.

## Core Data Flow

1. Ingestion: agent activities and portability jobs enter Yena.
2. Evidence Store: every input becomes immutable evidence.
3. Memory Compiler: evidence is transformed into canonical memory items through proposal + conflict resolution.
4. Observation Compiler: committed memory is projected into durable observations linked to memory and evidence.
5. Retrieval Indexing: committed memory, observations, and graph relationships are projected into a local SQLite FTS index.
6. Graph Canonicalization: alias rules and compaction collapse duplicate entities/edges into stable canonical graph state.
7. Retrieval v2: repo/workspace-scoped candidate sources are fused into an answer contract with abstention and optional trace output.
8. Policy Projection: memory and traces are filtered by scope/sensitivity before exposure via MCP.
9. Audit: every retrieval and redaction is logged.

## Modules

- `ingest-service`: ingest and normalize events.
- `evidence-store`: append-only evidence and provenance metadata.
- `memory-compiler`: dedupe, supersede, canonicalize.
- `graph-compiler`: entity/relationship memory with versioned edges, alias rules, and compaction.
- `policy-engine`: evaluate source/category/agent scope rules.
- `mcp-gateway`: retrieval and commit APIs.
- `audit-log`: immutable access and redaction events.
- `control-ui`: consent, retention, forget operations.

## Retrieval v2 Foundation

Retrieval v2 is a shared internal pipeline exposed through `POST /v2/retrieve` and MCP tool `yena.retrieve.v2`.

```text
MCP/API request
  |
  v
RetrievalV2Request
  |
  v
Scope resolution
  |-- agent scopes
  |-- repo/workspace scope
  |-- allowed memory types
  |
  v
Candidate sources
  |-- memory_items + memory_item_versions + memory_item_metadata
  |-- observations + observation evidence/memory links
  |-- graph_relationships + active versions
  |-- retrieval_documents_fts
  |
  v
Rank fusion foundation
  |-- query term matches
  |-- SQLite FTS/BM25 signal
  |-- stopword filtering
  |-- top relevance band filtering
  |-- duplicate memory/observation collapse
  |-- confidence
  |-- freshness
  |-- evidence count
  |
  v
Memory Answer Contract
  |-- should_abstain
  |-- abstention_reason
  |-- abstention_message
  |-- memories
  |-- optional trace
  |
  v
Trace redaction gate
  |
  v
Audit event + retrieval trace
```

The first implementation intentionally stays local-first: SQLite tables, deterministic scoring, SQLite FTS, and no required vector database.

## Non-Functional Requirements

- Local-first by default.
- Append-only evidence + traceable memory lineage.
- Explicit user controls for source opt-in/out.
- Deterministic policy decisions for every retrieval.

## Interface Boundaries (v1)

- Ingest API: append events with source metadata.
- Compiler API: create/update `MemoryProposal`, commit/reject.
- MCP API: `connect`, `retrieve`, `propose_commit`.
- Governance API: consent, retention policy, forget command.
