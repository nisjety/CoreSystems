"""
Data Plane audit logging.

Append-only log of every data access for GDPR compliance.
Writes to the data_plane_audit_log table in Postgres.
"""
from __future__ import annotations

import logging
from datetime import datetime, timezone
from typing import Optional

from sqlalchemy import text
from sqlalchemy.ext.asyncio import AsyncSession

logger = logging.getLogger(__name__)


async def log_access(
    db: AsyncSession,
    *,
    user_id: str,
    org_id: str,
    action: str,
    resource_type: str,
    resource_id: Optional[str] = None,
    ip_address: Optional[str] = None,
    user_agent: Optional[str] = None,
    details: Optional[str] = None,
) -> None:
    """
    Write an audit log entry. Fire-and-forget — errors are logged, not raised.
    """
    try:
        await db.execute(
            text(
                """
                INSERT INTO data_plane_audit_log
                    (user_id, org_id, action, resource_type, resource_id,
                     ip_address, user_agent, details, created_at)
                VALUES
                    (:user_id, :org_id, :action, :resource_type, :resource_id,
                     :ip_address, :user_agent, :details, :created_at)
                """
            ),
            {
                "user_id": user_id,
                "org_id": org_id,
                "action": action,
                "resource_type": resource_type,
                "resource_id": resource_id,
                "ip_address": ip_address,
                "user_agent": user_agent,
                "details": details,
                "created_at": datetime.now(timezone.utc),
            },
        )
        await db.commit()
    except Exception as exc:
        logger.warning("audit log write failed (non-fatal): %s", exc)
