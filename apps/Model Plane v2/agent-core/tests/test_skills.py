"""Tests for Phase H — Skills system (domain, registry matching)."""

from __future__ import annotations

import pytest

from app.skills.domain import SkillConfig, SkillMatch
from app.skills.registry import _compute_match_score


# ---------------------------------------------------------------------------
# SkillConfig domain
# ---------------------------------------------------------------------------


class TestSkillConfig:
    def test_defaults(self) -> None:
        s = SkillConfig(
            org_id="org1",
            name="test-skill",
            description="A test skill",
            content="# Skill content",
        )
        assert s.enabled is True
        assert s.trigger_keywords == []
        assert s.trigger_file_patterns == []
        assert s.tool_restrictions == []
        assert s.id  # auto-generated

    def test_with_triggers(self) -> None:
        s = SkillConfig(
            org_id="org1",
            name="tdd",
            description="Test-driven development",
            content="Write tests first.",
            trigger_keywords=["test", "tdd", "unittest"],
            trigger_file_patterns=["**/*.test.py"],
        )
        assert len(s.trigger_keywords) == 3
        assert s.trigger_file_patterns[0] == "**/*.test.py"


class TestSkillMatch:
    def test_defaults(self) -> None:
        m = SkillMatch(skill_id="s1", name="tdd", content="Write tests.")
        assert m.match_score == 0.0
        assert m.tool_restrictions == []


# ---------------------------------------------------------------------------
# _compute_match_score
# ---------------------------------------------------------------------------


class TestComputeMatchScore:
    def test_keyword_match(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="tdd",
            description="Test-driven development",
            content="...",
            trigger_keywords=["test", "tdd"],
        )
        score = _compute_match_score(skill, "write a test for my function", [])
        assert score > 0.0

    def test_no_match(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="deploy",
            description="Deployment patterns",
            content="...",
            trigger_keywords=["deploy", "kubernetes"],
        )
        score = _compute_match_score(skill, "write a python function", [])
        # Only description overlap might contribute
        assert score < 0.3

    def test_file_pattern_match(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="react",
            description="React patterns",
            content="...",
            trigger_file_patterns=["**/*.tsx", "**/*.jsx"],
        )
        score = _compute_match_score(skill, "something", ["src/App.tsx"])
        assert score > 0.0

    def test_description_overlap(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="security",
            description="security review vulnerability analysis",
            content="...",
        )
        score = _compute_match_score(skill, "review security of this module", [])
        assert score > 0.0

    def test_max_score_capped_at_1(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="all",
            description="test review deploy security analysis",
            content="...",
            trigger_keywords=["test", "review", "deploy", "security"],
            trigger_file_patterns=["**/*.py"],
        )
        score = _compute_match_score(
            skill,
            "test review deploy security analysis",
            ["main.py"],
        )
        assert score <= 1.0

    def test_empty_triggers_zero(self) -> None:
        skill = SkillConfig(
            org_id="o",
            name="bare",
            description="xyz",
            content="...",
        )
        score = _compute_match_score(skill, "something completely different", [])
        assert score == 0.0
