#!/usr/bin/env python3

from __future__ import annotations

import argparse
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import list_audit_events


class ListAuditEventsTests(unittest.TestCase):
    def test_build_request_omits_empty_filters(self) -> None:
        request = list_audit_events.build_request(
            argparse.Namespace(limit=10, agent_id=None, request_type=None)
        )

        self.assertEqual(request, {"limit": 10})

    def test_build_request_includes_filters(self) -> None:
        request = list_audit_events.build_request(
            argparse.Namespace(
                limit=5,
                agent_id="yena-cli",
                request_type="retrieve_v2",
            )
        )

        self.assertEqual(
            request,
            {
                "limit": 5,
                "agent_id": "yena-cli",
                "request_type": "retrieve_v2",
            },
        )

    def test_format_response_handles_empty_events(self) -> None:
        self.assertEqual(
            list_audit_events.format_response({"returned": 0, "events": []}),
            "No audit events found.",
        )

    def test_format_response_summarizes_retrieve_v2(self) -> None:
        formatted = list_audit_events.format_response(
            {
                "returned": 1,
                "events": [
                    {
                        "created_at": "2026-04-25T12:00:00+00:00",
                        "request_type": "retrieve_v2",
                        "agent_id": "yena-cli",
                        "scope_applied": "repo",
                        "shared_json": {
                            "should_abstain": False,
                            "memory_count": 1,
                            "memory_types": ["project"],
                            "evidence_refs": ["e1"],
                        },
                        "redacted_json": {"redacted_memory_count": 0},
                    }
                ],
            }
        )

        self.assertIn("Yena returned 1 audit event(s).", formatted)
        self.assertIn("[retrieve_v2] agent=yena-cli scope=repo", formatted)
        self.assertIn("should_abstain=false", formatted)
        self.assertIn('memory_types=["project"]', formatted)
        self.assertIn("memory_count=1", formatted)
        self.assertIn('"redacted_memory_count": 0', formatted)


if __name__ == "__main__":
    unittest.main()
