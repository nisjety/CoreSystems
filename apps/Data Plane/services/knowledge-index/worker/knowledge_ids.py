from __future__ import annotations

from uuid import NAMESPACE_URL, uuid5


def build_knowledge_id(document_id: str, chunk_index: int) -> str:
    """Return a deterministic ID so retries upsert the same knowledge unit."""
    return str(uuid5(NAMESPACE_URL, f"dataplane:{document_id}:{chunk_index}"))