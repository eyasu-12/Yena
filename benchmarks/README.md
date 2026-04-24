# Developer Memory Benchmark Seeds

`developer_memory_seed.json` is a local-first, machine-readable seed artifact for the developer-memory vertical slice.

The file contains:

- `fixtures.agent_scopes`: least-privilege scopes for runner setup.
- `fixtures.policies`: redaction policies to apply during retrieval.
- `fixtures.evidence_records`: immutable evidence references with stable IDs.
- `fixtures.memory_items`: active and stale memory fixtures linked to evidence IDs.
- `cases`: benchmark prompts with expected answer kind, required inclusions/exclusions, evidence IDs, abstention reason, and expected redactions.

Runners should treat `answer_kind = abstain` as a requirement to avoid unsupported claims, and should verify that stale memories are not presented as current facts.
