# Yena Threat Model (Initial)

## Security Objectives

- Prevent unauthorized memory exposure.
- Preserve provenance and audit integrity.
- Ensure user revocation actions are enforceable and visible.

## Assumptions

- Primary deployment is single-user local machine.
- Local filesystem permissions are trusted baseline.
- Cloud LLM calls are untrusted egress destinations.

## Key Risks

1. Over-broad retrieval scope to an agent.
2. Sensitive data leakage during projection.
3. Tampering with audit logs.
4. Ingestion of malicious or malformed evidence.

## Mitigations (v1)

- Default deny policy for agent scopes.
- Redaction-on-projection pipeline before MCP response.
- Append-only audit events with hash chaining (planned).
- Strict schema validation on ingest.
- Source-level consent gates before ingestion.

## Open Security Tasks

- Define cryptographic integrity strategy for evidence and audit logs.
- Add local encryption-at-rest option.
- Add policy regression suite for high-risk categories.
