#!/usr/bin/env python3
"""Import local Markdown memory files into Yena's memory compiler."""

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


DEFAULT_COMPILER_URL = "http://127.0.0.1:8081"
DEFAULT_SOURCE_TYPE = "local_markdown_memory"


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


def discover_repo_scope(path: Path) -> dict[str, str] | None:
    cwd = path.parent if path.is_file() else path
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


def source_ref_for(path: Path, repo_scope: dict[str, str] | None, cwd: Path) -> str:
    resolved = path.resolve()
    if repo_scope:
        try:
            return resolved.relative_to(Path(repo_scope["repo_path"])).as_posix()
        except ValueError:
            pass
    try:
        return resolved.relative_to(cwd.resolve()).as_posix()
    except ValueError:
        return str(resolved)


def build_payload(
    path: Path,
    *,
    source_type: str,
    commit: bool,
    confidence: float,
    include_scope: bool,
    cwd: Path,
) -> dict[str, Any]:
    if not path.is_file():
        raise ValueError(f"not a file: {path}")

    content = path.read_text(encoding="utf-8")
    repo_scope = discover_repo_scope(path) if include_scope else None
    payload: dict[str, Any] = {
        "source_ref": source_ref_for(path, repo_scope, cwd),
        "source_type": source_type,
        "content": content,
        "commit": commit,
        "confidence": confidence,
    }
    if repo_scope:
        payload["scope"] = repo_scope
    return payload


def payload_summary(payload: dict[str, Any]) -> dict[str, Any]:
    return {
        "source_ref": payload["source_ref"],
        "source_type": payload["source_type"],
        "commit": payload["commit"],
        "confidence": payload["confidence"],
        "scope": payload.get("scope"),
        "content_bytes": len(payload["content"].encode("utf-8")),
    }


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


def import_files(args: argparse.Namespace) -> list[dict[str, Any]]:
    endpoint = args.compiler_url.rstrip("/") + "/v1/import/markdown"
    results: list[dict[str, Any]] = []

    for file_arg in args.files:
        path = Path(file_arg).expanduser()
        payload = build_payload(
            path,
            source_type=args.source_type,
            commit=args.commit,
            confidence=args.confidence,
            include_scope=args.scope == "repo",
            cwd=Path.cwd(),
        )
        if args.dry_run:
            results.append(payload_summary(payload))
        else:
            response = post_json(endpoint, payload, args.timeout)
            response["source_ref"] = response.get("source_ref", payload["source_ref"])
            results.append(response)
    return results


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Import local Markdown memory files into Yena."
    )
    parser.add_argument("files", nargs="+", help="Markdown memory files to import.")
    parser.add_argument(
        "--compiler-url",
        default=os.environ.get("YENA_MEMORY_COMPILER_URL", DEFAULT_COMPILER_URL),
        help=f"Memory compiler base URL. Default: {DEFAULT_COMPILER_URL}",
    )
    parser.add_argument(
        "--source-type",
        default=DEFAULT_SOURCE_TYPE,
        help=f"Import source type. Default: {DEFAULT_SOURCE_TYPE}",
    )
    parser.add_argument(
        "--pending",
        action="store_false",
        dest="commit",
        help="Create pending proposals instead of committing imported memories.",
    )
    parser.add_argument(
        "--commit",
        action="store_true",
        dest="commit",
        help="Commit imported memories immediately. This is the default.",
    )
    parser.set_defaults(commit=True)
    parser.add_argument(
        "--confidence",
        type=float,
        default=0.74,
        help="Confidence assigned to imported memory proposals. Default: 0.74",
    )
    parser.add_argument(
        "--scope",
        choices=["repo", "none"],
        default="repo",
        help="Attach git repo scope when available. Default: repo",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=10.0,
        help="HTTP timeout in seconds. Default: 10",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print request summaries without sending them to the compiler.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print full JSON output instead of one-line summaries.",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    if not (0.0 <= args.confidence <= 1.0):
        print("confidence must be between 0.0 and 1.0", file=sys.stderr)
        return 2

    try:
        results = import_files(args)
    except (OSError, ValueError, RuntimeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.json or args.dry_run:
        print(json.dumps(results, indent=2, sort_keys=True))
        return 0

    for result in results:
        print(
            "imported {source_ref}: job={job_id} imported={imported_items} "
            "committed={committed_items} skipped={skipped_items}".format(**result)
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
