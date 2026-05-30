"""Skills loader — load and cache skill definitions from Postgres or filesystem.

CC pattern: skills are loaded once per org and cached, similar to
how CC loads SKILL.md files from disk at session start.

Filesystem skills are loaded from a directory containing .md files.
Files named ``SKILL.md`` are treated as org-independent "global" skills.
Files named ``<name>.skill.md`` are loaded with the file stem as the skill name.
Both formats may include optional YAML front matter for metadata:

    ---
    name: python-expert
    description: Senior Python developer expertise
    trigger_keywords: [python, pep8, pytest]
    trigger_file_patterns: ["**/*.py"]
    apply_to: ["**/*.py", "**/*.pyi"]
    when_to_use: Use when writing or reviewing Python code
    ---
    # Skill content here ...

CC-style argument substitution is supported in skill content:
  - ``${1}``          — first positional argument
  - ``${arguments}``  — all arguments joined
  - ``${SKILL_DIR}``  — directory of the skill file
  - ``${SESSION_ID}`` — current session id
"""

from __future__ import annotations

import fnmatch
import logging
import re
from pathlib import Path
from typing import Any

from app.database import get_pool
from app.skills.domain import SkillConfig, SkillSource

logger = logging.getLogger(__name__)

# In-memory per-org cache
_cache: dict[str, list[SkillConfig]] = {}

# Cache for filesystem-loaded global skills (keyed by directory path)
_fs_cache: dict[str, list[SkillConfig]] = {}

# YAML front-matter pattern
_FRONT_MATTER_RE = re.compile(r"^---\s*\n(.*?)\n---\s*\n", re.DOTALL)

# Argument substitution pattern
_ARG_RE = re.compile(r"\$\{(\w+)\}")


async def load_skills_for_org(org_id: str) -> list[SkillConfig]:
    """Load all enabled skills for an org (cached).

    Combines database skills with any filesystem skills from
    ``settings.skills_directory``.
    """
    if org_id in _cache:
        return _cache[org_id]

    pool = await get_pool()
    async with pool.acquire() as conn:
        rows = await conn.fetch(
            """
            SELECT id, org_id, name, description, content,
                   trigger_keywords, trigger_file_patterns,
                   tool_restrictions, enabled, created_at, updated_at
            FROM agent_skills
            WHERE org_id = $1 AND enabled = true
            ORDER BY name
            """,
            org_id,
        )

    import json

    skills: list[SkillConfig] = []
    for row in rows:
        skills.append(
            SkillConfig(
                id=row["id"],
                org_id=row["org_id"],
                name=row["name"],
                description=row["description"],
                content=row["content"],
                trigger_keywords=json.loads(row["trigger_keywords"]) if row["trigger_keywords"] else [],
                trigger_file_patterns=json.loads(row["trigger_file_patterns"]) if row["trigger_file_patterns"] else [],
                tool_restrictions=json.loads(row["tool_restrictions"]) if row["tool_restrictions"] else [],
                enabled=row["enabled"],
                created_at=row["created_at"],
                updated_at=row["updated_at"],
            )
        )

    # Merge in filesystem skills (org-id is set to the requesting org)
    from app.config import settings

    if settings.skills_directory:
        fs_skills = load_skills_from_directory(
            Path(settings.skills_directory), org_id=org_id
        )
        # Avoid duplicates by name (DB takes precedence)
        db_names = {s.name for s in skills}
        for fs_skill in fs_skills:
            if fs_skill.name not in db_names:
                skills.append(fs_skill)

    _cache[org_id] = skills
    return skills


def load_skills_from_directory(
    directory: Path,
    org_id: str = "__global__",
) -> list[SkillConfig]:
    """Load skill definitions from a filesystem directory.

    Scans for:
    - Any file matching ``*.skill.md`` or ``SKILL.md``
    - Optional YAML front-matter for metadata

    Results are cached in-memory keyed by ``(directory, org_id)``.
    """
    cache_key = f"{directory}::{org_id}"
    if cache_key in _fs_cache:
        return _fs_cache[cache_key]

    if not directory.exists() or not directory.is_dir():
        logger.warning("skills_directory_not_found", extra={"path": str(directory)})
        _fs_cache[cache_key] = []
        return []

    skills: list[SkillConfig] = []
    # Collect candidate files (non-recursive in root, recursive for skill sub-dirs)
    candidates: list[Path] = []
    for path in sorted(directory.rglob("*.md")):
        stem = path.stem.upper()
        if stem == "SKILL" or path.name.endswith(".skill.md"):
            candidates.append(path)

    for path in candidates:
        try:
            skill = _parse_skill_file(path, org_id)
            if skill is not None:
                skills.append(skill)
        except Exception as exc:
            logger.warning(
                "skills_file_parse_error",
                extra={"path": str(path), "error": str(exc)},
            )

    logger.info(
        "skills_loaded_from_directory",
        extra={"directory": str(directory), "count": len(skills)},
    )
    _fs_cache[cache_key] = skills
    return skills


def _parse_skill_file(path: Path, org_id: str) -> SkillConfig | None:
    """Parse a single skill markdown file.

    Extracts optional YAML front matter for metadata, then uses the
    remaining text as the skill content.
    """
    import json

    raw = path.read_text(encoding="utf-8")

    # Default metadata derived from filename
    # e.g.  python-expert.skill.md → name="python-expert"
    if path.name.lower() == "skill.md":
        # Use parent directory name as skill name
        name = path.parent.name
    else:
        name = path.stem
        if name.endswith(".skill"):
            name = name[: -len(".skill")]

    description = ""
    trigger_keywords: list[str] = []
    trigger_file_patterns: list[str] = []
    tool_restrictions: list[str] = []
    apply_to: list[str] = []
    when_to_use: str = ""

    # Try to parse YAML front matter (simple line-by-line, no PyYAML dep)
    fm_match = _FRONT_MATTER_RE.match(raw)
    if fm_match:
        fm_text = fm_match.group(1)
        raw = raw[fm_match.end():]  # strip front matter from content
        for line in fm_text.splitlines():
            if ":" not in line:
                continue
            key, _, val = line.partition(":")
            key = key.strip().lower()
            val = val.strip()
            if key == "name" and val:
                name = val
            elif key == "description" and val:
                description = val
            elif key == "trigger_keywords":
                trigger_keywords = _parse_yaml_list(val)
            elif key == "trigger_file_patterns":
                trigger_file_patterns = _parse_yaml_list(val)
            elif key == "tool_restrictions":
                tool_restrictions = _parse_yaml_list(val)
            elif key in ("apply_to", "applyto"):
                apply_to = _parse_yaml_list(val)
            elif key in ("when_to_use", "whentouse"):
                when_to_use = val

    if not raw.strip():
        return None

    # Derive description from first non-empty line if not in front matter
    if not description:
        for line in raw.splitlines():
            clean = line.lstrip("#").strip()
            if clean:
                description = clean[:200]
                break

    return SkillConfig(
        org_id=org_id,
        name=name,
        description=description,
        content=raw.strip(),
        trigger_keywords=trigger_keywords,
        trigger_file_patterns=trigger_file_patterns,
        tool_restrictions=tool_restrictions,
        apply_to=apply_to,
        when_to_use=when_to_use,
        source=SkillSource.FILESYSTEM,
    )


def _parse_yaml_list(value: str) -> list[str]:
    """Parse a simple YAML inline list like ``[a, b, c]`` or ``a, b, c``."""
    value = value.strip()
    if value.startswith("["):
        value = value.strip("[]")
    return [item.strip().strip("\"'") for item in value.split(",") if item.strip()]


def invalidate_cache(org_id: str | None = None) -> None:
    """Invalidate cached skills for an org or all orgs."""
    if org_id:
        _cache.pop(org_id, None)
    else:
        _cache.clear()


def invalidate_fs_cache() -> None:
    """Invalidate the in-memory filesystem skills cache (e.g. after a reload)."""
    _fs_cache.clear()


# ---------------------------------------------------------------------------
# applyTo glob matching (CC parity)
# ---------------------------------------------------------------------------


def skill_applies_to_file(skill: SkillConfig, file_path: str) -> bool:
    """Return True if the skill's apply_to patterns match the given file path.

    If apply_to is empty, the skill applies to all files (no restrictions).
    Patterns use fnmatch-style globbing (``**`` for recursive).
    """
    if not skill.apply_to:
        return True  # no restriction → always applies
    for pattern in skill.apply_to:
        if fnmatch.fnmatch(file_path, pattern):
            return True
    return False


def filter_skills_for_file(
    skills: list[SkillConfig], file_path: str
) -> list[SkillConfig]:
    """Return only skills whose apply_to matches the given file path."""
    return [s for s in skills if skill_applies_to_file(s, file_path)]


# ---------------------------------------------------------------------------
# Argument substitution (CC parity)
# ---------------------------------------------------------------------------


def substitute_arguments(
    content: str,
    args: list[str] | None = None,
    skill_dir: str = "",
    session_id: str = "",
) -> str:
    """Expand ${N}, ${arguments}, ${SKILL_DIR}, ${SESSION_ID} in skill content."""
    args = args or []

    def _replace(m: re.Match) -> str:
        key = m.group(1)
        if key == "arguments":
            return " ".join(args)
        if key == "SKILL_DIR":
            return skill_dir
        if key == "SESSION_ID":
            return session_id
        if key.isdigit():
            idx = int(key) - 1  # 1-based
            if 0 <= idx < len(args):
                return args[idx]
            return ""  # missing positional → empty
        return m.group(0)  # unknown → keep as-is

    return _ARG_RE.sub(_replace, content)
