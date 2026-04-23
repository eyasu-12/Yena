# Yena Architecture

## System Goal

Provide cross-agent persistent memory with explicit governance, provenance, and policy-aware projection.

## Core Data Flow

1. Ingestion: agent activities and portability jobs enter Yena.
2. Evidence Store: every input becomes immutable evidence.
3. Memory Compiler: evidence is transformed into canonical memory items through proposal + conflict resolution.
4. Graph Canonicalization: alias rules and compaction collapse duplicate entities/edges into stable canonical graph state.
5. Policy Projection: memory is filtered by scope/sensitivity before exposure via MCP.
6. Audit: every retrieval and redaction is logged.

## Modules

- `ingest-service`: ingest and normalize events.
- `evidence-store`: append-only evidence and provenance metadata.
- `memory-compiler`: dedupe, supersede, canonicalize.
- `graph-compiler`: entity/relationship memory with versioned edges, alias rules, and compaction.
- `policy-engine`: evaluate source/category/agent scope rules.
- `mcp-gateway`: retrieval and commit APIs.
- `audit-log`: immutable access and redaction events.
- `control-ui`: consent, retention, forget operations.

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
