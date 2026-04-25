#!/usr/bin/env python3

from __future__ import annotations

import argparse
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import dev_memory_smoke


class DevMemorySmokeTests(unittest.TestCase):
    def test_base_url_uses_bind_address(self) -> None:
        self.assertEqual(dev_memory_smoke.base_url("127.0.0.1:18082"), "http://127.0.0.1:18082")

    def test_fixture_markdown_contains_expected_memory(self) -> None:
        fixture = dev_memory_smoke.fixture_markdown()

        self.assertIn("Use SQLite for local-first storage.", fixture)
        self.assertIn("Audit every retrieval event", fixture)

    def test_validate_smoke_outputs_accepts_valid_loop(self) -> None:
        dev_memory_smoke.validate_smoke_outputs(
            {"committed_items": 2},
            {
                "answer_context": {
                    "should_abstain": False,
                    "memories": [{"statement": "Use SQLite for local-first storage."}],
                }
            },
            {"events": [{"request_type": "retrieve_v2"}]},
        )

    def test_validate_smoke_outputs_rejects_missing_audit(self) -> None:
        with self.assertRaises(RuntimeError):
            dev_memory_smoke.validate_smoke_outputs(
                {"committed_items": 2},
                {
                    "answer_context": {
                        "should_abstain": False,
                        "memories": [{"statement": "Use SQLite for local-first storage."}],
                    }
                },
                {"events": []},
            )

    def test_format_summary_includes_loop_outputs(self) -> None:
        summary = dev_memory_smoke.format_summary(
            {
                "import": {
                    "committed_items": 2,
                    "skipped_items": 0,
                    "job_id": "job-1",
                },
                "retrieval": {
                    "answer_context": {
                        "memories": [{"statement": "Use SQLite for local-first storage."}]
                    }
                },
                "audit": {
                    "returned": 1,
                    "events": [{"request_type": "retrieve_v2"}],
                },
                "forget": {
                    "deleted_memory_items": 2,
                    "deleted_evidence": 2,
                },
            }
        )

        self.assertIn("Yena dev-memory smoke passed.", summary)
        self.assertIn("import: committed=2 skipped=0 job=job-1", summary)
        self.assertIn("retrieval: Use SQLite", summary)
        self.assertIn("audit: events=1 latest=retrieve_v2", summary)
        self.assertIn("forget: deleted_memories=2 deleted_evidence=2", summary)

    def test_run_smoke_rejects_existing_explicit_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            db_path = Path(tmp) / "existing.db"
            markdown_path = Path(tmp) / "fixture.md"
            db_path.write_text("existing", encoding="utf-8")

            with self.assertRaises(RuntimeError):
                dev_memory_smoke.run_smoke(
                    argparse.Namespace(
                        db_path=db_path,
                        markdown_path=markdown_path,
                        compiler_bind="127.0.0.1:1",
                        gateway_bind="127.0.0.1:2",
                        startup_timeout=0.01,
                        timeout=0.01,
                        agent_id="test",
                        keep_files=True,
                    )
                )


if __name__ == "__main__":
    unittest.main()
