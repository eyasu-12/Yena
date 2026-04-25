#!/usr/bin/env python3

from __future__ import annotations

import argparse
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import forget_import_source


class ForgetImportSourceTests(unittest.TestCase):
    def test_build_request_resolves_existing_repo_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            memory_file = root / "docs" / "AGENTS.md"
            memory_file.parent.mkdir()
            memory_file.write_text("# Memory\n\n- Prefer Rust.", encoding="utf-8")

            original_discover = forget_import_source.import_markdown_memory.discover_repo_scope
            forget_import_source.import_markdown_memory.discover_repo_scope = lambda _path: {
                "kind": "repo",
                "repo_path": str(root),
            }
            try:
                request = forget_import_source.build_request(
                    argparse.Namespace(
                        source=str(memory_file),
                        scope="repo",
                        literal=False,
                        keep_evidence=False,
                        source_type="local_markdown_memory",
                    ),
                    root,
                )
            finally:
                forget_import_source.import_markdown_memory.discover_repo_scope = original_discover

            self.assertEqual(request["source_ref"], "docs/AGENTS.md")
            self.assertEqual(request["source_type"], "local_markdown_memory")
            self.assertTrue(request["forget_evidence"])

    def test_build_request_keeps_literal_source_and_evidence(self) -> None:
        request = forget_import_source.build_request(
            argparse.Namespace(
                source="AGENTS.md",
                scope="repo",
                literal=True,
                keep_evidence=True,
                source_type=None,
            ),
            Path.cwd(),
        )

        self.assertEqual(
            request,
            {
                "source_ref": "AGENTS.md",
                "forget_evidence": False,
            },
        )

    def test_format_response_summarizes_deleted_counts(self) -> None:
        formatted = forget_import_source.format_response(
            {
                "source_ref": "AGENTS.md",
                "matched_memory_items": 2,
                "deleted_memory_items": 2,
                "deleted_versions": 2,
                "deleted_proposals": 3,
                "deleted_evidence": 3,
            }
        )

        self.assertIn("forgot AGENTS.md", formatted)
        self.assertIn("matched=2", formatted)
        self.assertIn("deleted_proposals=3", formatted)
        self.assertIn("deleted_evidence=3", formatted)


if __name__ == "__main__":
    unittest.main()
