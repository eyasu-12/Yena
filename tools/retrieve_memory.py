#!/usr/bin/env python3
"""Query Yena retrieval v2 from the command line."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_GATEWAY_URL = "http://127.0.0.1:8082"
DEFAULT_AGENT_ID = "yena-cli"


def run_git(cwd: Path, args: list[str]) -> str | None:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=cwd,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
        )
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    value = result.stdout.strip()
    return value or None


def discover_repo_scope(cwd: Path) -> dict[str, str] | None:
    repo_root = run_git(cwd, ["rev-parse", "--show-toplevel"])
    if not repo_root:
        return None

    root = Path(repo_root).resolve()
    scope = {
        "kind": "repo",
        "repo_path": str(root),
    }
    remote = run_git(root, ["config", "--get", "remote.origin.url"])
    if remote:
        scope["repo_remote"] = remote
    branch = run_git(root, ["rev-parse", "--abbrev-ref", "HEAD"])
    if branch and branch != "HEAD":
        scope["branch"] = branch
    return scope


def build_request(args: argparse.Namespace, cwd: Path) -> dict[str, Any]:
    request: dict[str, Any] = {
        "agent_id": args.agent_id,
        "query": args.query,
        "limit": args.limit,
        "include_trace": args.include_trace,
    }

    if args.scope == "repo":
        request["scope"] = discover_repo_scope(cwd) or {"kind": "global"}
    elif args.scope == "global":
        request["scope"] = {"kind": "global"}
    elif args.scope_json:
        request["scope"] = json.loads(args.scope_json)

    return request


def post_json(url: str, payload: dict[str, Any], timeout: float) -> dict[str, Any]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={"content-type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        details = exc.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"gateway returned HTTP {exc.code}: {details}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"failed to reach MCP gateway at {url}: {exc.reason}") from exc


def retrieve(args: argparse.Namespace) -> dict[str, Any]:
    endpoint = args.gateway_url.rstrip("/") + "/v2/retrieve"
    request = build_request(args, Path.cwd())
    if args.dry_run:
        return {"request": request}
    return post_json(endpoint, request, args.timeout)


def format_response(response: dict[str, Any]) -> str:
    answer = response.get("answer_context", {})
    memories = answer.get("memories", [])
    lines: list[str] = []

    if answer.get("should_abstain"):
        message = answer.get("abstention_message") or "Yena abstained."
        reason = answer.get("abstention_reason") or "unknown"
        lines.append(f"ABSTAIN ({reason}): {message}")
        if not memories:
            return "\n".join(lines)
        lines.append("Supporting memory:")
    else:
        lines.append(f"Yena returned {len(memories)} memory item(s).")

    for index, memory in enumerate(memories, start=1):
        confidence = memory.get("confidence")
        confidence_text = f"{confidence:.2f}" if isinstance(confidence, (int, float)) else "n/a"
        evidence_count = len(memory.get("evidence_refs", []))
        redactions = memory.get("redactions", [])
        redaction_text = f", redacted={','.join(redactions)}" if redactions else ""
        lines.append(
            "{idx}. [{memory_type} confidence={confidence} evidence={evidence}{redactions}] {statement}".format(
                idx=index,
                memory_type=memory.get("memory_type", "unknown"),
                confidence=confidence_text,
                evidence=evidence_count,
                redactions=redaction_text,
                statement=memory.get("statement", ""),
            )
        )
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Query Yena /v2/retrieve.")
    parser.add_argument("query", help="Question or search text to send to Yena.")
    parser.add_argument(
        "--gateway-url",
        default=os.environ.get("YENA_MCP_GATEWAY_URL", DEFAULT_GATEWAY_URL),
        help=f"MCP gateway base URL. Default: {DEFAULT_GATEWAY_URL}",
    )
    parser.add_argument(
        "--agent-id",
        default=DEFAULT_AGENT_ID,
        help=f"Agent id used for retrieval and audit logs. Default: {DEFAULT_AGENT_ID}",
    )
    parser.add_argument("--limit", type=int, default=8, help="Maximum memories to return.")
    parser.add_argument(
        "--scope",
        choices=["repo", "global", "custom"],
        default="repo",
        help="Retrieval scope. Default: repo, falling back to global outside git.",
    )
    parser.add_argument(
        "--scope-json",
        help="Custom JSON scope object. Requires --scope custom.",
    )
    parser.add_argument(
        "--include-trace",
        action="store_true",
        help="Request redaction-safe retrieval trace fields.",
    )
    parser.add_argument("--timeout", type=float, default=10.0, help="HTTP timeout in seconds.")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the request without sending it to the gateway.",
    )
    parser.add_argument("--json", action="store_true", help="Print full JSON response.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if args.limit < 1:
        print("limit must be at least 1", file=sys.stderr)
        return 2
    if args.scope_json and args.scope != "custom":
        print("--scope-json requires --scope custom", file=sys.stderr)
        return 2
    if args.scope == "custom" and not args.scope_json:
        print("--scope custom requires --scope-json", file=sys.stderr)
        return 2

    try:
        response = retrieve(args)
    except (json.JSONDecodeError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.json or args.dry_run:
        print(json.dumps(response, indent=2, sort_keys=True))
    else:
        print(format_response(response))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
