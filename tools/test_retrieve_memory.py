#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import retrieve_memory


class RetrieveMemoryTests(unittest.TestCase):
    def test_build_request_uses_repo_scope_when_available(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            original_discover = retrieve_memory.discover_repo_scope
            retrieve_memory.discover_repo_scope = lambda _cwd: {
                "kind": "repo",
                "repo_path": str(root),
                "repo_remote": "https://github.com/example/yena.git",
                "branch": "main",
            }
            try:
                request = retrieve_memory.build_request(
                    argparse.Namespace(
                        agent_id="agent",
                        query="What database did we choose?",
                        limit=5,
                        include_trace=True,
                        scope="repo",
                        scope_json=None,
                    ),
                    root,
                )
            finally:
                retrieve_memory.discover_repo_scope = original_discover

            self.assertEqual(request["agent_id"], "agent")
            self.assertEqual(request["query"], "What database did we choose?")
            self.assertEqual(request["limit"], 5)
            self.assertTrue(request["include_trace"])
            self.assertEqual(request["scope"]["kind"], "repo")
            self.assertEqual(request["scope"]["repo_path"], str(root))

    def test_build_request_accepts_custom_scope_json(self) -> None:
        request = retrieve_memory.build_request(
            argparse.Namespace(
                agent_id="agent",
                query="token storage",
                limit=3,
                include_trace=False,
                scope="custom",
                scope_json=json.dumps({"kind": "workspace", "workspace_path": "/tmp/ws"}),
            ),
            Path.cwd(),
        )

        self.assertEqual(request["scope"]["kind"], "workspace")
        self.assertEqual(request["scope"]["workspace_path"], "/tmp/ws")

    def test_format_response_handles_abstention(self) -> None:
        formatted = retrieve_memory.format_response(
            {
                "answer_context": {
                    "should_abstain": True,
                    "abstention_reason": "missing_evidence",
                    "abstention_message": "No evidence.",
                    "memories": [],
                }
            }
        )

        self.assertEqual(formatted, "ABSTAIN (missing_evidence): No evidence.")

    def test_format_response_lists_memories(self) -> None:
        formatted = retrieve_memory.format_response(
            {
                "answer_context": {
                    "should_abstain": False,
                    "memories": [
                        {
                            "memory_type": "project",
                            "confidence": 0.82,
                            "evidence_refs": ["e1", "e2"],
                            "redactions": ["email"],
                            "statement": "Use SQLite.",
                        }
                    ],
                }
            }
        )

        self.assertIn("Yena returned 1 memory item(s).", formatted)
        self.assertIn("[project confidence=0.82 evidence=2, redacted=email]", formatted)
        self.assertIn("Use SQLite.", formatted)


if __name__ == "__main__":
    unittest.main()
