# Persistent Memory Research Notes

Saved on: 2026-02-16
Purpose: reference implementations and reusable patterns for Yena core memory work.

## Candidates Reviewed

## Mem0

- Repo: https://github.com/mem0ai/mem0
- What it offers:
  - memory SDK and retrieval workflows
  - integrations across frameworks/providers
  - self-host and hosted options
- Reusable ideas for Yena:
  - memory CRUD/retrieval API shape
  - integration adapters and client ergonomics

## OpenMemory (Mem0 MCP layer)

- Overview: https://docs.mem0.ai/openmemory/overview
- Quickstart: https://docs.mem0.ai/openmemory/quickstart
- README: https://raw.githubusercontent.com/mem0ai/mem0/main/openmemory/README.md
- What it offers:
  - MCP-oriented memory server
  - local deployment flow
  - standard memory operations for assistant clients
- Reusable ideas for Yena:
  - MCP tool surface conventions
  - cross-client persistence handoff patterns

## Supermemory

- Repo: https://github.com/supermemoryai/supermemory
- MCP repo: https://github.com/supermemoryai/supermemory-mcp
- Integration reference: https://supermemory.ai/docs/integrations/opencode
- Plugin repo: https://github.com/supermemoryai/opencode-supermemory
- What it offers:
  - practical scope and context injection patterns
  - memory compaction and operational UX patterns
- Reusable ideas for Yena:
  - user/project memory scope strategy
  - trigger-driven capture and remember/forget UX

## Reuse Strategy for Yena

- Keep Yena as source-of-truth for:
  - immutable evidence records
  - proposal/commit/supersede compiler lifecycle
  - policy projection + retrieval audit logs
- Reuse externally for:
  - MCP command compatibility patterns
  - client integrations and onboarding UX
  - optional retrieval backend patterns

## Build Decision

- Build core governance model in-house.
- Add compatibility adapters for Mem0/OpenMemory-like MCP semantics.
- Borrow Supermemory UX ideas where they improve user control and quality.

## Caveat

- Some local-first setups still depend on external model providers for extraction/summarization.
- Keep provider path pluggable so Yena can remain fully local when needed.
