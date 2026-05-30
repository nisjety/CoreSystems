"""Goal normalisation — strips specifics to produce a canonical task_pattern.

The pattern is stored on ``agent_trajectories.task_pattern`` and is used to:
- Cluster similar runs together for skill synthesis
- Drive org-level analytics (which task types succeed / fail)
- Route Letta memory updates efficiently

Design principles:
- Deterministic: same goal → same pattern, always
- Idempotent: can be called multiple times without side-effects
- Short: ≤ 128 chars so it fits in a ``VARCHAR(128)`` column index
"""

from __future__ import annotations

import re

# ---------------------------------------------------------------------------
# Regex-based normalisation rules (applied in order)
# ---------------------------------------------------------------------------

# Each entry: (compiled_pattern, replacement)
# Applied sequentially; first match wins for verb extraction then all run.
_STRIP_RULES: list[tuple[re.Pattern[str], str]] = [
    # Quoted strings, UUIDs, hex IDs
    (re.compile(r'"[^"]{0,200}"'), "QUOTED"),
    (re.compile(r"'[^']{0,200}'"), "QUOTED"),
    (re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b", re.I), "ID"),
    (re.compile(r"\b[0-9a-f]{24,}\b", re.I), "ID"),
    # URLs
    (re.compile(r"https?://\S+"), "URL"),
    # File paths
    (re.compile(r"(/[\w./-]+){2,}"), "PATH"),
    (re.compile(r"(\w+/)(\w+/)+\w+(\.\w+)?"), "PATH"),
    # Numbers
    (re.compile(r"\b\d[\d,._]*\b"), "NUM"),
    # E-mail addresses
    (re.compile(r"\S+@\S+\.\S+"), "EMAIL"),
    # Collapse whitespace
    (re.compile(r"\s+"), " "),
]

# Verb → canonical task category mapping (lowercase)
_VERB_MAP: dict[str, str] = {
    # summarise family
    "summarise": "summarise",
    "summarize": "summarise",
    "summary": "summarise",
    "tldr": "summarise",
    # analyse family
    "analyse": "analyse",
    "analyze": "analyse",
    "analysis": "analyse",
    "audit": "analyse",
    "review": "analyse",
    "evaluate": "analyse",
    "assess": "analyse",
    # generate / write
    "write": "generate",
    "generate": "generate",
    "create": "generate",
    "draft": "generate",
    "compose": "generate",
    "produce": "generate",
    "author": "generate",
    # search / find
    "search": "search",
    "find": "search",
    "look": "search",
    "lookup": "search",
    "retrieve": "search",
    "fetch": "search",
    "query": "search",
    # code family
    "code": "code",
    "implement": "code",
    "refactor": "code",
    "debug": "code",
    "fix": "code",
    "test": "code",
    "build": "code",
    # translate / convert
    "translate": "convert",
    "convert": "convert",
    "transform": "convert",
    "format": "convert",
    "parse": "convert",
    # classify / categorise
    "classify": "classify",
    "categorise": "classify",
    "categorize": "classify",
    "label": "classify",
    "tag": "classify",
    # extract
    "extract": "extract",
    "scrape": "extract",
    "pull": "extract",
    # plan / organise
    "plan": "plan",
    "organise": "plan",
    "organize": "plan",
    "schedule": "plan",
    "prioritise": "plan",
    "prioritize": "plan",
    # explain
    "explain": "explain",
    "describe": "explain",
    "document": "explain",
    "clarify": "explain",
    # compare
    "compare": "compare",
    "contrast": "compare",
    "benchmark": "compare",
    # notify / send
    "send": "notify",
    "notify": "notify",
    "alert": "notify",
    "email": "notify",
    "message": "notify",
    # answer / ask
    "answer": "answer",
    "ask": "answer",
    "respond": "answer",
    "reply": "answer",
    # delete / remove
    "delete": "delete",
    "remove": "delete",
    "clean": "delete",
    "prune": "delete",
}

# Object / domain classifier words — appended after the verb category
_OBJECT_KEYWORDS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"\b(document|pdf|file|report|article|essay|text|page)\b", re.I), "document"),
    (re.compile(r"\b(code|function|method|class|module|script|file|repo|repository)\b", re.I), "code"),
    (re.compile(r"\b(data|dataset|csv|json|table|database|db|sql)\b", re.I), "data"),
    (re.compile(r"\b(email|message|slack|notification|thread)\b", re.I), "message"),
    (re.compile(r"\b(image|photo|diagram|chart|figure|screenshot)\b", re.I), "image"),
    (re.compile(r"\b(api|endpoint|url|request|response|webhook)\b", re.I), "api"),
    (re.compile(r"\b(pipeline|workflow|process|task|job|run)\b", re.I), "workflow"),
    (re.compile(r"\b(user|customer|client|account|org|team)\b", re.I), "entity"),
    (re.compile(r"\b(meeting|calendar|schedule|event|booking)\b", re.I), "calendar"),
]


def normalize_goal(goal: str) -> str:
    """Return a short canonical task_pattern for ``goal``.

    Examples:
        "Summarise the PDF at /reports/q3.pdf" → "summarise_document"
        "Write unit tests for class UserService" → "generate_code"
        "Find customers who haven't logged in since 2024-01-01" → "search_entity"
        "Deploy the staging pipeline" → "workflow"
    """
    if not goal or not goal.strip():
        return "unknown"

    normalized = goal.strip()

    # Apply stripping rules
    for pattern, replacement in _STRIP_RULES:
        normalized = pattern.sub(replacement, normalized)

    normalized_lower = normalized.lower()
    words = re.findall(r"[a-z]+", normalized_lower)

    # Identify verb category from first N words
    verb_category: str | None = None
    for word in words[:8]:
        if word in _VERB_MAP:
            verb_category = _VERB_MAP[word]
            break

    # Identify object domain
    object_domain: str | None = None
    for pat, domain in _OBJECT_KEYWORDS:
        if pat.search(goal):  # search original goal for domain hints
            object_domain = domain
            break

    # Compose pattern
    if verb_category and object_domain:
        pattern_str = f"{verb_category}_{object_domain}"
    elif verb_category:
        pattern_str = verb_category
    elif object_domain:
        pattern_str = object_domain
    else:
        # Fall back to first content word
        content_words = [w for w in words if len(w) > 3]
        pattern_str = content_words[0] if content_words else "generic"

    # Truncate to fit VARCHAR(128)
    return pattern_str[:128]
