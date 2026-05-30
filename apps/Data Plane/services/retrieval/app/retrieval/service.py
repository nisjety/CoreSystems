from __future__ import annotations

from typing import Any, Optional

from app.events.publisher import publish_search_executed
from app.quota_enforcement_handler import get_quota_blocker
from app.retrieval.pipeline import retrieve


class QuotaExceededError(Exception):
    pass


async def execute_retrieval(
    *,
    org_id: str,
    query: str,
    document_types: Optional[list[str]] = None,
    departments: Optional[list[str]] = None,
    languages: Optional[list[str]] = None,
    document_ids: Optional[list[str]] = None,
    region: Optional[str] = None,
    top_k: Optional[int] = None,
    top_n: Optional[int] = None,
    context_window: Optional[int] = None,
) -> dict[str, Any]:
    normalized_query = query.strip()
    if not normalized_query:
        raise ValueError("query cannot be empty or whitespace only")

    if get_quota_blocker().is_blocked(org_id):
        raise QuotaExceededError(
            f"Organisation {org_id!r} has exceeded its quota limit. Upgrade your plan to continue."
        )

    result = await retrieve(
        org_id=org_id,
        query=normalized_query,
        document_types=document_types,
        departments=departments,
        languages=languages,
        document_ids=document_ids,
        region=region,
        top_k=top_k,
        top_n=top_n,
        context_window=context_window,
    )

    await publish_search_executed(
        org_id=org_id,
        query=normalized_query,
        result_count=len(result["facts"]),
    )

    return result