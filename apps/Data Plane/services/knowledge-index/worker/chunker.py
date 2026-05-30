"""
Chunker — splits raw document content into overlapping text chunks.

Phase 2.2: Token-aware chunking using tiktoken to match the
text-embedding-3-large tokenizer. This ensures chunk sizes align with
model token limits rather than arbitrary character counts.

Strategy:
  1. Tokenize the full document once with tiktoken (cl100k_base).
  2. Split by paragraph boundaries (double newlines) to preserve
     semantic structure.
  3. If a paragraph exceeds chunk_size tokens, split by sentences.
  4. Greedily pack sentences into chunks respecting token budget.
  5. Overlap is in *tokens*, not characters.

The output is a list of (chunk_index, chunk_text) tuples.
"""
from __future__ import annotations

import logging
import re
from typing import List, Tuple

logger = logging.getLogger(__name__)

_encoder = None


def _get_encoder():
    """Lazy-load the tiktoken encoder (cl100k_base for text-embedding-3-large)."""
    global _encoder
    if _encoder is None:
        try:
            import tiktoken
            _encoder = tiktoken.get_encoding("cl100k_base")
        except ImportError:
            logger.warning("tiktoken not installed — falling back to char-based chunking")
    return _encoder


def _count_tokens(text: str) -> int:
    """Count tokens in *text*. Falls back to len(text)//4 if tiktoken unavailable."""
    enc = _get_encoder()
    if enc is not None:
        return len(enc.encode(text))
    # rough approximation: 1 token ≈ 4 chars for English
    return len(text) // 4


def _decode_tokens(token_ids: list[int]) -> str:
    """Decode token IDs back to text."""
    enc = _get_encoder()
    if enc is None:
        raise RuntimeError("tiktoken encoder not available for decode")
    return enc.decode(token_ids)


def chunk_text(
    text: str,
    chunk_size: int = 256,
    chunk_overlap: int = 32,
) -> List[Tuple[int, str]]:
    """Split *text* into overlapping token-aware chunks.

    Args:
        text: Document text to chunk.
        chunk_size: Maximum tokens per chunk (default 256).
        chunk_overlap: Token overlap between consecutive chunks (default 32).

    Returns:
        List of (chunk_index, chunk_text) tuples.
    """
    if not text or not text.strip():
        return []

    # Step 1: paragraph-aware pre-split
    paragraphs = [p.strip() for p in re.split(r"\n\s*\n", text) if p.strip()]

    # Step 2: flatten large paragraphs into sentences
    segments: List[str] = []
    for para in paragraphs:
        if _count_tokens(para) <= chunk_size:
            segments.append(para)
        else:
            # Split on sentence boundaries
            parts = re.split(r"(?<=[.!?])\s+", para)
            segments.extend(p.strip() for p in parts if p.strip())

    # Step 3: greedily pack segments respecting token budget with overlap
    chunks: List[str] = []
    current_segments: List[str] = []
    current_tokens = 0

    for segment in segments:
        seg_tokens = _count_tokens(segment)

        # If this single segment exceeds chunk_size, split it by token windows
        if seg_tokens > chunk_size:
            # Flush current buffer first
            if current_segments:
                chunks.append(" ".join(current_segments))
                current_segments = []
                current_tokens = 0

            enc = _get_encoder()
            if enc is not None:
                ids = enc.encode(segment)
                step = max(chunk_size - chunk_overlap, 1)
                for start in range(0, len(ids), step):
                    window = ids[start : start + chunk_size]
                    chunks.append(enc.decode(window))
            else:
                # Fallback: character-based split
                step = max((chunk_size * 4) - (chunk_overlap * 4), 1)
                for start in range(0, len(segment), step):
                    chunks.append(segment[start : start + chunk_size * 4])
            continue

        if current_tokens + seg_tokens > chunk_size and current_segments:
            chunks.append(" ".join(current_segments))

            # Overlap: keep trailing segments that fit within overlap budget
            overlap_segments: List[str] = []
            overlap_tokens = 0
            for seg in reversed(current_segments):
                seg_t = _count_tokens(seg)
                if overlap_tokens + seg_t > chunk_overlap:
                    break
                overlap_segments.insert(0, seg)
                overlap_tokens += seg_t

            current_segments = overlap_segments + [segment]
            current_tokens = overlap_tokens + seg_tokens
        else:
            current_segments.append(segment)
            current_tokens += seg_tokens

    if current_segments:
        chunks.append(" ".join(current_segments))

    return [(idx, chunk) for idx, chunk in enumerate(chunks)]

