"""Persistence layer for run_costs and analytics_events."""
from __future__ import annotations

import json
from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List, Optional
from uuid import UUID

from app.database import get_pool


async def insert_run_cost_turn(
    *,
    run_id: str,
    org_id: str,
    model: str,
    input_tokens: int,
    output_tokens: int,
    cost_usd: Decimal,
    turn_index: Optional[int] = None,
    metadata: Optional[Dict[str, Any]] = None,
) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO run_costs (
                run_id, org_id, model, input_tokens, output_tokens,
                cost_usd, turn_index, metadata
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb)
            """,
            run_id,
            org_id,
            model,
            int(input_tokens),
            int(output_tokens),
            cost_usd,
            turn_index,
            json.dumps(metadata or {}),
        )


async def get_run_cost_aggregate(run_id: str) -> Dict[str, Any]:
    pool = await get_pool()
    async with pool.acquire() as conn:
        row = await conn.fetchrow(
            """
            SELECT
                run_id,
                COALESCE(MIN(org_id), '') AS org_id,
                COUNT(*)::int AS turn_count,
                COALESCE(SUM(input_tokens), 0)::bigint AS total_input_tokens,
                COALESCE(SUM(output_tokens), 0)::bigint AS total_output_tokens,
                COALESCE(SUM(cost_usd), 0) AS total_cost_usd
            FROM run_costs
            WHERE run_id = $1
            GROUP BY run_id
            """,
            run_id,
        )
        if row is None:
            return {
                "run_id": run_id,
                "org_id": "",
                "turn_count": 0,
                "total_input_tokens": 0,
                "total_output_tokens": 0,
                "total_cost_usd": Decimal("0"),
            }
        return dict(row)


async def insert_analytics_event(
    *,
    event_id: UUID,
    event_type: str,
    org_id: str,
    run_id: Optional[str] = None,
    user_id: Optional[str] = None,
    agent_id: Optional[str] = None,
    props: Optional[Dict[str, Any]] = None,
    ts: Optional[datetime] = None,
) -> None:
    pool = await get_pool()
    async with pool.acquire() as conn:
        await conn.execute(
            """
            INSERT INTO analytics_events (
                event_id, event_type, org_id, run_id, user_id,
                agent_id, props, ts
            ) VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, COALESCE($8, NOW()))
            """,
            event_id,
            event_type,
            org_id,
            run_id,
            user_id,
            agent_id,
            json.dumps(props or {}),
            ts,
        )


async def query_analytics_events(
    *,
    org_id: str,
    event_type: Optional[str] = None,
    since: Optional[datetime] = None,
    until: Optional[datetime] = None,
    limit: int = 100,
) -> List[Dict[str, Any]]:
    pool = await get_pool()
    query = [
        "SELECT event_id, event_type, org_id, run_id, user_id, agent_id, props, ts",
        "FROM analytics_events WHERE org_id = $1",
    ]
    params: List[Any] = [org_id]
    if event_type:
        params.append(event_type)
        query.append(f"AND event_type = ${len(params)}")
    if since:
        params.append(since)
        query.append(f"AND ts >= ${len(params)}")
    if until:
        params.append(until)
        query.append(f"AND ts < ${len(params)}")
    params.append(int(limit))
    query.append(f"ORDER BY ts DESC LIMIT ${len(params)}")
    sql = " ".join(query)

    async with pool.acquire() as conn:
        rows = await conn.fetch(sql, *params)
        return [dict(r) for r in rows]
