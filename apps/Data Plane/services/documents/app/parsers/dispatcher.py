"""Parser dispatcher — routes content_type to the right extractor.

Phase 2.1: attempts LlamaIndex first for richer extraction (tables,
structure, metadata), then falls back to legacy parsers.
"""

from __future__ import annotations

import logging

from app.parsers import docx, html, pdf
from app.parsers import llama_index_parser

logger = logging.getLogger(__name__)

_LEGACY_PARSERS: dict[str, object] = {
    "application/pdf": pdf,
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": docx,
    "text/html": html,
}


def parse(data: bytes, content_type: str) -> str:
    """Extract text from raw document bytes.

    Strategy:
      1. If LlamaIndex is available and supports the content type, use it.
      2. Otherwise fall back to legacy parsers.
      3. Last resort: UTF-8 decode.
    """
    # --- LlamaIndex path (Phase 2.1) ---
    if llama_index_parser.is_available() and content_type in llama_index_parser.supported_content_types():
        try:
            result = llama_index_parser.extract_text(data, content_type)
            if result and result.strip():
                return result
            logger.warning("llama_index returned empty for %s, falling back to legacy", content_type)
        except Exception:
            logger.exception("llama_index_parser failed for %s, falling back to legacy", content_type)

    # --- Legacy parser path ---
    legacy_parser = _LEGACY_PARSERS.get(content_type)
    if legacy_parser is not None:
        return legacy_parser.extract_text(data)  # type: ignore[union-attr]

    # Fallback: treat as plain text
    return data.decode("utf-8", errors="replace")
