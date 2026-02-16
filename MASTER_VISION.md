# Yena: Master Vision and Product Specification

This document serves as the Master Vision and Product Specification for Yena. It incorporates the Local-First architectural mandate, the Memory Firewall governance model, and the Portability-First ingestion strategy.

## 1. Executive Summary

Yena is a local-first, long-term memory control plane. It sits between a user's disparate data sources and the AI agents they use. Unlike a standard database, Yena is a governance layer that transforms raw activity and imported data into structured, portable, and policy-governed Memory Items.

The goal is to move beyond stateless agents and brittle `memory.md` files toward a reliable, automated, and user-controlled cognitive history that works across any model or platform.

## 2. The Core Problem: The Context Gap

Today, users face a forced choice between two suboptimal states:

- Manual Burden: Users manually maintain Markdown files (for example `user_prefs.md`) which are hard for agents to update accurately and impossible to scale over years.
- Data Silo: Memory is trapped inside a specific provider (for example ChatGPT memory), making it non-portable and opaque to the user.

Yena's answer is a unified, local-first interface that gives agents structured recall while giving users a kill switch and audit log for every piece of information an agent knows.

## 3. Product Architecture and Firewall Logic

Yena is built on a four-tier architecture designed for Privacy by Design.

### Tier 1: Ingestion and Portability

Yena uses two primary paths to gather context:

- Activity Logging: Real-time capture of agent interactions (tool calls, decisions, user preferences).
- Portability Jobs (DTP-style): Bulk ingestion of historical data (emails, calendars, documents) using open-source data transfer protocols. This is treated as historical grounding rather than active memory.

### Tier 2: Evidence Store (Immutable Ground Truth)

Every piece of data Yena receives is stored as an Evidence Record.

- Provenance: Every memory points back to a specific event or file.
- Immutability: Evidence is never deleted and provides the audit trail for "Why does the agent think this?"

### Tier 3: Memory Compiler (Git Model)

Yena compiles memory.

- Proposal System: When an agent learns something new, it issues a `MemoryProposal`.
- Conflict Resolution: If new evidence contradicts old memory, the compiler version-controls the fact. It marks old facts as Superseded and new facts as Active while preserving temporal history.
- Deduplication: Similar observations are merged into a canonical Memory Item.

### Tier 4: Policy Projection Layer (Firewall)

This is the interface exposed to agents via MCP.

- Redaction-on-the-Fly: Before memory is sent to a cloud LLM, Yena filters based on user-defined sensitivity rules.
- Scope Limitation: A coding agent may only access project memories, while a personal assistant can access relationship and calendar memories.

## 4. Success Conditions (North Star)

Yena succeeds when:

1. Zero-Manual Maintenance: The user deletes `memory.md` because automated committed memories are more accurate and easier to search.
2. Agent Portability: A user can switch agents and retain preferences and project context via Yena MCP.
3. Verifiable Privacy: A user can inspect every retrieval event, what was shared, and what was redacted.

## 5. User Control and Governance Primitives

- Granular Consent: Enable or disable specific data sources.
- Retention Policies: Decay short-term data while preserving long-term facts.
- Forget Command: Strike a memory and associated evidence from local store.

## 6. Agent Handshake Experience

1. Connection: "OpenClaw, connect to Yena."
2. Verification: Agent confirms reachable contexts and privacy mode.
3. Learning: Agent asks whether to commit inferred preference.
4. Retrieval: Agent recalls prior decisions and proceeds with continuity.

## 7. Strategic Differentiation

| Feature | Legacy Memory (Vector DBs) | Yena (Memory Control Plane) |
| --- | --- | --- |
| Trust | Black-box storage | Audit log for every retrieval |
| Accuracy | Bag-of-bits strings | Structured schema (facts, relationships, decisions) |
| Control | Difficult targeted deletion | Granular revocation by source/category |
| Persistence | Session-based or siloed | Cross-agent and cross-model persistence |

## 8. Closing Statement

Yena is Sovereign Memory for the AI era. By building on open standards (MCP and DTP) and prioritizing local-first architecture, Yena ensures users remain the authority over their digital history.
