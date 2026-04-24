# Developer Memory Benchmark Seeds

`developer_memory_seed.json` is a local-first, machine-readable seed artifact for the developer-memory vertical slice.

The file contains:

- `fixtures.agent_scopes`: least-privilege scopes for runner setup.
- `fixtures.policies`: redaction policies to apply during retrieval.
- `fixtures.evidence_records`: immutable evidence references with stable IDs.
- `fixtures.memory_items`: active and stale memory fixtures linked to evidence IDs.
- `cases`: benchmark prompts with expected answer kind, required inclusions/exclusions, evidence IDs, abstention reason, and expected redactions.

Runners should treat `answer_kind = abstain` as a requirement to avoid unsupported claims, and should verify that stale memories are not presented as current facts.

## Load Fixtures

`load_developer_memory_seed.py` creates a local Yena SQLite database from the seed file. It applies the project migrations, inserts evidence, agent scopes, redaction policy, memory versions, metadata, canonical compiled observations, observation event history, links, and retrieval FTS documents.

```sh
python3 benchmarks/load_developer_memory_seed.py \
  --db /tmp/yena-dev-memory-benchmark.db \
  --reset
```

For repo-scoped benchmark data:

```sh
python3 benchmarks/load_developer_memory_seed.py \
  --db /tmp/yena-dev-memory-benchmark.db \
  --reset \
  --scope-kind repo \
  --repo-path /Users/eyasu/Projects/Yena \
  --repo-remote https://github.com/eyasu-12/Yena.git \
  --branch main
```

## Local Retrieval v2 Runner

`run_developer_memory_benchmark.py` calls a configurable Yena `POST /v2/retrieve` endpoint once per case and scores the response against the seed expectations.

It checks:

- answer kind, inferred from `answer_context.answer_kind` when present or from `should_abstain` otherwise
- required text inclusions and exclusions across the returned answer context
- expected evidence IDs from `evidence_refs` or `evidence_ids`
- expected redaction keys from any `redactions` fields
- expected abstention reason when a case requires abstention

When `--include-trace` is used, observation-backed answers may also include compact lifecycle events from `observation_events` so retrieval debugging can distinguish loaded, strengthened, weakened, and contradicted observations without exposing raw previous/current payloads.

Both scripts use only the Python standard library.

### Usage

Start `mcp-gateway` against a database containing the seed fixture data:

```sh
YENA_DB_PATH=/tmp/yena-dev-memory-benchmark.db \
YENA_BIND=127.0.0.1:8082 \
cargo run -p mcp-gateway
```

Then run:

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
