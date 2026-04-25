#!/usr/bin/env python3

from __future__ import annotations

import tempfile
import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import import_markdown_memory as importer


class ImportMarkdownMemoryTests(unittest.TestCase):
    def test_source_ref_prefers_repo_relative_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            memory_file = root / "docs" / "AGENTS.md"
            memory_file.parent.mkdir()
            memory_file.write_text("# Memory\n\n- Prefer Rust.", encoding="utf-8")

            source_ref = importer.source_ref_for(
                memory_file,
                {"kind": "repo", "repo_path": str(root.resolve())},
                root / "other",
            )

            self.assertEqual(source_ref, "docs/AGENTS.md")

    def test_build_payload_includes_repo_scope_when_available(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            memory_file = root / "AGENTS.md"
            memory_file.write_text("# Memory\n\n- Prefer Rust.", encoding="utf-8")

            original_discover = importer.discover_repo_scope
            importer.discover_repo_scope = lambda _path: {
                "kind": "repo",
                "repo_path": str(root.resolve()),
                "repo_remote": "https://github.com/example/yena.git",
                "branch": "main",
            }
            try:
                payload = importer.build_payload(
                    memory_file,
                    source_type="local_markdown_memory",
                    commit=False,
                    confidence=0.8,
                    include_scope=True,
                    cwd=root,
                )
            finally:
                importer.discover_repo_scope = original_discover

            self.assertEqual(payload["source_ref"], "AGENTS.md")
            self.assertEqual(payload["source_type"], "local_markdown_memory")
            self.assertFalse(payload["commit"])
            self.assertEqual(payload["confidence"], 0.8)
            self.assertEqual(payload["scope"]["kind"], "repo")
            self.assertEqual(payload["content"], "# Memory\n\n- Prefer Rust.")

    def test_dry_run_summary_excludes_content(self) -> None:
        payload = {
            "source_ref": "AGENTS.md",
            "source_type": "local_markdown_memory",
            "content": "secret local memory",
            "commit": True,
            "confidence": 0.74,
        }

        summary = importer.payload_summary(payload)

        self.assertEqual(summary["source_ref"], "AGENTS.md")
        self.assertEqual(summary["content_bytes"], len("secret local memory"))
        self.assertNotIn("content", summary)


if __name__ == "__main__":
    unittest.main()
