"""AtroposExporter — nightly JSONL export of trajectory data to MinIO.

Exports one file per org per day:
    s3://velion-artifacts/rl-training/{org_id}/{date}.jsonl.gz

Each line is a JSON object compatible with the Atropos RL training format:
{
  "id": str,
  "org_id": str,
  "task_pattern": str,
  "goal": str,
  "planned_actions": [...],
  "executed_actions": [...],
  "outcome": "success" | "partial" | "failure",
  "duration_sec": float | null,
  "skills_used": [...],
  "cost_usd": float,
  "model": str | null,
  "created_at": str  (ISO-8601)
}

Called by the cron scheduler at midnight UTC.
"""

from __future__ import annotations

import asyncio
import gzip
import json
import logging
from datetime import date, datetime, timezone
from io import BytesIO
from typing import Any

from app.database import get_pool

logger = logging.getLogger(__name__)

_BUCKET = "velion-artifacts"
_PREFIX = "rl-training"
# Rows to fetch per org per export (safety cap)
_MAX_ROWS = 50_000


def _make_s3_client() -> Any | None:
    """Return a boto3 S3 client pointed at MinIO, or None if misconfigured."""
    try:
        import boto3  # type: ignore[import-untyped]
        from app.config import settings

        endpoint = getattr(settings, "object_storage_endpoint", "") or ""
        access_key = getattr(settings, "object_storage_access_key", "") or ""
        secret_key = getattr(settings, "object_storage_secret_key", "") or ""

        if not endpoint:
            logger.debug("atropos_exporter_disabled: OBJECT_STORAGE_ENDPOINT not set")
            return None

        return boto3.client(
            "s3",
            endpoint_url=endpoint,
            aws_access_key_id=access_key,
            aws_secret_access_key=secret_key,
        )
    except Exception as exc:
        logger.warning("atropos_s3_client_init_failed", extra={"error": str(exc)})
        return None


def _ensure_bucket(s3: Any) -> None:
    """Create the bucket if it doesn't exist."""
    try:
        s3.head_bucket(Bucket=_BUCKET)
    except Exception:
        try:
            s3.create_bucket(Bucket=_BUCKET)
            logger.info("atropos_bucket_created", extra={"bucket": _BUCKET})
        except Exception as exc:
            logger.warning("atropos_bucket_create_failed", extra={"error": str(exc)})


async def export_org(org_id: str, export_date: date | None = None) -> bool:
    """Export one day's trajectories for ``org_id`` to MinIO.

    Args:
        org_id: Target org UUID string.
        export_date: The UTC date to export (default: today).

    Returns True if the upload succeeded, False otherwise.
    """
    s3 = _make_s3_client()
    if s3 is None:
        return False

    if export_date is None:
        export_date = datetime.now(timezone.utc).date()

    date_str = export_date.isoformat()
    key = f"{_PREFIX}/{org_id}/{date_str}.jsonl.gz"

    try:
        pool = await get_pool()
        async with pool.acquire() as conn:
            rows = await conn.fetch(
                """
                SELECT
                    id, org_id, session_id, task_goal, task_pattern,
                    model, tokens_in, tokens_out, cost_usd,
                    planned_actions, executed_actions,
                    outcome, duration_sec, skills_used, created_at
                FROM agent_trajectories
                WHERE org_id = $1
                  AND created_at::date = $2
                LIMIT $3
                """,
                org_id,
                date_str,
                _MAX_ROWS,
            )
    except Exception as exc:
        logger.warning(
            "atropos_fetch_failed", extra={"org_id": org_id, "error": str(exc)}
        )
        return False

    if not rows:
        logger.debug("atropos_no_rows", extra={"org_id": org_id, "date": date_str})
        return True  # Nothing to export, not an error

    # Build JSONL in memory
    buf = BytesIO()
    with gzip.GzipFile(fileobj=buf, mode="wb") as gz:
        for row in rows:
            record = {
                "id": str(row["id"]),
                "org_id": str(row["org_id"]),
                "session_id": row["session_id"],
                "task_pattern": row["task_pattern"],
                "goal": row["task_goal"],
                "planned_actions": row["planned_actions"] or [],
                "executed_actions": row["executed_actions"] or [],
                "outcome": row["outcome"],
                "duration_sec": float(row["duration_sec"]) if row["duration_sec"] else None,
                "skills_used": list(row["skills_used"] or []),
                "cost_usd": float(row["cost_usd"] or 0),
                "tokens_in": row["tokens_in"],
                "tokens_out": row["tokens_out"],
                "model": row["model"],
                "created_at": row["created_at"].isoformat() if row["created_at"] else None,
            }
            gz.write(json.dumps(record).encode() + b"\n")

    payload = buf.getvalue()

    try:
        _ensure_bucket(s3)
        s3.put_object(
            Bucket=_BUCKET,
            Key=key,
            Body=payload,
            ContentEncoding="gzip",
            ContentType="application/x-ndjson",
        )
        logger.info(
            "atropos_export_complete",
            extra={
                "org_id": org_id,
                "date": date_str,
                "rows": len(rows),
                "bytes": len(payload),
                "key": key,
            },
        )
        return True
    except Exception as exc:
        logger.warning(
            "atropos_upload_failed",
            extra={"org_id": org_id, "key": key, "error": str(exc)},
        )
        return False


async def export_all_active_orgs(export_date: date | None = None) -> None:
    """Run export for every org with trajectories on ``export_date``.

    Intended to be called by the midnight cron task.
    """
    if export_date is None:
        export_date = datetime.now(timezone.utc).date()

    date_str = export_date.isoformat()
    try:
        pool = await get_pool()
        async with pool.acquire() as conn:
            rows = await conn.fetch(
                """
                SELECT DISTINCT org_id
                FROM agent_trajectories
                WHERE created_at::date = $1
                """,
                date_str,
            )
    except Exception as exc:
        logger.warning("atropos_list_orgs_failed", extra={"error": str(exc)})
        return

    org_ids = [str(r["org_id"]) for r in rows]
    if not org_ids:
        logger.debug("atropos_no_orgs_today", extra={"date": date_str})
        return

    results = await asyncio.gather(
        *[export_org(oid, export_date) for oid in org_ids],
        return_exceptions=True,
    )
    failures = sum(1 for r in results if not r or isinstance(r, Exception))
    logger.info(
        "atropos_nightly_complete",
        extra={"total": len(org_ids), "failures": failures, "date": date_str},
    )
