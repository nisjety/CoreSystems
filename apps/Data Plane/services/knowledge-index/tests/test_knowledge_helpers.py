from __future__ import annotations

import sys
from pathlib import Path
import unittest
from unittest.mock import AsyncMock


SERVICE_ROOT = Path(__file__).resolve().parents[1]
if str(SERVICE_ROOT) not in sys.path:
    sys.path.insert(0, str(SERVICE_ROOT))

from worker.knowledge_ids import build_knowledge_id
from worker.stream_recovery import ClaimedMessage, claim_pending_messages


class KnowledgeIdTests(unittest.TestCase):
    def test_build_knowledge_id_is_deterministic(self) -> None:
        first = build_knowledge_id("doc-123", 4)
        second = build_knowledge_id("doc-123", 4)
        third = build_knowledge_id("doc-123", 5)

        self.assertEqual(first, second)
        self.assertNotEqual(first, third)


class ClaimPendingMessagesTests(unittest.IsolatedAsyncioTestCase):
    async def test_claim_pending_messages_normalizes_bytes_ids(self) -> None:
        redis_client = AsyncMock()
        redis_client.xpending_range.return_value = [
            {
                "message_id": b"1711111111111-0",
                "consumer": b"knowledge-index-worker",
                "time_since_delivered": 40000,
                "times_delivered": 1,
            }
        ]
        redis_client.xclaim.return_value = [
            ("1711111111111-0", {"document_id": "doc-123", "org_id": "org-123"})
        ]

        messages = await claim_pending_messages(
            redis_client,
            "dataplane.documents.created",
            "knowledge-index",
            "knowledge-index-worker",
            min_idle_ms=30_000,
            count=10,
        )

        self.assertEqual(
            messages,
            [
                ClaimedMessage(
                    message_id="1711111111111-0",
                    fields={"document_id": "doc-123", "org_id": "org-123"},
                    delivery_count=1,
                    previous_consumer="knowledge-index-worker",
                    idle_ms=40000,
                )
            ],
        )
        redis_client.xclaim.assert_awaited_once_with(
            name="dataplane.documents.created",
            groupname="knowledge-index",
            consumername="knowledge-index-worker",
            min_idle_time=30_000,
            message_ids=["1711111111111-0"],
        )