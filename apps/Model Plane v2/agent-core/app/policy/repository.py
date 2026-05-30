"""Policy repository — CRUD for org policy limits.

Uses the same asyncpg pool as the rest of agent-core.
"""

from __future__ import annotations

import json
import logging
from typing import Any

import asyncpg

from app.policy import OrgPolicyLimits

logger = logging.getLogger(__name__)


async def get_org_policy(
    conn: asyncpg.Connection, org_id: str
) -> OrgPolicyLimits | None:
    """Return the policy limits for an org, or None if not configured."""
    row = await conn.fetchrow(
        "SELECT * FROM org_policy_limits WHERE org_id = $1", org_id
    )
    if row is None:
        return None
    return _row_to_policy(row)


async def upsert_org_policy(
    conn: asyncpg.Connection, policy: OrgPolicyLimits
) -> None:
    """Insert or update the policy limits for an org."""
    allowed_tools_json = json.dumps(policy.allowed_tools)
    await conn.execute(
        """
        INSERT INTO org_policy_limits (
            org_id,
            max_concurrent_runs,
            max_concurrent_runs_per_user,
            max_tokens_per_run,
            max_cost_usd_per_run,
            max_cost_usd_monthly,
            max_actions_per_run,
            allowed_tools,
            cost_limit_action
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        ON CONFLICT (org_id) DO UPDATE SET
            max_concurrent_runs = EXCLUDED.max_concurrent_runs,
            max_concurrent_runs_per_user = EXCLUDED.max_concurrent_runs_per_user,
            max_tokens_per_run = EXCLUDED.max_tokens_per_run,
            max_cost_usd_per_run = EXCLUDED.max_cost_usd_per_run,
            max_cost_usd_monthly = EXCLUDED.max_cost_usd_monthly,
            max_actions_per_run = EXCLUDED.max_actions_per_run,
            allowed_tools = EXCLUDED.allowed_tools,
            cost_limit_action = EXCLUDED.cost_limit_action,
            updated_at = now()
        """,
        policy.org_id,
        policy.max_concurrent_runs,
        policy.max_concurrent_runs_per_user,
        policy.max_tokens_per_run,
        policy.max_cost_usd_per_run,
        policy.max_cost_usd_monthly,
        policy.max_actions_per_run,
        allowed_tools_json,
        policy.cost_limit_action.value,
    )


async def delete_org_policy(conn: asyncpg.Connection, org_id: str) -> bool:
    """Delete the policy for an org.  Returns True if a row was deleted."""
    result = await conn.execute(
        "DELETE FROM org_policy_limits WHERE org_id = $1", org_id
    )
    return result.endswith("1")


# ──────────────────────────────────────────────────────────────────────────
# Internal helpers
# ──────────────────────────────────────────────────────────────────────────


def _row_to_policy(row: Any) -> OrgPolicyLimits:
    allowed_tools: list[str] = []
    raw = row["allowed_tools"]
    if raw:
        try:
            allowed_tools = json.loads(raw)
        except (ValueError, TypeError):
            allowed_tools = []

    return OrgPolicyLimits(
        org_id=row["org_id"],
        max_concurrent_runs=int(row["max_concurrent_runs"]),
        max_concurrent_runs_per_user=int(row["max_concurrent_runs_per_user"]),
        max_tokens_per_run=int(row["max_tokens_per_run"]),
        max_cost_usd_per_run=float(row["max_cost_usd_per_run"]),
        max_cost_usd_monthly=float(row["max_cost_usd_monthly"]),
        max_actions_per_run=int(row["max_actions_per_run"]),
        allowed_tools=allowed_tools,
        cost_limit_action=row["cost_limit_action"],
    )
