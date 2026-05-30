"""
Quota Enforcement Handler — updates organization quotas when plan changes.

Subscribes to aqencia.controlplane.org.plan_changed and updates org_quotas table.
Enforces plan-tier limits:
  - free:         100 docs, 10K API calls/month, 1 GB storage, 1 concurrent user
  - professional: 1K docs, 100K API calls/month, 50 GB storage, 10 concurrent users
  - enterprise:   unlimited docs, unlimited API calls, unlimited storage, unlimited users
"""
from __future__ import annotations

import logging
from threading import Lock
from typing import Optional

from sqlalchemy import select, insert, update
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.config import settings

logger = logging.getLogger(__name__)

# Common plan name aliases for normalization
PLAN_ALIASES = {
    "pro": "professional",
    "starter": "free",
    "basic": "free",
    "business": "professional",
    "unlimited": "enterprise",
}

# Plan tier limits: (documents_limit, api_calls_per_month, storage_gb, concurrent_users, custom_models)
PLAN_LIMITS = {
    "free": {
        "documents_limit": 100,
        "api_calls_per_month": 10000,
        "storage_gb": 1.0,
        "concurrent_users": 1,
        "custom_models": False,
    },
    "professional": {
        "documents_limit": 1000,
        "api_calls_per_month": 100000,
        "storage_gb": 50.0,
        "concurrent_users": 10,
        "custom_models": True,
    },
    "enterprise": {
        "documents_limit": 999999,
        "api_calls_per_month": 999999999,
        "storage_gb": 999999.0,
        "concurrent_users": 999,
        "custom_models": True,
    },
}


class QuotaEnforcementHandler:
    """Handles Control Plane org.plan_changed events and updates quotas in Data Plane."""

    def __init__(self, database_url: str):
        """
        Initialize Quota Enforcement Handler.

        Args:
            database_url: PostgreSQL connection URL
        """
        self.engine = create_async_engine(database_url, pool_size=3, max_overflow=2)
        self.SessionLocal = async_sessionmaker(
            self.engine, expire_on_commit=False, class_=AsyncSession
        )

    async def handle(self, org_id: str, plan: str) -> bool:
        """
        Handle org.plan_changed event.

        Updates org_quotas table with limits for the new plan tier.
        Creates org_quotas entry if it doesn't exist.

        Args:
            org_id: Organization ID
            plan: Plan tier (free | professional | enterprise)

        Returns:
            True if successful (will ack message), False on error (will nack for redelivery)
        """
        try:
            # Normalize and validate plan tier
            plan = PLAN_ALIASES.get(plan, plan)
            if plan not in PLAN_LIMITS:
                logger.warning(f"Unknown plan tier {plan!r} for org_id={org_id} — skipping quota update")
                return True  # ACK the message; don't retry on permanently unknown plans

            limits = PLAN_LIMITS[plan]

            # Get or create org_quotas entry
            async with self.SessionLocal() as session:
                # Try to get existing quotas
                from sqlalchemy import text

                result = await session.execute(
                    text(
                        """
                        SELECT org_id FROM org_quotas WHERE org_id = :org_id
                        """
                    ),
                    {"org_id": org_id},
                )
                existing = result.first()

                if existing:
                    # Update existing quotas
                    await session.execute(
                        text(
                            """
                            UPDATE org_quotas
                            SET plan_tier = :plan_tier,
                                documents_limit = :documents_limit,
                                api_calls_per_month = :api_calls_per_month,
                                storage_gb = :storage_gb,
                                concurrent_users = :concurrent_users,
                                custom_models = :custom_models,
                                updated_at = NOW()
                            WHERE org_id = :org_id
                            """
                        ),
                        {
                            "org_id": org_id,
                            "plan_tier": plan,
                            "documents_limit": limits["documents_limit"],
                            "api_calls_per_month": limits["api_calls_per_month"],
                            "storage_gb": limits["storage_gb"],
                            "concurrent_users": limits["concurrent_users"],
                            "custom_models": limits["custom_models"],
                        },
                    )
                    logger.info(
                        f"quota enforcement: updated org_id={org_id} plan={plan}"
                    )
                else:
                    # Insert new quotas entry
                    await session.execute(
                        text(
                            """
                            INSERT INTO org_quotas (
                                org_id,
                                plan_tier,
                                documents_limit,
                                api_calls_per_month,
                                storage_gb,
                                concurrent_users,
                                custom_models
                            ) VALUES (
                                :org_id,
                                :plan_tier,
                                :documents_limit,
                                :api_calls_per_month,
                                :storage_gb,
                                :concurrent_users,
                                :custom_models
                            )
                            """
                        ),
                        {
                            "org_id": org_id,
                            "plan_tier": plan,
                            "documents_limit": limits["documents_limit"],
                            "api_calls_per_month": limits["api_calls_per_month"],
                            "storage_gb": limits["storage_gb"],
                            "concurrent_users": limits["concurrent_users"],
                            "custom_models": limits["custom_models"],
                        },
                    )
                    logger.info(
                        f"quota enforcement: created org_id={org_id} plan={plan}"
                    )

                await session.commit()
            return True

        except Exception as e:
            logger.error(f"Error handling org.plan_changed for {org_id}: {e}")
            return False

    async def cleanup(self) -> None:
        """Cleanup: close database engine."""
        await self.engine.dispose()


# ── Quota Blocker (in-memory) ─────────────────────────────────────────────────

class QuotaBlocker:
    """
    In-memory registry of orgs that have exceeded their quota.
    Populated by billing.quota_exceeded events; cleared on plan upgrades.
    Resets on service restart (intentional — events will re-populate on delivery).
    """

    def __init__(self) -> None:
        self._blocked: set[str] = set()
        self._lock = Lock()

    async def block(self, org_id: str, metric: str, limit: int, current: int) -> bool:
        """Mark org as quota-exceeded. Returns True (always succeeds)."""
        with self._lock:
            self._blocked.add(org_id)
        logger.warning(
            f"quota_blocker: BLOCKED org_id={org_id} "
            f"metric={metric} current={current} limit={limit}"
        )
        return True

    def unblock(self, org_id: str) -> None:
        """Remove block when org upgrades plan."""
        was_blocked = False
        with self._lock:
            if org_id in self._blocked:
                self._blocked.discard(org_id)
                was_blocked = True

        if was_blocked:
            logger.info(f"quota_blocker: UNBLOCKED org_id={org_id} (plan upgraded)")

    def is_blocked(self, org_id: str) -> bool:
        with self._lock:
            return org_id in self._blocked


# ── Singleton pattern ──────────────────────────────────────────────────────────

_handler: Optional[QuotaEnforcementHandler] = None
_blocker: Optional[QuotaBlocker] = None


def get_quota_enforcement_handler() -> QuotaEnforcementHandler:
    """Get or create singleton instance of QuotaEnforcementHandler."""
    global _handler
    if _handler is None:
        _handler = QuotaEnforcementHandler(settings.database_url)
    return _handler


def get_quota_blocker() -> QuotaBlocker:
    """Get or create singleton instance of QuotaBlocker."""
    global _blocker
    if _blocker is None:
        _blocker = QuotaBlocker()
    return _blocker
