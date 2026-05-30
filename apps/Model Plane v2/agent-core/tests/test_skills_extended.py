"""Tests for Phase B3: Skills — applyTo, argument substitution, bundled, MCP."""

from __future__ import annotations

import pytest

from app.skills.domain import SkillConfig, SkillInvocation, SkillMatch, SkillSource
from app.skills.loader import (
    filter_skills_for_file,
    load_skills_from_directory,
    skill_applies_to_file,
    substitute_arguments,
)
from app.skills.bundled import get_bundled_skill, load_bundled_skills
from app.skills.mcp_skills import build_skill_from_mcp_tool, build_skills_from_tools


# ===========================================================================
# Domain model tests
# ===========================================================================


class TestSkillConfigExtended:
    def test_apply_to_field(self):
        skill = SkillConfig(
            org_id="o1",
            name="py",
            description="Python",
            content="...",
            apply_to=["**/*.py"],
        )
        assert skill.apply_to == ["**/*.py"]

    def test_when_to_use_field(self):
        skill = SkillConfig(
            org_id="o1",
            name="py",
            description="Python",
            content="...",
            when_to_use="Use for Python code",
        )
        assert skill.when_to_use == "Use for Python code"

    def test_source_enum_values(self):
        assert SkillSource.DATABASE == "database"
        assert SkillSource.FILESYSTEM == "filesystem"
        assert SkillSource.BUNDLED == "bundled"
        assert SkillSource.MCP == "mcp"

    def test_source_default(self):
        skill = SkillConfig(org_id="o1", name="x", description="x", content="x")
        assert skill.source == SkillSource.DATABASE

    def test_skill_invocation(self):
        inv = SkillInvocation(skill_id="s1", name="test-skill")
        assert inv.skill_id == "s1"
        assert inv.invoked_at is not None

    def test_skill_match_source(self):
        m = SkillMatch(skill_id="s1", name="x", content="y", source=SkillSource.MCP)
        assert m.source == SkillSource.MCP


# ===========================================================================
# applyTo glob matching
# ===========================================================================


class TestApplyTo:
    def _make_skill(self, patterns: list[str]) -> SkillConfig:
        return SkillConfig(
            org_id="o1",
            name="test",
            description="test",
            content="...",
            apply_to=patterns,
        )

    def test_empty_applies_to_all(self):
        skill = self._make_skill([])
        assert skill_applies_to_file(skill, "anything.txt") is True

    def test_py_pattern_matches(self):
        skill = self._make_skill(["**/*.py"])
        assert skill_applies_to_file(skill, "src/main.py") is True
        assert skill_applies_to_file(skill, "tests/test_foo.py") is True

    def test_py_pattern_rejects_js(self):
        skill = self._make_skill(["**/*.py"])
        assert skill_applies_to_file(skill, "src/app.js") is False

    def test_multiple_patterns(self):
        skill = self._make_skill(["**/*.py", "**/*.pyi"])
        assert skill_applies_to_file(skill, "stubs/types.pyi") is True

    def test_filter_skills_for_file(self):
        py_skill = self._make_skill(["**/*.py"])
        js_skill = SkillConfig(
            org_id="o1",
            name="js",
            description="JS",
            content="...",
            apply_to=["**/*.js", "**/*.ts"],
        )
        all_skill = SkillConfig(
            org_id="o1", name="all", description="all", content="..."
        )
        result = filter_skills_for_file([py_skill, js_skill, all_skill], "app/main.py")
        names = [s.name for s in result]
        assert "test" in names  # py_skill matches
        assert "all" in names   # no apply_to → matches
        assert "js" not in names


# ===========================================================================
# Argument substitution
# ===========================================================================


class TestArgumentSubstitution:
    def test_positional_args(self):
        content = "Process ${1} and ${2}"
        result = substitute_arguments(content, args=["file.py", "output.txt"])
        assert result == "Process file.py and output.txt"

    def test_missing_positional(self):
        content = "Process ${1} and ${3}"
        result = substitute_arguments(content, args=["file.py"])
        assert result == "Process file.py and "

    def test_arguments_joined(self):
        content = "All: ${arguments}"
        result = substitute_arguments(content, args=["a", "b", "c"])
        assert result == "All: a b c"

    def test_skill_dir(self):
        content = "Look in ${SKILL_DIR}/data"
        result = substitute_arguments(content, skill_dir="/skills/python")
        assert result == "Look in /skills/python/data"

    def test_session_id(self):
        content = "Session: ${SESSION_ID}"
        result = substitute_arguments(content, session_id="abc-123")
        assert result == "Session: abc-123"

    def test_unknown_kept(self):
        content = "Value: ${UNKNOWN_VAR}"
        result = substitute_arguments(content)
        assert result == "Value: ${UNKNOWN_VAR}"

    def test_no_args_empty(self):
        content = "No args: ${arguments}"
        result = substitute_arguments(content)
        assert result == "No args: "


# ===========================================================================
# Filesystem loader with extended front matter
# ===========================================================================


class TestLoaderExtendedFrontMatter:
    def test_apply_to_parsed(self, tmp_path):
        skill_dir = tmp_path / "skills" / "myskill"
        skill_dir.mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text(
            "---\n"
            "name: myskill\n"
            "description: A skill\n"
            "apply_to: [**/*.py, **/*.pyi]\n"
            "when_to_use: Use for Python\n"
            "---\n"
            "# My Skill\n"
            "Content here.\n"
        )
        skills = load_skills_from_directory(tmp_path / "skills", org_id="test")
        assert len(skills) == 1
        assert skills[0].apply_to == ["**/*.py", "**/*.pyi"]
        assert skills[0].when_to_use == "Use for Python"
        assert skills[0].source == SkillSource.FILESYSTEM


# ===========================================================================
# Bundled skills
# ===========================================================================


class TestBundledSkills:
    def test_load_returns_skills(self):
        skills = load_bundled_skills()
        assert len(skills) >= 3
        names = [s.name for s in skills]
        assert "coding-standards" in names
        assert "security-review" in names
        assert "tdd-workflow" in names

    def test_source_is_bundled(self):
        skills = load_bundled_skills()
        for s in skills:
            assert s.source == SkillSource.BUNDLED

    def test_get_bundled_skill_found(self):
        skill = get_bundled_skill("coding-standards")
        assert skill is not None
        assert skill.name == "coding-standards"

    def test_get_bundled_skill_missing(self):
        assert get_bundled_skill("nonexistent") is None

    def test_tdd_has_apply_to(self):
        skill = get_bundled_skill("tdd-workflow")
        assert skill is not None
        assert "**/*.py" in skill.apply_to

    def test_bundled_has_when_to_use(self):
        skills = load_bundled_skills()
        for s in skills:
            assert s.when_to_use != ""


# ===========================================================================
# MCP skill builder
# ===========================================================================


class TestMCPSkillBuilder:
    def test_build_single_tool(self):
        skill = build_skill_from_mcp_tool(
            server_name="github",
            tool_name="search-code",
            description="Search code across repos",
        )
        assert skill.name == "mcp-github-search-code"
        assert skill.source == SkillSource.MCP
        assert "search-code" in skill.content
        assert "github" in skill.content

    def test_build_with_schema(self):
        schema = {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "description": "Max results"},
            },
            "required": ["query"],
        }
        skill = build_skill_from_mcp_tool(
            server_name="test",
            tool_name="find",
            description="Find things",
            input_schema=schema,
        )
        assert "`query`" in skill.content
        assert "(required)" in skill.content
        assert "`limit`" in skill.content

    def test_build_batch(self):
        tools = [
            {"name": "read", "description": "Read data"},
            {"name": "write", "description": "Write data"},
            {"name": "", "description": "No name — skip"},
        ]
        skills = build_skills_from_tools("myserver", tools)
        assert len(skills) == 2
        names = [s.name for s in skills]
        assert "mcp-myserver-read" in names
        assert "mcp-myserver-write" in names

    def test_trigger_keywords_from_name(self):
        skill = build_skill_from_mcp_tool(
            server_name="ctx",
            tool_name="query_docs",
            description="Query documentation",
        )
        assert "query" in skill.trigger_keywords
        assert "docs" in skill.trigger_keywords
