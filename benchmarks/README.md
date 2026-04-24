# Developer Memory Benchmark Seeds

`developer_memory_seed.json` is a local-first, machine-readable seed artifact for the developer-memory vertical slice.

The file contains:

- `fixtures.agent_scopes`: least-privilege scopes for runner setup.
- `fixtures.policies`: redaction policies to apply during retrieval.
- `fixtures.evidence_records`: immutable evidence references with stable IDs.
- `fixtures.memory_items`: active and stale memory fixtures linked to evidence IDs.
- `cases`: benchmark prompts with expected answer kind, required inclusions/exclusions, evidence IDs, abstention reason, and expected redactions.

Runners should treat `answer_kind = abstain` as a requirement to avoid unsupported claims, and should verify that stale memories are not presented as current facts.

## Local Retrieval v2 Runner

`run_developer_memory_benchmark.py` calls a configurable Yena `POST /v2/retrieve` endpoint once per case and scores the response against the seed expectations.

It checks:

- answer kind, inferred from `answer_context.answer_kind` when present or from `should_abstain` otherwise
- required text inclusions and exclusions across the returned answer context
- expected evidence IDs from `evidence_refs` or `evidence_ids`
- expected redaction keys from any `redactions` fields
- expected abstention reason when a case requires abstention

The script uses only the Python standard library. It assumes the benchmark fixtures have already been loaded into the Yena database used by `mcp-gateway`; this artifact is an evaluator, not a fixture loader.

### Usage

Start `mcp-gateway` against a database containing the seed fixture data, then run:

```sh
python3 benchmarks/run_developer_memory_benchmark.py \
  --url http://127.0.0.1:8082/v2/retrieve \
  --include-trace
```

Useful options:

```sh
# Run one case.
python3 benchmarks/run_developer_memory_benchmark.py --case dm-001-project-decision-recall

# Send a repo scope to every retrieval request.
python3 benchmarks/run_developer_memory_benchmark.py \
  --scope-json '{"kind":"repo","repo_path":"/Users/eyasu/Projects/Yena","branch":"main"}'

# Save a full machine-readable report.
python3 benchmarks/run_developer_memory_benchmark.py --json --output benchmarks/developer_memory_report.json
```

Exit code is `0` when all selected cases pass and `1` when any case fails. HTTP or JSON failures are reported as failed cases so the output can still be used in local iteration.
