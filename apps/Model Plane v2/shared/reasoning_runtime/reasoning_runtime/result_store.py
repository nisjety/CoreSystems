"""Result store — persists large completion payloads to MinIO/S3.

BUG FIX: Uses ``get_config().object_storage_*`` attributes (matching the
real config) instead of the old ``settings.minio_*`` names.
"""

from __future__ import annotations

import io
import json
import logging
import uuid

import boto3
from botocore.config import Config as BotoConfig

from reasoning_runtime.config import get_config

logger = logging.getLogger(__name__)

_client = None
_BUCKET: str | None = None


def init() -> None:
    """Initialise the S3/MinIO client from runtime config.

    Failure is non-fatal: if object storage is unavailable, large results
    will not be offloaded but the service will continue running normally.
    """
    global _client, _BUCKET
    cfg = get_config()
    _BUCKET = cfg.object_storage_bucket

    try:
        _client = boto3.client(
            "s3",
            endpoint_url=cfg.object_storage_endpoint,
            aws_access_key_id=cfg.object_storage_access_key,
            aws_secret_access_key=cfg.object_storage_secret_key,
            config=BotoConfig(signature_version="s3v4"),
            region_name=cfg.object_storage_region,
        )

        # Ensure bucket exists
        try:
            _client.head_bucket(Bucket=_BUCKET)
        except Exception:
            _client.create_bucket(Bucket=_BUCKET)
            logger.info("created_bucket bucket=%s", _BUCKET)

    except Exception as exc:
        logger.warning(
            "result_store_init_skipped: object storage unavailable (%s). "
            "Large results will not be offloaded.",
            exc,
        )
        _client = None


def should_store(content: str) -> bool:
    """Return True when ``content`` exceeds the configured byte threshold."""
    cfg = get_config()
    return len(content.encode("utf-8")) > cfg.result_store_threshold_bytes


def store(content: str, org_id: str, request_id: str) -> str:
    """Store content and return ``result_id``, or empty string if unavailable."""
    if _client is None:
        logger.debug("result_store_unavailable: skipping large-result offload")
        return ""

    result_id = f"{org_id}/{request_id}/{uuid.uuid4().hex}.json"
    body = json.dumps({"content": content}).encode("utf-8")

    _client.put_object(
        Bucket=_BUCKET,
        Key=result_id,
        Body=io.BytesIO(body),
        ContentLength=len(body),
        ContentType="application/json",
    )

    logger.info("stored_result id=%s bytes=%d", result_id, len(body))
    return result_id


def retrieve(result_id: str) -> str | None:
    """Fetch a stored result by its id."""
    if _client is None:
        raise RuntimeError("result_store not initialised — call init() first")

    try:
        resp = _client.get_object(Bucket=_BUCKET, Key=result_id)
        data = json.loads(resp["Body"].read())
        return data.get("content")
    except Exception:
        logger.warning("retrieve_failed result_id=%s", result_id)
        return None
