"""DOCX parser using python-docx."""

from __future__ import annotations

import io
import logging

from docx import Document

logger = logging.getLogger(__name__)


def extract_text(data: bytes) -> str:
    """Extract all paragraph text from a docx byte stream."""
    doc = Document(io.BytesIO(data))
    paragraphs = [p.text.strip() for p in doc.paragraphs if p.text.strip()]
    result = "\n\n".join(paragraphs)
    logger.debug("docx_extracted paragraphs=%d chars=%d", len(paragraphs), len(result))
    return result
