"""Built-in agent definitions — pre-configured sub-agent archetypes.

Ported from CC's AgentTool/builtInAgents.ts:
These are ready-to-use agent configurations that can be referenced by name
in the spawn_agent tool call.

Each definition specifies:
- System prompt / instructions
- Model preference (alias or inherit)
- Tool whitelist/blacklist
- Max turns
- Whether to retain output in parent context
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class BuiltInAgent:
    """A pre-configured agent archetype."""

    name: str
    description: str
    system_prompt: str
    model: str = "inherit"  # inherit | opus | sonnet | haiku | explicit model
    allowed_tools: list[str] = field(default_factory=list)
    denied_tools: list[str] = field(default_factory=list)
    max_turns: int = 25
    retain_output: bool = True
    background: bool = False


# ---------------------------------------------------------------------------
# Standard agent definitions
# ---------------------------------------------------------------------------

GENERAL_PURPOSE_AGENT = BuiltInAgent(
    name="general",
    description="General-purpose sub-agent that inherits parent capabilities",
    system_prompt=(
        "You are a focused sub-agent. Complete the assigned task efficiently. "
        "Use available tools as needed. Report your findings clearly when done."
    ),
)

EXPLORE_AGENT = BuiltInAgent(
    name="explore",
    description="Read-only codebase exploration agent (fast, no writes)",
    system_prompt=(
        "You are a codebase exploration agent. Your job is to search, read, "
        "and understand code. You CANNOT modify files. Report findings in a "
        "structured format: file paths, key patterns, and relevant code snippets."
    ),
    model="haiku",  # Use cheapest model for exploration
    allowed_tools=[
        "search", "retrieve", "read_file", "list_files",
        "grep", "glob", "get_file_contents", "memory_recall",
    ],
    max_turns=15,
)

PLAN_AGENT = BuiltInAgent(
    name="planner",
    description="Planning agent that creates structured implementation plans",
    system_prompt=(
        "You are a planning agent. Analyze the task and create a detailed, "
        "step-by-step implementation plan. Output a numbered list of concrete "
        "actions. Do NOT execute any actions — only plan them.\n\n"
        "Include for each step:\n"
        "1. What file(s) to modify\n"
        "2. What changes to make\n"
        "3. Why (rationale)\n"
        "4. Dependencies on other steps"
    ),
    model="sonnet",
    allowed_tools=[
        "search", "retrieve", "read_file", "list_files",
        "grep", "glob", "get_file_contents",
    ],
    max_turns=10,
)

VERIFICATION_AGENT = BuiltInAgent(
    name="verifier",
    description="Verification agent that validates changes after implementation",
    system_prompt=(
        "You are a verification agent. Check that recent changes are correct by:\n"
        "1. Reading modified files to verify the changes\n"
        "2. Running tests if available\n"
        "3. Checking for lint/type errors\n"
        "4. Verifying the changes match the original intent\n\n"
        "Report: PASS (all checks ok) or FAIL (with specific issues)."
    ),
    model="haiku",
    max_turns=15,
)

CODE_REVIEWER = BuiltInAgent(
    name="reviewer",
    description="Code review agent that finds bugs, security issues, and style problems",
    system_prompt=(
        "You are a code reviewer. Examine the provided code changes for:\n"
        "- Bugs and logic errors\n"
        "- Security vulnerabilities (OWASP Top 10)\n"
        "- Performance issues\n"
        "- Style and consistency problems\n"
        "- Missing error handling\n\n"
        "Rate each issue: CRITICAL / HIGH / MEDIUM / LOW.\n"
        "If no issues found, say 'LGTM' with a brief summary."
    ),
    model="sonnet",
    allowed_tools=[
        "search", "retrieve", "read_file", "list_files",
        "grep", "glob", "get_file_contents",
    ],
    max_turns=15,
)

RESEARCH_AGENT = BuiltInAgent(
    name="researcher",
    description="Research agent that searches the web and synthesizes information",
    system_prompt=(
        "You are a research agent. Search for relevant information and "
        "provide a well-structured summary with key findings, source "
        "references, and actionable recommendations."
    ),
    allowed_tools=[
        "web_search", "web_fetch", "memory_recall", "memory_store",
    ],
    max_turns=10,
)

TDD_AGENT = BuiltInAgent(
    name="tdd",
    description="Test-driven development agent that writes tests first, then implementation",
    system_prompt=(
        "You are a TDD agent. Follow this strict workflow:\n"
        "1. RED: Write a failing test for the feature\n"
        "2. GREEN: Write minimal implementation to pass the test\n"
        "3. REFACTOR: Clean up without breaking tests\n\n"
        "Always write tests BEFORE implementation code."
    ),
    model="sonnet",
    max_turns=30,
)


# Registry of all built-in agents
BUILT_IN_AGENTS: dict[str, BuiltInAgent] = {
    agent.name: agent
    for agent in [
        GENERAL_PURPOSE_AGENT,
        EXPLORE_AGENT,
        PLAN_AGENT,
        VERIFICATION_AGENT,
        CODE_REVIEWER,
        RESEARCH_AGENT,
        TDD_AGENT,
    ]
}


def get_built_in_agent(name: str) -> BuiltInAgent | None:
    """Look up a built-in agent by name."""
    return BUILT_IN_AGENTS.get(name)


def list_built_in_agents() -> list[BuiltInAgent]:
    """Return all available built-in agents."""
    return list(BUILT_IN_AGENTS.values())
