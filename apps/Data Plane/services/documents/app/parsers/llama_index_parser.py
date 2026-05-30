"""LlamaIndex-based document parser — Phase 2.1.

Wraps LlamaIndex file readers to extract structured text from
formats not covered by the legacy parsers (CSV, Markdown, EPUB,
PPTX, XLSX) while also providing improved extraction quality for
PDF and DOCX.

Falls back gracefully to legacy parsers if LlamaIndex is unavailable.
"""
from __future__ import annotations

import io
import logging
import tempfile
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

_LLAMA_AVAILABLE = False
try:
    from llama_index.core import SimpleDirectoryReader  # type: ignore[import-untyped]

    _LLAMA_AVAILABLE = True
except ImportError:
    logger.info("llama_index not installed — LlamaIndex parser disabled")


# Map content types to file extensions that LlamaIndex recognises
_CONTENT_TYPE_TO_EXT: dict[str, str] = {
    "application/pdf": ".pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document": ".docx",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet": ".xlsx",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation": ".pptx",
    "application/epub+zip": ".epub",
    "text/csv": ".csv",
    "text/markdown": ".md",
    "text/plain": ".txt",
    "text/html": ".html",
}


def is_available() -> bool:
    """Return True if LlamaIndex readers are importable."""
    return _LLAMA_AVAILABLE


def supported_content_types() -> list[str]:
    """Content types this parser can handle."""
    return list(_CONTENT_TYPE_TO_EXT.keys())


def extract_text(data: bytes, content_type: str) -> str:
    """Extract text from *data* using LlamaIndex readers.

    Writes data to a temp file (LlamaIndex readers require a file path),
    runs SimpleDirectoryReader, and returns concatenated document text.
    """
    if not _LLAMA_AVAILABLE:
        raise RuntimeError("LlamaIndex is not installed")

    ext = _CONTENT_TYPE_TO_EXT.get(content_type, "")
    if not ext:
        raise ValueError(f"Unsupported content_type for LlamaIndex: {content_type}")

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp_path = Path(tmpdir) / f"document{ext}"
        tmp_path.write_bytes(data)

        documents = SimpleDirectoryReader(
            input_files=[str(tmp_path)],
        ).load_data()

    texts = [doc.text.strip() for doc in documents if doc.text and doc.text.strip()]
    result = "\n\n".join(texts)
    logger.debug(
        "llama_index_extracted content_type=%s docs=%d chars=%d",
        content_type,
        len(documents),
        len(result),
    )
    return result
