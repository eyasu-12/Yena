#!/usr/bin/env python3
"""Load the developer-memory benchmark seed into a local Yena SQLite DB."""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

DEFAULT_SEED = Path(__file__).with_name("developer_memory_seed.json")
REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MIGRATIONS_DIR = REPO_ROOT / "db" / "migrations"
DEFAULT_DB = REPO_ROOT / "data" / "yena-benchmark.db"
NOW = "2026-04-24T00:00:00Z"


def main() -> int:
    args = parse_args()
    seed = load_json(args.seed)
    if args.reset and args.db.exists():
        args.db.unlink()
    args.db.parent.mkdir(parents=True, exist_ok=True)

    conn = sqlite3.connect(args.db)
    conn.execute("PRAGMA foreign_keys = ON")
    try:
        apply_migrations(conn, args.migrations_dir)
        counts = load_seed(conn, seed, args)
    finally:
        conn.close()

    print(
        "Loaded {evidence} evidence records, {scopes} agent scopes, "
        "{policies} policies, {memory_items} memory items, {versions} versions, "
        "and {fts_documents} retrieval documents into {db}".format(
            **counts,
            db=args.db,
        )
    )
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Load developer_memory_seed.json fixtures into a local Yena SQLite DB."
    )
    parser.add_argument("--seed", type=Path, default=DEFAULT_SEED, help="Path to seed JSON.")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB, help="Target SQLite DB path.")
    parser.add_argument(
        "--migrations-dir",
        type=Path,
        default=DEFAULT_MIGRATIONS_DIR,
        help="Directory containing db/migrations/*.sql.",
    )
    parser.add_argument(
        "--reset",
        action="store_true",
        help="Delete the target DB before loading fixtures.",
    )
    parser.add_argument(
        "--scope-kind",
        default="global",
        choices=["global", "repo", "workspace"],
        help="Scope kind assigned to loaded memory metadata.",
    )
    parser.add_argument("--repo-path", help="Repo path for --scope-kind repo.")
    parser.add_argument("--repo-remote", help="Repo remote for --scope-kind repo.")
    parser.add_argument("--branch", help="Repo branch for --scope-kind repo.")
    parser.add_argument("--workspace-path", help="Workspace path for --scope-kind workspace.")
    return parser.parse_args()


def load_json(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text())
    except OSError as exc:
        raise SystemExit(f"Failed to read {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise SystemExit(f"Invalid JSON in {path}: {exc}") from exc


def apply_migrations(conn: sqlite3.Connection, migrations_dir: Path) -> None:
    ordered = [
        "0001_init.sql",
        "0002_indexes.sql",
        "0003_knowledge_graph.sql",
        "0004_graph_confidence.sql",
        "0005_graph_canonicalization.sql",
        "0006_retrieval_v2_foundation.sql",
    ]
    for migration in ordered:
        if migration == "0004_graph_confidence.sql":
            ensure_graph_confidence_column(conn)
        path = migrations_dir / migration
        try:
            conn.executescript(path.read_text())
        except OSError as exc:
            raise SystemExit(f"Failed to read migration {path}: {exc}") from exc


def ensure_graph_confidence_column(conn: sqlite3.Connection) -> None:
    columns = {row[1] for row in conn.execute("PRAGMA table_info(graph_relationship_versions)")}
    if "confidence" not in columns:
        conn.execute(
            "ALTER TABLE graph_relationship_versions "
            "ADD COLUMN confidence REAL NOT NULL DEFAULT 1.0"
        )


def load_seed(
    conn: sqlite3.Connection, seed: dict[str, Any], args: argparse.Namespace
) -> dict[str, int]:
    fixtures = seed.get("fixtures", {})
    evidence_by_id = load_evidence(conn, fixtures.get("evidence_records", []))
    policy_count = load_policies(conn, fixtures.get("policies", []))
    scope_count = load_agent_scopes(conn, fixtures.get("agent_scopes", []))
    memory_counts = load_memories(
        conn,
        fixtures.get("memory_items", []),
        evidence_by_id,
        scope_payload_from_args(args),
    )
    conn.commit()
    return {
        "evidence": len(evidence_by_id),
        "scopes": scope_count,
        "policies": policy_count,
        **memory_counts,
    }


def load_evidence(
    conn: sqlite3.Connection, evidence_records: list[dict[str, Any]]
) -> dict[str, dict[str, Any]]:
    evidence_by_id = {}
    for evidence in evidence_records:
        evidence_id = require(evidence, "evidence_id")
        evidence_by_id[evidence_id] = evidence
        captured_at = evidence.get("captured_at") or NOW
        conn.execute(
            """
            INSERT OR REPLACE INTO evidence_records (
              id, source_type, source_ref, content_type, content, created_at, ingested_at, checksum
            ) VALUES (?, ?, ?, 'text/plain', ?, ?, ?, ?)
            """,
            (
                evidence_id,
                require(evidence, "source_type"),
                require(evidence, "source_ref"),
                evidence.get("summary", ""),
                captured_at,
                captured_at,
                require(evidence, "checksum"),
            ),
        )
    return evidence_by_id


def load_policies(conn: sqlite3.Connection, policies: list[dict[str, Any]]) -> int:
    count = 0
    for policy in policies:
        redact_keys = policy.get("redact_keys") or []
        conn.execute(
            """
            INSERT OR REPLACE INTO policy_rules (
              id, rule_name, rule_json, enabled, created_at, updated_at
            ) VALUES (?, 'redact_keys', ?, 1, ?, ?)
            """,
            (
                policy.get("policy_id") or "policy-redact-keys",
                json.dumps({"keys": redact_keys}, sort_keys=True),
                NOW,
                NOW,
            ),
        )
        count += 1
    return count


def load_agent_scopes(conn: sqlite3.Connection, scopes: list[dict[str, Any]]) -> int:
    count = 0
    for scope in scopes:
        conn.execute(
            """
            INSERT OR REPLACE INTO agent_scopes (
              id, agent_id, scope_name, scope_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            """,
            (
                require(scope, "scope_id"),
                require(scope, "agent_id"),
                require(scope, "scope_id"),
                json.dumps(
                    {"allowed_memory_types": scope.get("allowed_memory_types", [])},
                    sort_keys=True,
                ),
                NOW,
                NOW,
            ),
        )
        count += 1
    return count


def load_memories(
    conn: sqlite3.Connection,
    memories: list[dict[str, Any]],
    evidence_by_id: dict[str, dict[str, Any]],
    scope: dict[str, str | None],
) -> dict[str, int]:
    grouped = defaultdict(list)
    for memory in memories:
        grouped[require(memory, "canonical_key")].append(memory)

    memory_item_count = 0
    version_count = 0
    fts_count = 0
    for canonical_key, versions in grouped.items():
        ordered_versions = sorted(versions, key=lambda item: item.get("valid_from") or "")
        active_memory = next(
            (memory for memory in ordered_versions if memory.get("status") == "active"),
            ordered_versions[-1],
        )
        memory_item_id = require(active_memory, "memory_id")
        active_version_id = version_id(active_memory)
        item_status = active_memory.get("status") or "active"
        created_at = ordered_versions[0].get("valid_from") or NOW
        updated_at = active_memory.get("valid_from") or NOW

        conn.execute(
            """
            INSERT OR REPLACE INTO memory_items (
              id, memory_type, canonical_key, active_version_id, status, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            """,
            (
                memory_item_id,
                require(active_memory, "memory_type"),
                canonical_key,
                active_version_id,
                item_status,
                created_at,
                updated_at,
            ),
        )
        memory_item_count += 1

        for index, memory in enumerate(ordered_versions, start=1):
            state = "active" if require(memory, "memory_id") == require(active_memory, "memory_id") else "superseded"
            conn.execute(
                """
                INSERT OR REPLACE INTO memory_item_versions (
                  id, memory_item_id, version_number, state, value_json,
                  supersedes_version_id, valid_from, valid_to, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    version_id(memory),
                    memory_item_id,
                    index,
                    state,
                    json.dumps(memory_value(memory), sort_keys=True),
                    supersedes_version_id(ordered_versions, index),
                    memory.get("valid_from") or NOW,
                    memory.get("valid_until"),
                    memory.get("valid_from") or NOW,
                ),
            )
            version_count += 1
            for evidence_id in memory.get("evidence_ids", []):
                if evidence_id not in evidence_by_id:
                    raise SystemExit(
                        f"Memory {memory.get('memory_id')} references unknown evidence {evidence_id}"
                    )
                conn.execute(
                    """
                    INSERT OR REPLACE INTO memory_links (
                      id, memory_item_version_id, evidence_record_id, link_type, created_at
                    ) VALUES (?, ?, ?, 'supporting_evidence', ?)
                    """,
                    (
                        f"link-{version_id(memory)}-{evidence_id}",
                        version_id(memory),
                        evidence_id,
                        memory.get("valid_from") or NOW,
                    ),
                )

        upsert_memory_metadata(conn, memory_item_id, active_memory, scope, updated_at)
        upsert_fts_document(conn, memory_item_id, canonical_key, active_memory, evidence_by_id, scope)
        fts_count += 1

    return {
        "memory_items": memory_item_count,
        "versions": version_count,
        "fts_documents": fts_count,
    }


def upsert_memory_metadata(
    conn: sqlite3.Connection,
    memory_item_id: str,
    memory: dict[str, Any],
    scope: dict[str, str | None],
    updated_at: str,
) -> None:
    conn.execute(
        """
        INSERT OR REPLACE INTO memory_item_metadata (
          memory_item_id, scope_kind, repo_path, repo_remote, branch, workspace_path,
          sensitivity, freshness, confidence, decay_policy, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'normal', ?, ?, NULL, ?, ?)
        """,
        (
            memory_item_id,
            scope["scope_kind"],
            scope["repo_path"],
            scope["repo_remote"],
            scope["branch"],
            scope["workspace_path"],
            freshness_for(memory),
            float(memory.get("confidence", 1.0)),
            memory.get("valid_from") or NOW,
            updated_at,
        ),
    )


def upsert_fts_document(
    conn: sqlite3.Connection,
    memory_item_id: str,
    canonical_key: str,
    memory: dict[str, Any],
    evidence_by_id: dict[str, dict[str, Any]],
    scope: dict[str, str | None],
) -> None:
    body_parts = [
        require(memory, "memory_type"),
        canonical_key,
        json.dumps(memory_value(memory), sort_keys=True),
    ]
    for evidence_id in memory.get("evidence_ids", []):
        summary = evidence_by_id[evidence_id].get("summary")
        if summary:
            body_parts.append(summary)

    conn.execute(
        "DELETE FROM retrieval_documents_fts WHERE source_type = ? AND source_id = ?",
        ("memory_item", memory_item_id),
    )
    conn.execute(
        """
        INSERT INTO retrieval_documents_fts (
          source_type, source_id, scope_kind, repo_path, repo_remote, branch, title, body
        ) VALUES ('memory_item', ?, ?, ?, ?, ?, ?, ?)
        """,
        (
            memory_item_id,
            scope["scope_kind"],
            scope["repo_path"],
            scope["repo_remote"],
            scope["branch"],
            canonical_key,
            " ".join(body_parts),
        ),
    )


def memory_value(memory: dict[str, Any]) -> dict[str, Any]:
    content = dict(memory.get("content") or {})
    content.setdefault("statement", statement_for(memory))
    return content


def statement_for(memory: dict[str, Any]) -> str:
    content = memory.get("content") or {}
    for key in ("statement", "decision", "rejected_approach", "convention", "error", "preference", "open_question"):
        value = content.get(key)
        if isinstance(value, str) and value.strip():
            if key == "rejected_approach" and content.get("reason"):
                return f"{value} Rejected because {content['reason']}"
            if key == "error" and content.get("resolution"):
                return f"{value} Resolution: {content['resolution']}"
            return value
    return json.dumps(content, sort_keys=True)


def scope_payload_from_args(args: argparse.Namespace) -> dict[str, str | None]:
    return {
        "scope_kind": args.scope_kind,
        "repo_path": args.repo_path if args.scope_kind == "repo" else None,
        "repo_remote": args.repo_remote if args.scope_kind == "repo" else None,
        "branch": args.branch if args.scope_kind == "repo" else None,
        "workspace_path": args.workspace_path if args.scope_kind == "workspace" else None,
    }


def freshness_for(memory: dict[str, Any]) -> str:
    if memory.get("status") == "stale":
        return "stale"
    return "stable"


def version_id(memory: dict[str, Any]) -> str:
    return f"version-{require(memory, 'memory_id')}"


def supersedes_version_id(versions: list[dict[str, Any]], index: int) -> str | None:
    if index <= 1:
        return None
    return version_id(versions[index - 2])


def require(mapping: dict[str, Any], key: str) -> Any:
    value = mapping.get(key)
    if value in (None, ""):
        raise SystemExit(f"Missing required seed field: {key}")
    return value


if __name__ == "__main__":
    raise SystemExit(main())
