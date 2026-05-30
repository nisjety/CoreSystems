"""
Hard Filters — build Qdrant filter conditions from retrieval request metadata.

These are applied BEFORE any vector similarity is computed.
Hard filters are ZERO-COMPROMISE:
  - org_id is ALWAYS enforced (multi-tenancy)
  - document_types, departments, languages reduce the search space
  - document_ids allow pinning retrieval to specific sources

AI-Core never bypasses this layer.
"""
from __future__ import annotations

from typing import Any, Dict, List, Optional

from qdrant_client.http import models as qm


def build_qdrant_filter(
    org_id: str,
    document_types: Optional[List[str]] = None,
    departments: Optional[List[str]] = None,
    languages: Optional[List[str]] = None,
    document_ids: Optional[List[str]] = None,
    region: Optional[str] = None,
) -> qm.Filter:
    """
    Compose a Qdrant Filter that enforces hard constraints.

    org_id is always a MUST condition — cross-tenant leakage is impossible.
    All other filters are optional ANDs appended to the must list.
    """
    must: List[qm.Condition] = [
        qm.FieldCondition(key="org_id", match=qm.MatchValue(value=org_id))
    ]

    if document_types:
        must.append(
            qm.FieldCondition(key="type", match=qm.MatchAny(any=document_types))
        )

    if departments:
        must.append(
            qm.FieldCondition(key="department", match=qm.MatchAny(any=departments))
        )

    if languages:
        must.append(
            qm.FieldCondition(key="language", match=qm.MatchAny(any=languages))
        )

    if document_ids:
        must.append(
            qm.FieldCondition(key="document_id", match=qm.MatchAny(any=document_ids))
        )

    if region:
        must.append(
            qm.FieldCondition(key="region", match=qm.MatchValue(value=region))
        )

    return qm.Filter(must=must)
