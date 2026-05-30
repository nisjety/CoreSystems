"""System prompt composer — injects memory context and org instructions.

Implements the CC pattern of templated system prompt composition:
  <memory>
  ## key1
  content1
  ## key2
  content2
  </memory>

  {base_prompt}
"""

from __future__ import annotations

import hashlib
import logging
from functools import lru_cache

logger = logging.getLogger(__name__)

# Simple memoization cache: (session_id, snippets_hash) → composed prompt
_prompt_cache: dict[tuple[str, str], str] = {}
_MAX_CACHE = 128


def compose_system_prompt(
    base_prompt: str,
    memory_snippets: list[str],
    session_id: str = "",
) -> str:
    """Compose the full system prompt with memory context injected.

    Memoized on (session_id, hash(snippets)) to avoid recomposition.
    """
    if not memory_snippets:
        return base_prompt

    snippets_hash = _hash_snippets(memory_snippets)
    cache_key = (session_id, snippets_hash)

    if cache_key in _prompt_cache:
        return _prompt_cache[cache_key]

    memory_block = "\n\n".join(memory_snippets)
    composed = f"<memory>\n{memory_block}\n</memory>\n\n{base_prompt}"

    # Evict oldest if cache full
    if len(_prompt_cache) >= _MAX_CACHE:
        oldest = next(iter(_prompt_cache))
        del _prompt_cache[oldest]

    _prompt_cache[cache_key] = composed
    return composed


def invalidate_prompt_cache() -> None:
    """Clear the prompt composition cache."""
    _prompt_cache.clear()


def _hash_snippets(snippets: list[str]) -> str:
    """Hash memory snippets for cache key."""
    combined = "\n---\n".join(snippets)
    return hashlib.sha256(combined.encode()).hexdigest()[:16]
