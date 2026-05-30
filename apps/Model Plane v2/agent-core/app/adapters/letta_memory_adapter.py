"""Letta memory adapter — memory read/write via self-hosted Letta server.

Phase 5: Real integration using ``letta-client`` SDK talking to the
self-hosted Letta server container.
"""

from __future__ import annotations

import logging
from typing import Any

from app.domain import AgentAction, RunRecord

logger = logging.getLogger(__name__)

_WIRED = True

# Module-level cached Letta client (lazy init).
_letta_client: Any = None


def _get_client() -> Any:
    """Return a cached Letta client, created lazily."""
    global _letta_client
    if _letta_client is not None:
        return _letta_client

    from app.config import Settings

    settings = Settings()
    if not settings.letta_enabled:
        raise RuntimeError("Letta is disabled (letta_enabled=False)")

    try:
        from letta_client import Letta  # type: ignore[import-untyped]
    except ImportError:
        raise RuntimeError("letta-client is not installed")

    _letta_client = Letta(base_url=settings.letta_api_url)
    logger.info("letta_client_initialized url=%s", settings.letta_api_url)
    return _letta_client


def reset_client() -> None:
    """Reset the cached client (for testing)."""
    global _letta_client
    _letta_client = None


async def execute(action: AgentAction, run: RunRecord) -> Any:
    """Execute a Letta memory operation.

    Supported action names:
      - ``letta_memory_read``  — retrieve memory blocks for an agent
      - ``letta_memory_write`` — update a memory block
      - ``letta_memory_search`` — search agent archival memory
      - ``letta_agent_create`` — create a Letta agent for an org/session
      - ``letta_agent_message`` — send a message and get response
    """
    op = action.name
    params = action.input or {}

    try:
        if op == "letta_agent_create":
            return await _create_agent(params, run)
        if op == "letta_memory_read":
            return await _read_memory(params, run)
        if op == "letta_memory_write":
            return await _write_memory(params, run)
        if op == "letta_memory_search":
            return await _search_memory(params, run)
        if op == "letta_agent_message":
            return await _send_message(params, run)
        return {"error": f"Unknown Letta operation: {op}", "adapter": "letta_memory"}
    except RuntimeError as exc:
        logger.warning("letta_adapter_error op=%s error=%s", op, exc)
        return {"error": str(exc), "adapter": "letta_memory", "status": "error"}
    except Exception as exc:
        logger.exception("letta_adapter_unexpected op=%s", op)
        return {"error": str(exc), "adapter": "letta_memory", "status": "error"}


async def _create_agent(params: dict[str, Any], run: RunRecord) -> dict[str, Any]:
    """Create a Letta agent with memory blocks for the org/session."""
    client = _get_client()

    name = params.get("name", f"agent-{run.org_id}-{run.id[:8]}")
    model = params.get("model", "gpt-4o-mini")
    embedding_model = params.get("embedding_model", "text-embedding-3-small")

    memory_blocks = []
    for block in params.get("memory_blocks", []):
        from letta_client import CreateBlock  # type: ignore[import-untyped]
        memory_blocks.append(CreateBlock(
            label=block.get("label", "human"),
            value=block.get("value", ""),
            limit=block.get("limit", 5000),
        ))

    # Default blocks if none provided
    if not memory_blocks:
        from letta_client import CreateBlock  # type: ignore[import-untyped]
        memory_blocks = [
            CreateBlock(label="human", value=f"User from org {run.org_id}", limit=5000),
            CreateBlock(label="persona", value="I am a helpful AI assistant.", limit=5000),
        ]

    agent = client.agents.create(
        name=name,
        model=model,
        embedding=embedding_model,
        memory_blocks=memory_blocks,
    )
    logger.info("letta_agent_created id=%s name=%s", agent.id, name)
    return {"agent_id": agent.id, "name": name, "status": "created"}


async def _read_memory(params: dict[str, Any], run: RunRecord) -> dict[str, Any]:
    """Read memory blocks for a Letta agent."""
    client = _get_client()
    agent_id = params.get("agent_id")
    if not agent_id:
        return {"error": "agent_id is required", "adapter": "letta_memory"}

    blocks = client.agents.blocks.list(agent_id=agent_id)
    return {
        "agent_id": agent_id,
        "blocks": [
            {"label": b.label, "value": b.value, "limit": b.limit}
            for b in blocks
        ],
    }


async def _write_memory(params: dict[str, Any], run: RunRecord) -> dict[str, Any]:
    """Update a memory block on a Letta agent."""
    client = _get_client()
    agent_id = params.get("agent_id")
    block_label = params.get("label")
    new_value = params.get("value")

    if not agent_id or not block_label:
        return {"error": "agent_id and label are required", "adapter": "letta_memory"}

    # Find the block by label
    blocks = client.agents.blocks.list(agent_id=agent_id)
    target = next((b for b in blocks if b.label == block_label), None)
    if target is None:
        return {"error": f"Block '{block_label}' not found", "adapter": "letta_memory"}

    updated = client.blocks.update(
        block_id=target.id,
        value=new_value,
    )
    logger.info("letta_memory_updated agent=%s label=%s", agent_id, block_label)
    return {"agent_id": agent_id, "label": block_label, "updated": True, "value": updated.value}


async def _search_memory(params: dict[str, Any], run: RunRecord) -> dict[str, Any]:
    """Search archival memory for a Letta agent."""
    client = _get_client()
    agent_id = params.get("agent_id")
    query = params.get("query", "")

    if not agent_id:
        return {"error": "agent_id is required", "adapter": "letta_memory"}

    passages = client.agents.archival_memory.list(
        agent_id=agent_id,
    )
    # Client-side filtering (Letta API may support server-side search)
    results = []
    query_lower = query.lower()
    for p in passages:
        if query_lower in (p.text or "").lower():
            results.append({"text": p.text, "id": p.id})

    return {"agent_id": agent_id, "query": query, "results": results[:10]}


async def _send_message(params: dict[str, Any], run: RunRecord) -> dict[str, Any]:
    """Send a message to a Letta agent and return the response."""
    client = _get_client()
    agent_id = params.get("agent_id")
    message = params.get("message", "")

    if not agent_id or not message:
        return {"error": "agent_id and message are required", "adapter": "letta_memory"}

    response = client.agents.messages.create(
        agent_id=agent_id,
        messages=[{"role": "user", "content": message}],
    )
    # Extract assistant messages
    assistant_texts = []
    for msg in response.messages:
        if hasattr(msg, "content") and msg.content:
            assistant_texts.append(msg.content)

    return {
        "agent_id": agent_id,
        "response": " ".join(assistant_texts) if assistant_texts else "(no response)",
    }
