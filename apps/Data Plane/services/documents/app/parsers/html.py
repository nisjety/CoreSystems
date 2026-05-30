"""HTML parser using BeautifulSoup — strips tags, extracts text."""

from __future__ import annotations

import logging

from bs4 import BeautifulSoup

logger = logging.getLogger(__name__)

_REMOVE_TAGS = frozenset({"script", "style", "nav", "footer", "header", "noscript"})


def extract_text(data: bytes, encoding: str = "utf-8") -> str:
    """Extract visible text from an HTML byte stream."""
    soup = BeautifulSoup(data.decode(encoding, errors="replace"), "lxml")

    # Remove non-content elements
    for tag in soup.find_all(_REMOVE_TAGS):
        tag.decompose()

    text = soup.get_text(separator="\n", strip=True)
    logger.debug("html_extracted chars=%d", len(text))
    return text
