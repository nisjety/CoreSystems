"""PDF parser using pypdf."""

from __future__ import annotations

import io
import logging

from pypdf import PdfReader

logger = logging.getLogger(__name__)


def extract_text(data: bytes) -> str:
    """Extract all text from a PDF byte stream."""
    reader = PdfReader(io.BytesIO(data))
    pages: list[str] = []
    for page in reader.pages:
        text = page.extract_text()
        if text:
            pages.append(text.strip())
    result = "\n\n".join(pages)
    logger.debug("pdf_extracted pages=%d chars=%d", len(reader.pages), len(result))
    return result
