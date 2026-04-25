#!/usr/bin/env python3
"""Run the local Markdown import -> retrieval v2 -> audit smoke workflow."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

import forget_import_source
import import_markdown_memory
import list_audit_events
import retrieve_memory


DEFAULT_COMPILER_BIND = "127.0.0.1:18081"
DEFAULT_GATEWAY_BIND = "127.0.0.1:18082"
DEFAULT_AGENT_ID = "yena-smoke"


def base_url(bind: str) -> str:
    return f"http://{bind}"


def fixture_markdown() -> str:
    return "\n".join(
        [
            "# Decisions",
            "",
            "- Use SQLite for local-first storage.",
            "- Audit every retrieval event before dashboard work.",
            "",
        ]
    )


def start_service(
    package: str,
    *,
    db_path: Path,
    bind: str,
    cwd: Path,
) -> subprocess.Popen[str]:
    env = os.environ.copy()
    env["YENA_DB_PATH"] = str(db_path)
    env["YENA_BIND"] = bind
    return subprocess.Popen(
        ["cargo", "run", "-p", package],
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )


def stop_service(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def read_process_output(process: subprocess.Popen[str]) -> str:
    if process.stdout is None:
        return ""
    try:
        return process.stdout.read()
    except ValueError:
        return ""


def wait_for_health(base: str, process: subprocess.Popen[str], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    url = base.rstrip("/") + "/health"
    last_error = "not attempted"

    while time.monotonic() < deadline:
        if process.poll() is not None:
            output = read_process_output(process)
            raise RuntimeError(
                f"service exited before health check succeeded: {output.strip()}"
            )
        try:
            with urllib.request.urlopen(url, timeout=0.5) as response:
                if response.status == 200:
                    return
        except (urllib.error.URLError, TimeoutError) as exc:
            last_error = str(exc)
        time.sleep(0.1)

    raise RuntimeError(f"timed out waiting for {url}: {last_error}")


def find_repo_root(cwd: Path) -> Path:
    current = cwd.resolve()
    for candidate in [current, *current.parents]:
        if (candidate / "Cargo.toml").is_file() and (candidate / "tools").is_dir():
            return candidate
    return current


def import_fixture(markdown_path: Path, compiler_url: str, timeout: float) -> dict[str, Any]:
    args = argparse.Namespace(
        files=[str(markdown_path)],
        compiler_url=compiler_url,
        source_type="local_markdown_memory",
        commit=True,
        confidence=0.84,
        scope="none",
        timeout=timeout,
        dry_run=False,
        json=True,
    )
    results = import_markdown_memory.import_files(args)
    if not results:
        raise RuntimeError("import returned no results")
    return results[0]


def retrieve_fixture(gateway_url: str, agent_id: str, timeout: float) -> dict[str, Any]:
    args = argparse.Namespace(
        gateway_url=gateway_url,
        agent_id=agent_id,
        query="What storage did we choose?",
        limit=5,
        scope="global",
        scope_json=None,
        include_trace=True,
        timeout=timeout,
        dry_run=False,
        json=True,
    )
    return retrieve_memory.retrieve(args)


def list_retrieval_audits(gateway_url: str, agent_id: str, timeout: float) -> dict[str, Any]:
    args = argparse.Namespace(
        gateway_url=gateway_url,
        limit=10,
        agent_id=agent_id,
        request_type="retrieve_v2",
        timeout=timeout,
        dry_run=False,
        json=True,
    )
    return list_audit_events.list_events(args)


def forget_fixture_source(markdown_path: Path, compiler_url: str, timeout: float) -> dict[str, Any]:
    args = argparse.Namespace(
        source=str(markdown_path),
        compiler_url=compiler_url,
        source_type="local_markdown_memory",
        keep_evidence=False,
        scope="none",
        literal=False,
        timeout=timeout,
        dry_run=False,
        json=True,
    )
    return forget_import_source.forget_source(args)


def validate_smoke_outputs(
    import_response: dict[str, Any],
    retrieval_response: dict[str, Any],
    audit_response: dict[str, Any],
    forget_response: dict[str, Any] | None = None,
) -> None:
    if import_response.get("committed_items", 0) < 1:
        raise RuntimeError(f"expected committed import items, got: {import_response}")

    answer = retrieval_response.get("answer_context", {})
    memories = answer.get("memories", [])
    statements = [memory.get("statement", "") for memory in memories]
    if answer.get("should_abstain"):
        raise RuntimeError(f"retrieval abstained unexpectedly: {retrieval_response}")
    if not any("SQLite" in statement for statement in statements):
        raise RuntimeError(f"retrieval did not return SQLite memory: {retrieval_response}")

    events = audit_response.get("events", [])
    if not events:
        raise RuntimeError(f"expected retrieve_v2 audit events, got: {audit_response}")
    if events[0].get("request_type") != "retrieve_v2":
        raise RuntimeError(f"latest audit event was not retrieve_v2: {events[0]}")

    if forget_response is not None:
        if forget_response.get("deleted_memory_items", 0) < 1:
            raise RuntimeError(f"expected source forget to delete memories: {forget_response}")
        if forget_response.get("deleted_evidence", 0) < 1:
            raise RuntimeError(f"expected source forget to delete evidence: {forget_response}")


def run_smoke(args: argparse.Namespace) -> dict[str, Any]:
    repo_root = find_repo_root(Path.cwd())
    generated_db_path = args.db_path is None
    generated_markdown_path = args.markdown_path is None
    db_path = args.db_path or Path(tempfile.gettempdir()) / f"yena-dev-memory-smoke-{os.getpid()}.db"
    markdown_path = args.markdown_path or Path(tempfile.gettempdir()) / f"yena-dev-memory-smoke-{os.getpid()}.md"
    if db_path.exists():
        raise RuntimeError(f"refusing to modify existing DB path: {db_path}")
    if markdown_path.exists():
        raise RuntimeError(f"refusing to overwrite existing Markdown path: {markdown_path}")
    markdown_path.write_text(fixture_markdown(), encoding="utf-8")

    compiler_url = base_url(args.compiler_bind)
    gateway_url = base_url(args.gateway_bind)
    compiler: subprocess.Popen[str] | None = None
    gateway: subprocess.Popen[str] | None = None
    forget_response: dict[str, Any] | None = None

    try:
        compiler = start_service(
            "memory-compiler",
            db_path=db_path,
            bind=args.compiler_bind,
            cwd=repo_root,
        )
        wait_for_health(compiler_url, compiler, args.startup_timeout)
        import_response = import_fixture(markdown_path, compiler_url, args.timeout)
        stop_service(compiler)
        compiler = None

        gateway = start_service(
            "mcp-gateway",
            db_path=db_path,
            bind=args.gateway_bind,
            cwd=repo_root,
        )
        wait_for_health(gateway_url, gateway, args.startup_timeout)
        retrieval_response = retrieve_fixture(gateway_url, args.agent_id, args.timeout)
        audit_response = list_retrieval_audits(gateway_url, args.agent_id, args.timeout)
        stop_service(gateway)
        gateway = None

        compiler = start_service(
            "memory-compiler",
            db_path=db_path,
            bind=args.compiler_bind,
            cwd=repo_root,
        )
        wait_for_health(compiler_url, compiler, args.startup_timeout)
        forget_response = forget_fixture_source(markdown_path, compiler_url, args.timeout)
        validate_smoke_outputs(
            import_response,
            retrieval_response,
            audit_response,
            forget_response,
        )
    finally:
        if compiler is not None:
            stop_service(compiler)
        if gateway is not None:
            stop_service(gateway)
        if not args.keep_files:
            if generated_db_path:
                db_path.unlink(missing_ok=True)
            if generated_markdown_path:
                markdown_path.unlink(missing_ok=True)

    return {
        "db_path": str(db_path),
        "markdown_path": str(markdown_path),
        "import": import_response,
        "retrieval": retrieval_response,
        "audit": audit_response,
        "forget": forget_response,
    }


def format_summary(report: dict[str, Any]) -> str:
    import_response = report["import"]
    answer = report["retrieval"]["answer_context"]
    audit = report["audit"]
    forget = report["forget"]
    first_memory = answer["memories"][0]
    return "\n".join(
        [
            "Yena dev-memory smoke passed.",
            "import: committed={committed} skipped={skipped} job={job}".format(
                committed=import_response.get("committed_items"),
                skipped=import_response.get("skipped_items"),
                job=import_response.get("job_id"),
            ),
            "retrieval: {statement}".format(statement=first_memory.get("statement")),
            "audit: events={count} latest={latest}".format(
                count=audit.get("returned"),
                latest=audit.get("events", [{}])[0].get("request_type"),
            ),
            "forget: deleted_memories={memories} deleted_evidence={evidence}".format(
                memories=forget.get("deleted_memory_items"),
                evidence=forget.get("deleted_evidence"),
            ),
        ]
    )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the local Markdown import -> retrieval v2 -> audit smoke workflow."
    )
    parser.add_argument("--compiler-bind", default=DEFAULT_COMPILER_BIND)
    parser.add_argument("--gateway-bind", default=DEFAULT_GATEWAY_BIND)
    parser.add_argument("--agent-id", default=DEFAULT_AGENT_ID)
    parser.add_argument("--db-path", type=Path)
    parser.add_argument("--markdown-path", type=Path)
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--startup-timeout", type=float, default=20.0)
    parser.add_argument(
        "--keep-files",
        action="store_true",
        help="Keep the temporary DB and Markdown fixture after the run.",
    )
    parser.add_argument("--json", action="store_true", help="Print the full smoke report.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    try:
        report = run_smoke(args)
    except RuntimeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(format_summary(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
