#!/usr/bin/env python3
"""Run the Yena developer-memory retrieval v2 benchmark seed.

The runner is intentionally stdlib-only so it can run in a fresh checkout. It
expects the benchmark fixtures to already be loaded into the target Yena DB and
calls the configured HTTP /v2/retrieve endpoint once per case.
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Iterable

DEFAULT_SEED = Path(__file__).with_name("developer_memory_seed.json")
DEFAULT_URL = "http://127.0.0.1:8082/v2/retrieve"
DEFAULT_SCOPE = {"kind": "global"}


def main() -> int:
    args = parse_args()
    seed = load_json(args.seed)
    cases = select_cases(seed.get("cases", []), args.case)
    if not cases:
        print("No benchmark cases selected.", file=sys.stderr)
        return 2

    scope_by_id = {
        scope.get("scope_id"): scope for scope in seed.get("fixtures", {}).get("agent_scopes", [])
    }

    results = []
    for case in cases:
        result = run_case(case, scope_by_id, args)
        results.append(result)
        if not args.json:
            print(format_case_result(result))

    summary = summarize(results)
    report = {
        "benchmark_id": seed.get("benchmark_id"),
        "schema_version": seed.get("schema_version"),
        "target_url": args.url,
        "summary": summary,
        "results": results,
    }

    if args.output:
        args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(
            "\nSummary: {passed}/{total} passed, {failed} failed, score {score:.1%}".format(
                **summary
            )
        )
        if args.output:
            print(f"Wrote JSON report to {args.output}")

    return 0 if summary["failed"] == 0 else 1


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Evaluate Yena /v2/retrieve responses against developer_memory_seed.json."
    )
    parser.add_argument("--seed", type=Path, default=DEFAULT_SEED, help="Path to seed JSON.")
    parser.add_argument("--url", default=DEFAULT_URL, help="Yena /v2/retrieve URL.")
    parser.add_argument(
        "--scope-json",
        help="JSON scope object sent to every request. Defaults to {'kind':'global'}."
    )
    parser.add_argument(
        "--agent-id",
        help="Override the agent_id from the selected fixture scope."
    )
    parser.add_argument("--limit", type=int, default=8, help="Retrieval limit per case.")
    parser.add_argument(
        "--include-trace",
        action="store_true",
        help="Request trace fields from /v2/retrieve. Useful for redaction checks."
    )
    parser.add_argument(
        "--case",
        action="append",
        help="Run only a case_id. Can be provided multiple times."
    )
    parser.add_argument("--timeout", type=float, default=10.0, help="HTTP timeout in seconds.")
    parser.add_argument("--retries", type=int, default=0, help="Retries per case after HTTP errors.")
    parser.add_argument("--retry-delay", type=float, default=0.25, help="Delay between retries.")
    parser.add_argument("--output", type=Path, help="Optional path for a JSON report.")
    parser.add_argument("--json", action="store_true", help="Print the full JSON report only.")
    return parser.parse_args()


def load_json(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text())
    except OSError as exc:
        raise SystemExit(f"Failed to read {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"Invalid JSON in {path}: {exc}") from exc


def select_cases(cases: list[dict[str, Any]], selected: list[str] | None) -> list[dict[str, Any]]:
    if not selected:
        return cases
    wanted = set(selected)
    found = {case.get("case_id") for case in cases if case.get("case_id") in wanted}
    missing = sorted(wanted - found)
    if missing:
        raise SystemExit(f"Unknown case_id(s): {', '.join(missing)}")
    return [case for case in cases if case.get("case_id") in wanted]


def run_case(case: dict[str, Any], scope_by_id: dict[str, dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    request = build_request(case, scope_by_id, args)
    started = time.monotonic()
    response, error = post_json_with_retries(args.url, request, args.timeout, args.retries, args.retry_delay)
    elapsed_ms = round((time.monotonic() - started) * 1000, 1)

    if error is not None:
        return {
            "case_id": case.get("case_id"),
            "category": case.get("category"),
            "passed": False,
            "score": 0.0,
            "elapsed_ms": elapsed_ms,
            "request": request,
            "error": error,
            "checks": [{"name": "http_request", "passed": False, "details": error}],
        }

    checks = evaluate(case.get("expected", {}), response)
    passed = all(check["passed"] for check in checks)
    score = sum(1 for check in checks if check["passed"]) / len(checks) if checks else 0.0
    return {
        "case_id": case.get("case_id"),
        "category": case.get("category"),
        "passed": passed,
        "score": round(score, 4),
        "elapsed_ms": elapsed_ms,
        "request": request,
        "response": response,
        "checks": checks,
    }


def build_request(case: dict[str, Any], scope_by_id: dict[str, dict[str, Any]], args: argparse.Namespace) -> dict[str, Any]:
    fixture_scope = scope_by_id.get(case.get("scope_id"), {})
    agent_id = args.agent_id or fixture_scope.get("agent_id") or case.get("scope_id") or "benchmark-agent"
    scope = json.loads(args.scope_json) if args.scope_json else DEFAULT_SCOPE
    return {
        "agent_id": agent_id,
        "query": case.get("query", ""),
        "limit": args.limit,
        "include_trace": bool(args.include_trace),
        "scope": scope,
    }


def post_json_with_retries(
    url: str, payload: dict[str, Any], timeout: float, retries: int, retry_delay: float
) -> tuple[dict[str, Any] | None, str | None]:
    attempts = retries + 1
    last_error = None
    for attempt in range(attempts):
        response, error = post_json(url, payload, timeout)
        if error is None:
            return response, None
        last_error = error
        if attempt < attempts - 1:
            time.sleep(retry_delay)
    return None, last_error


def post_json(url: str, payload: dict[str, Any], timeout: float) -> tuple[dict[str, Any] | None, str | None]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read().decode("utf-8")
            return json.loads(raw), None
    except urllib.error.HTTPError as exc:
        raw = exc.read().decode("utf-8", errors="replace")
        return None, f"HTTP {exc.code}: {raw}"
    except (urllib.error.URLError, TimeoutError) as exc:
        return None, str(exc)
    except json.JSONDecodeError as exc:
        return None, f"Invalid JSON response: {exc}"


def evaluate(expected: dict[str, Any], response: dict[str, Any]) -> list[dict[str, Any]]:
    context = extract_context(response)
    text_context = dict(context)
    text_context.pop("query", None)
    text_context.pop("scope", None)
    response_text = normalize_text(collect_strings(text_context))
    evidence_refs = set(extract_evidence_refs(context))
    redaction_keys = set(extract_redaction_keys(context))
    answer_kind = extract_answer_kind(context, response_text)
    abstention_reason = normalize_reason(context.get("abstention_reason"))

    checks = [
        check_equal("answer_kind", expected.get("answer_kind"), answer_kind),
        check_terms_present("must_include", expected.get("must_include", []), response_text),
        check_terms_absent("must_not_include", expected.get("must_not_include", []), response_text),
        check_subset("evidence_ids", expected.get("evidence_ids", []), evidence_refs),
    ]

    if expected.get("abstention_reason") is not None:
        checks.append(
            check_equal(
                "abstention_reason",
                normalize_reason(expected.get("abstention_reason")),
                abstention_reason,
            )
        )

    expected_redaction_keys = [item.get("key") for item in expected.get("redactions", []) if item.get("key")]
    if expected_redaction_keys:
        checks.append(check_subset("redactions", expected_redaction_keys, redaction_keys))

    return checks


def extract_context(response: dict[str, Any]) -> dict[str, Any]:
    if isinstance(response.get("answer_context"), dict):
        return response["answer_context"]
    return response


def extract_answer_kind(context: dict[str, Any], response_text: str) -> str:
    explicit = context.get("answer_kind") or context.get("kind")
    if explicit:
        return normalize_reason(explicit)
    if context.get("should_abstain") is True:
        return "abstain"
    caveat_terms = ("conflict note", "contradictory", "newer evidence", "superseded")
    if any(term in response_text for term in caveat_terms):
        return "answer_with_caveat"
    return "answer"


def normalize_reason(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value)
    out = []
    for index, char in enumerate(text):
        if char.isupper() and index > 0 and (text[index - 1].islower() or text[index - 1].isdigit()):
            out.append("_")
        elif char in " -":
            out.append("_")
            continue
        out.append(char.lower())
    return "".join(out)


def collect_strings(value: Any) -> str:
    parts = []
    walk(value, lambda item: parts.append(item) if isinstance(item, str) else None)
    return "\n".join(parts)


def extract_evidence_refs(context: dict[str, Any]) -> list[str]:
    refs = []

    def visit(value: Any) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                if key in {"evidence_refs", "evidence_ids"} and isinstance(child, list):
                    refs.extend(str(item) for item in child)
                else:
                    visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)

    visit(context)
    return refs


def extract_redaction_keys(context: dict[str, Any]) -> list[str]:
    keys = []

    def visit(value: Any) -> None:
        if isinstance(value, dict):
            if "redactions" in value:
                keys.extend(redaction_keys_from(value["redactions"]))
            for child in value.values():
                visit(child)
        elif isinstance(value, list):
            for child in value:
                visit(child)

    visit(context)
    return keys


def redaction_keys_from(value: Any) -> list[str]:
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        keys = []
        for item in value:
            if isinstance(item, str):
                keys.append(item)
            elif isinstance(item, dict):
                key = item.get("key") or item.get("field") or item.get("name")
                if key:
                    keys.append(str(key))
        return keys
    if isinstance(value, dict):
        key = value.get("key") or value.get("field") or value.get("name")
        return [str(key)] if key else []
    return []


def walk(value: Any, visit) -> None:
    visit(value)
    if isinstance(value, dict):
        for child in value.values():
            walk(child, visit)
    elif isinstance(value, list):
        for child in value:
            walk(child, visit)


def normalize_text(value: str) -> str:
    return " ".join(value.lower().split())


def check_equal(name: str, expected: Any, actual: Any) -> dict[str, Any]:
    return {
        "name": name,
        "passed": expected == actual,
        "expected": expected,
        "actual": actual,
    }


def check_terms_present(name: str, expected_terms: Iterable[str], response_text: str) -> dict[str, Any]:
    missing = [term for term in expected_terms if term.lower() not in response_text]
    return {
        "name": name,
        "passed": not missing,
        "missing": missing,
    }


def check_terms_absent(name: str, excluded_terms: Iterable[str], response_text: str) -> dict[str, Any]:
    present = [term for term in excluded_terms if term.lower() in response_text]
    return {
        "name": name,
        "passed": not present,
        "present": present,
    }


def check_subset(name: str, expected_values: Iterable[str], actual_values: set[str]) -> dict[str, Any]:
    expected = set(expected_values)
    missing = sorted(expected - actual_values)
    return {
        "name": name,
        "passed": not missing,
        "missing": missing,
        "actual": sorted(actual_values),
    }


def summarize(results: list[dict[str, Any]]) -> dict[str, Any]:
    total = len(results)
    passed = sum(1 for result in results if result["passed"])
    return {
        "total": total,
        "passed": passed,
        "failed": total - passed,
        "score": passed / total if total else 0.0,
    }


def format_case_result(result: dict[str, Any]) -> str:
    status = "PASS" if result["passed"] else "FAIL"
    failed_checks = [check["name"] for check in result.get("checks", []) if not check.get("passed")]
    suffix = "" if not failed_checks else f" failed_checks={','.join(failed_checks)}"
    return f"[{status}] {result['case_id']} score={result['score']:.0%} elapsed={result['elapsed_ms']}ms{suffix}"


if __name__ == "__main__":
    raise SystemExit(main())
