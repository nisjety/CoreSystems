"""Bundled skills — built-in skill definitions shipped with agent-core.

CC pattern: some skills are bundled in the distribution and always
available, independent of org configuration. They can be overridden
by org-specific skills of the same name.

Bundled skills are defined as plain Python dicts and converted to
SkillConfig instances at load time.
"""

from __future__ import annotations

from app.skills.domain import SkillConfig, SkillSource

# ---------------------------------------------------------------------------
# Bundled skill catalogue
# ---------------------------------------------------------------------------

_BUNDLED_SKILLS: list[dict] = [
    {
        "name": "coding-standards",
        "description": "Universal coding standards and best practices",
        "when_to_use": "Use when writing or reviewing code in any language",
        "trigger_keywords": ["code review", "best practice", "coding standard"],
        "content": (
            "Follow language-idiomatic conventions. "
            "Prefer immutability, small functions (<50 lines), "
            "explicit error handling, and input validation at boundaries."
        ),
    },
    {
        "name": "security-review",
        "description": "OWASP Top 10 security checklist",
        "when_to_use": (
            "Use when handling user input, authentication, API endpoints, "
            "or sensitive data"
        ),
        "trigger_keywords": ["security", "auth", "injection", "xss", "csrf"],
        "content": (
            "Check for: SQL injection, XSS, CSRF, insecure deserialization, "
            "hardcoded secrets, missing rate limiting, improper access control."
        ),
    },
    {
        "name": "tdd-workflow",
        "description": "Test-driven development workflow",
        "when_to_use": "Use when writing new features, fixing bugs, or refactoring",
        "trigger_keywords": ["test", "tdd", "coverage", "pytest", "jest"],
        "apply_to": ["**/*.py", "**/*.ts", "**/*.js", "**/*.go", "**/*.rs"],
        "content": (
            "1. Write test first (RED). "
            "2. Implement to pass (GREEN). "
            "3. Refactor (IMPROVE). "
            "Target 80%+ coverage."
        ),
    },
]


def load_bundled_skills(org_id: str = "__bundled__") -> list[SkillConfig]:
    """Return all bundled skills as SkillConfig instances."""
    results: list[SkillConfig] = []
    for defn in _BUNDLED_SKILLS:
        results.append(
            SkillConfig(
                org_id=org_id,
                name=defn["name"],
                description=defn["description"],
                content=defn["content"],
                trigger_keywords=defn.get("trigger_keywords", []),
                apply_to=defn.get("apply_to", []),
                when_to_use=defn.get("when_to_use", ""),
                source=SkillSource.BUNDLED,
            )
        )
    return results


def get_bundled_skill(name: str) -> SkillConfig | None:
    """Return a single bundled skill by name, or None."""
    for skill in load_bundled_skills():
        if skill.name == name:
            return skill
    return None
