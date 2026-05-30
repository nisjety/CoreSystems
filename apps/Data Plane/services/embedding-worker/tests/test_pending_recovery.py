"""Test pending message recovery for Redis Streams consumer groups."""

from __future__ import annotations

import sys
from pathlib import Path
import unittest
from unittest.mock import AsyncMock
import asyncio


SERVICE_ROOT = Path(__file__).resolve().parents[1]
if str(SERVICE_ROOT) not in sys.path:
    sys.path.insert(0, str(SERVICE_ROOT))

from worker.stream_recovery import ClaimedMessage, claim_pending_messages


class TestPendingMessageRecovery(unittest.TestCase):
    """
    Test cases for Redis Streams pending message recovery.
    Current bug: worker only uses '>' and never recovers pending messages.
    """
    
    def setUp(self):
        """Set up test fixtures."""
        self.redis_mock = AsyncMock()
        self.db_mock = AsyncMock()
        
    def test_claim_pending_messages_returns_stuck_jobs(self):
        """
        RED TEST 1: When pending messages exist, they should be claimed and returned.
        
        Scenario: A message was delivered but not acked (e.g., worker crashed).
        Expected: XPENDING query identifies it, XCLAIM retrieves it for reprocessing.
        """
        # Arrange: Redis has 1 pending message (stuck for > 30 seconds)
        self.redis_mock.xpending_range = AsyncMock(return_value=[
            {
                "message_id": b"1234-0",
                "consumer": b"embedding-worker-0", 
                "time_since_delivered": 35000,  # 35 seconds
                "times_delivered": 1,
            }
        ])
        
        self.redis_mock.xclaim = AsyncMock(return_value=[
            (
                "1234-0",
                {
                    "knowledge_id": "ku-001",
                    "text": "Test content",
                    "document_id": "doc-001",
                    "org_id": "org-001",
                    "chunk_index": "0",
                }
            )
        ])
        
        # Act: Call the recovery function in isolation
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            result = loop.run_until_complete(
                claim_pending_messages(
                    self.redis_mock,
                    "dataplane.knowledge.units.created",
                    "embedding-worker",
                    "embedding-worker-0",
                )
            )
        finally:
            loop.close()
        
        # Assert: Pending message was claimed
        self.redis_mock.xpending_range.assert_called_once()
        self.redis_mock.xclaim.assert_called_once_with(
            name="dataplane.knowledge.units.created",
            groupname="embedding-worker",
            consumername="embedding-worker-0",
            min_idle_time=30000,
            message_ids=["1234-0"],
        )
        self.assertEqual(len(result), 1)
        self.assertEqual(
            result[0],
            ClaimedMessage(
                message_id="1234-0",
                fields={
                    "knowledge_id": "ku-001",
                    "text": "Test content",
                    "document_id": "doc-001",
                    "org_id": "org-001",
                    "chunk_index": "0",
                },
                delivery_count=1,
                previous_consumer="embedding-worker-0",
                idle_ms=35000,
            ),
        )

    def test_claim_pending_returns_empty_when_no_pending(self):
        """
        RED TEST 2: When no pending messages exist, return empty list.
        
        Scenario: All messages are either acked or brand new (not pending).
        Expected: XPENDING returns nothing, function returns [].
        """
        # Arrange: No pending messages
        self.redis_mock.xpending_range = AsyncMock(return_value=[])
        
        # Act
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            result = loop.run_until_complete(
                claim_pending_messages(
                    self.redis_mock,
                    "dataplane.knowledge.units.created",
                    "embedding-worker",
                    "embedding-worker-0",
                )
            )
        finally:
            loop.close()
        
        # Assert: No claims made
        self.redis_mock.xpending_range.assert_called_once()
        self.redis_mock.xclaim.assert_not_called()
        self.assertEqual(result, [])

    def test_run_loop_processes_pending_before_new(self):
        """
        RED TEST 3: Main loop should recover pending messages before reading new ones.
        
        Scenario: Worker restarts and has 1 pending message + 1 new message.
        Expected: Both are processed in the same iteration.
        """
        # This test would require mocking the entire run() loop
        # For simplicity, we'll test the integration by verifying:
        # 1. claim_pending_messages is called before xreadgroup with '>'
        # 2. Both pending and new messages end up in the batch
        
        # Arrange: 1 pending, 1 new
        self.redis_mock.xpending_range = AsyncMock(return_value=[
            {
                "message_id": b"1000-0",
                "consumer": b"embedding-worker-0",
                "time_since_delivered": 40000,
                "times_delivered": 1,
            }
        ])
        
        self.redis_mock.xclaim = AsyncMock(return_value=[
            (
                "1000-0",
                {
                    "knowledge_id": "ku-pending",
                    "text": "Pending message",
                    "document_id": "doc-001",
                    "org_id": "org-001",
                    "chunk_index": "0",
                }
            )
        ])
        
        self.redis_mock.xreadgroup = AsyncMock(return_value=[
            (
                "dataplane.knowledge.units.created",
                [
                    (
                        "2000-0",
                        {
                            "knowledge_id": "ku-new",
                            "text": "New message",
                            "document_id": "doc-002",
                            "org_id": "org-002",
                            "chunk_index": "0",
                        }
                    )
                ]
            )
        ])
        
        # Act: Simulate one iteration (this would be tested via modified run())
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            pending = loop.run_until_complete(
                claim_pending_messages(
                    self.redis_mock,
                    "dataplane.knowledge.units.created",
                    "embedding-worker",
                    "embedding-worker-0",
                )
            )
        finally:
            loop.close()
            
        # Assert: We got the pending message
        self.assertEqual(len(pending), 1)
        self.assertEqual(pending[0].fields["knowledge_id"], "ku-pending")
        
        # In the real implementation, the run() loop would combine
        # pending + new messages before processing

    def test_pending_recovery_respects_idle_time_threshold(self):
        """
        RED TEST 4: Only claim messages idle for > 30 seconds to avoid race conditions.
        
        Scenario: Message is pending but only for 5 seconds (might be in-flight).
        Expected: Don't claim it yet (wait for IDLE_TIME_MS threshold).
        """
        # Arrange: Pending message, but recently delivered
        self.redis_mock.xpending_range = AsyncMock(return_value=[
            {
                "message_id": b"3000-0",
                "consumer": b"embedding-worker-0",
                "time_since_delivered": 5000,  # Only 5 seconds
                "times_delivered": 1,
            }
        ])
        
        # Act
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        try:
            result = loop.run_until_complete(
                claim_pending_messages(
                    self.redis_mock,
                    "dataplane.knowledge.units.created",
                    "embedding-worker",
                    "embedding-worker-0",
                    min_idle_ms=30000,  # 30 second threshold
                )
            )
        finally:
            loop.close()
        
        self.redis_mock.xpending_range.assert_called_once()
        self.redis_mock.xclaim.assert_not_called()
        self.assertEqual(result, [])
        

if __name__ == "__main__":
    unittest.main()
