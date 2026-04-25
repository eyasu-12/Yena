#!/usr/bin/env python3
"""Forget memories imported from a Markdown source."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

import import_markdown_memory


DEFAULT_COMPILER_URL = "http://127.0.0.1:8081"
DEFAULT_SOURCE_TYPE = "local_markdown_memory"


def resolve_source_ref(source: str, *, scope: str, literal: bool, cwd: Path) -> str:
    if literal:
        return source.strip()

    path = Path(source).expanduser()
    if not path.exists():
        return source.strip()

    repo_scope = import_markdown_memory.discover_repo_scope(path) if scope == "repo" else None
    return import_markdown_memory.source_ref_for(path, repo_scope, cwd)


def build_request(args: argparse.Namespace, cwd: Path) -> dict[str, Any]:
    request = {
        "source_ref": resolve_source_ref(
            args.source,
            scope=args.scope,
            literal=args.literal,
            cwd=cwd,
        ),
        "forget_evidence": not args.keep_evidence,
    }
    if args.source_type:
        request["source_type"] = args.source_type
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
        raise RuntimeError(f"compiler returned HTTP {exc.code}: {details}") from exc
    except urllib.error.URLError as exc:
        raise RuntimeError(f"failed to reach memory compiler at {url}: {exc.reason}") from exc


def forget_source(args: argparse.Namespace) -> dict[str, Any]:
    endpoint = args.compiler_url.rstrip("/") + "/v1/import/sources/forget"
    request = build_request(args, Path.cwd())
    if args.dry_run:
        return {"request": request}
    return post_json(endpoint, request, args.timeout)


def format_response(response: dict[str, Any]) -> str:
    return (
        "forgot {source_ref}: matched={matched_memory_items} "
        "deleted_memories={deleted_memory_items} deleted_versions={deleted_versions} "
        "deleted_proposals={deleted_proposals} deleted_evidence={deleted_evidence}"
    ).format(
        source_ref=response.get("source_ref", ""),
        matched_memory_items=response.get("matched_memory_items", 0),
        deleted_memory_items=response.get("deleted_memory_items", 0),
        deleted_versions=response.get("deleted_versions", 0),
        deleted_proposals=response.get("deleted_proposals", 0),
        deleted_evidence=response.get("deleted_evidence", 0),
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Forget memories imported from a Markdown source."
    )
    parser.add_argument(
        "source",
        help="Source path/ref to revoke. Existing paths are resolved like import_markdown_memory.py unless --literal is set.",
    )
    parser.add_argument(
        "--compiler-url",
        default=os.environ.get("YENA_MEMORY_COMPILER_URL", DEFAULT_COMPILER_URL),
        help=f"Memory compiler base URL. Default: {DEFAULT_COMPILER_URL}",
    )
    parser.add_argument(
        "--source-type",
        default=DEFAULT_SOURCE_TYPE,
        help=f"Import source type filter. Default: {DEFAULT_SOURCE_TYPE}",
    )
    parser.add_argument(
        "--all-source-types",
        action="store_const",
        const=None,
        dest="source_type",
        help="Match the source_ref across all import source types.",
    )
    parser.add_argument(
        "--keep-evidence",
        action="store_true",
        help="Detach imported memories/proposals but keep evidence records when possible.",
    )
    parser.add_argument(
        "--scope",
        choices=["repo", "none"],
        default="repo",
        help="How to resolve existing file paths before forgetting. Default: repo",
    )
    parser.add_argument(
        "--literal",
        action="store_true",
        help="Use source exactly as provided instead of resolving an existing path.",
    )
    parser.add_argument("--timeout", type=float, default=10.0, help="HTTP timeout in seconds.")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the request without sending it to the compiler.",
    )
    parser.add_argument("--json", action="store_true", help="Print full JSON response.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    try:
        response = forget_source(args)
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
