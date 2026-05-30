"""Artifact transfer — upload/download via S3-compatible object storage.

Uses presigned URLs for direct runner<->storage transfer, avoiding
proxying large files through execution-core.
"""

from __future__ import annotations

import logging
from typing import BinaryIO

import boto3
from botocore.config import Config as BotoConfig

from app.config import settings
from app.domain import ArtifactKind, ArtifactMetadata

logger = logging.getLogger(__name__)

_s3_client = None


def _get_s3():
    """Lazy-initialised S3 client targeting the configured object storage."""
    global _s3_client
    if _s3_client is None:
        _s3_client = boto3.client(
            "s3",
            endpoint_url=settings.object_storage_endpoint,
            aws_access_key_id=settings.object_storage_access_key,
            aws_secret_access_key=settings.object_storage_secret_key,
            region_name=settings.object_storage_region,
            config=BotoConfig(
                signature_version="s3v4",
                retries={"max_attempts": 3, "mode": "adaptive"},
            ),
        )
    return _s3_client


def _storage_key(workspace_id: str, task_id: str, filename: str) -> str:
    """Generate a deterministic storage key for an artifact."""
    # Prevent path traversal in filename
    safe_name = filename.replace("/", "_").replace("..", "_")
    return f"workspaces/{workspace_id}/tasks/{task_id}/{safe_name}"


def generate_upload_url(
    workspace_id: str,
    task_id: str,
    filename: str,
    content_type: str = "application/octet-stream",
    expires_in: int = 3600,
) -> tuple[str, str]:
    """Generate a presigned URL for uploading an artifact.

    Returns (presigned_url, storage_key).
    """
    key = _storage_key(workspace_id, task_id, filename)
    s3 = _get_s3()
    url = s3.generate_presigned_url(
        "put_object",
        Params={
            "Bucket": settings.object_storage_bucket,
            "Key": key,
            "ContentType": content_type,
        },
        ExpiresIn=expires_in,
    )
    logger.info("upload_url_generated", extra={"key": key})
    return url, key


def generate_download_url(
    storage_key: str,
    expires_in: int = 3600,
) -> str:
    """Generate a presigned URL for downloading an artifact."""
    s3 = _get_s3()
    url = s3.generate_presigned_url(
        "get_object",
        Params={
            "Bucket": settings.object_storage_bucket,
            "Key": storage_key,
        },
        ExpiresIn=expires_in,
    )
    return url


def upload_artifact(
    workspace_id: str,
    task_id: str,
    filename: str,
    body: BinaryIO,
    content_type: str = "application/octet-stream",
) -> str:
    """Upload an artifact directly (for small files). Returns storage_key."""
    key = _storage_key(workspace_id, task_id, filename)
    s3 = _get_s3()
    s3.upload_fileobj(
        body,
        settings.object_storage_bucket,
        key,
        ExtraArgs={"ContentType": content_type},
    )
    logger.info("artifact_uploaded", extra={"key": key})
    return key


def delete_artifact(storage_key: str) -> None:
    """Delete an artifact from object storage."""
    s3 = _get_s3()
    s3.delete_object(Bucket=settings.object_storage_bucket, Key=storage_key)
    logger.info("artifact_deleted", extra={"key": storage_key})


def ensure_bucket() -> None:
    """Create the artifact bucket if it doesn't exist (idempotent)."""
    s3 = _get_s3()
    try:
        s3.head_bucket(Bucket=settings.object_storage_bucket)
    except Exception:
        try:
            s3.create_bucket(Bucket=settings.object_storage_bucket)
            logger.info("bucket_created", extra={"bucket": settings.object_storage_bucket})
        except Exception as e:
            logger.warning("bucket_create_failed", extra={"error": str(e)})
