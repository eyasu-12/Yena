#!/usr/bin/env python3
"""List Yena retrieval audit events from the command line."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from typing import Any


DEFAULT_GATEWAY_URL = "http://127.0.0.1:8082"


def build_request(args: argparse.Namespace) -> dict[str, Any]:
    request: dict[str, Any] = {"limit": args.limit}
    if args.agent_id:
        request["agent_id"] = args.agent_id
    if args.request_type:
        request["request_type"] = args.request_type
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


def list_events(args: argparse.Namespace) -> dict[str, Any]:
    endpoint = args.gateway_url.rstrip("/") + "/v1/audit/events/list"
    request = build_request(args)
    if args.dry_run:
        return {"request": request}
    return post_json(endpoint, request, args.timeout)


def summarize_json(value: Any) -> str:
    if value is None:
        return "none"
    if isinstance(value, dict):
        preferred = [
            "should_abstain",
            "abstention_reason",
            "memory_count",
            "memory_types",
            "evidence_refs",
            "relationship_ids",
        ]
        parts = []
        for key in preferred:
            if key in value:
                parts.append(f"{key}={json.dumps(value[key], sort_keys=True)}")
        if parts:
            return "; ".join(parts)
    return json.dumps(value, sort_keys=True)


def format_response(response: dict[str, Any]) -> str:
    events = response.get("events", [])
    if not events:
        return "No audit events found."

    lines = [f"Yena returned {len(events)} audit event(s)."]
    for index, event in enumerate(events, start=1):
        lines.append(
            "{idx}. {created_at} [{request_type}] agent={agent_id} scope={scope}".format(
                idx=index,
                created_at=event.get("created_at", "unknown-time"),
                request_type=event.get("request_type", "unknown"),
                agent_id=event.get("agent_id", "unknown"),
                scope=event.get("scope_applied", ""),
            )
        )
        lines.append(f"   shared: {summarize_json(event.get('shared_json'))}")
        lines.append(f"   redacted: {summarize_json(event.get('redacted_json'))}")
    return "\n".join(lines)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="List Yena retrieval audit events.")
    parser.add_argument(
        "--gateway-url",
        default=os.environ.get("YENA_MCP_GATEWAY_URL", DEFAULT_GATEWAY_URL),
        help=f"MCP gateway base URL. Default: {DEFAULT_GATEWAY_URL}",
    )
    parser.add_argument("--limit", type=int, default=20, help="Maximum events to return.")
    parser.add_argument("--agent-id", help="Filter by agent id.")
    parser.add_argument(
        "--request-type",
        help="Filter by request type, such as retrieve, retrieve_v2, or graph_retrieve.",
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

    try:
        response = list_events(args)
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
