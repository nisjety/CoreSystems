"""Tests for Phase N — Session memory write-back (fact extraction)."""

from __future__ import annotations

import json
import pytest

from app.context.session_compact import _extract_facts


class MockLLMClient:
    """Mock LLM client that returns predefined responses."""

    def __init__(self, response: str) -> None:
        self._response = response

    async def planner_complete(self, messages: list[dict]) -> str:
        return self._response


@pytest.mark.asyncio
class TestExtractFacts:
    async def test_valid_json_response(self) -> None:
        facts_json = json.dumps([
            {"key": "auth-pattern", "content": "Uses Better Auth with social OAuth"},
            {"key": "db-schema", "content": "agent_runs table stores all run state"},
        ])
        llm = MockLLMClient(facts_json)
        facts = await _extract_facts("test summary", llm)
        assert len(facts) == 2
        assert facts[0]["key"] == "auth-pattern"
        assert facts[1]["key"] == "db-schema"

    async def test_code_block_wrapped_response(self) -> None:
        facts_json = json.dumps([
            {"key": "pattern-a", "content": "fact one"},
        ])
        wrapped = f"```json\n{facts_json}\n```"
        llm = MockLLMClient(wrapped)
        facts = await _extract_facts("summary", llm)
        assert len(facts) == 1
        assert facts[0]["key"] == "pattern-a"

    async def test_invalid_json_returns_empty(self) -> None:
        llm = MockLLMClient("not json at all")
        facts = await _extract_facts("summary", llm)
        assert facts == []

    async def test_non_list_response_returns_empty(self) -> None:
        llm = MockLLMClient(json.dumps({"key": "x", "content": "y"}))
        facts = await _extract_facts("summary", llm)
        assert facts == []

    async def test_missing_fields_filtered(self) -> None:
        facts_json = json.dumps([
            {"key": "valid", "content": "good"},
            {"key": "no-content"},  # missing content
            {"content": "no-key"},  # missing key
            {"key": "", "content": "empty key"},  # empty key
        ])
        llm = MockLLMClient(facts_json)
        facts = await _extract_facts("summary", llm)
        assert len(facts) == 1
        assert facts[0]["key"] == "valid"

    async def test_capped_at_10_facts(self) -> None:
        many_facts = [{"key": f"fact-{i}", "content": f"content {i}"} for i in range(20)]
        llm = MockLLMClient(json.dumps(many_facts))
        facts = await _extract_facts("summary", llm)
        assert len(facts) == 10

    async def test_llm_error_returns_empty(self) -> None:
        class FailingLLM:
            async def planner_complete(self, messages: list[dict]) -> str:
                raise RuntimeError("API error")

        facts = await _extract_facts("summary", FailingLLM())
        assert facts == []

    async def test_key_and_content_truncated(self) -> None:
        long_key = "k" * 200
        long_content = "c" * 3000
        facts_json = json.dumps([{"key": long_key, "content": long_content}])
        llm = MockLLMClient(facts_json)
        facts = await _extract_facts("summary", llm)
        assert len(facts) == 1
        assert len(facts[0]["key"]) <= 100
        assert len(facts[0]["content"]) <= 2000
