"""
GCS reader — production replacement for bq_reader.py.

Reads Xero API data from the GCS bucket written by the sync service.
Each API call produces two files:

  Data file:  raw/{vendor}/{tenant_id}/2.0/{endpoint}/{date}/{ts}_{run}_{page}.json
  Meta file:  raw/{vendor}/{tenant_id}/2.0/{endpoint}/{date}/{ts}_{run}_{page}.json.meta.json

The meta file carries context (tenant_id, endpoint, synced_at, run_id).
The data file is the raw Xero API response — a wrapper object containing a
PascalCase-keyed array of records (e.g. "Accounts": [...], "Invoices": [...]).

Usage (Cloud Function — single meta file event):
    from etl.common.gcs_reader import GCSReader

    reader = GCSReader(bucket="prj-dw-dev-raw")
    for record in reader.iter_records_from_meta("raw/xero/.../accounts/2026-06-18/file.json.meta.json"):
        # record["tenant_id"], record["record_id"], record["payload"], record["synced_at"]
        ...

Usage (batch / replay — iterate all files for an endpoint):
    for record in reader.iter_records("xero", "accounts", date="2026-06-18"):
        ...

Interface contract:
    Each yielded dict has the same shape as BQReader.iter_records() so all
    entity parsers work without modification:
        tenant_id     str
        record_id     str   (entity-specific ID, e.g. AccountID)
        payload       dict  (the individual record from the array)
        first_seen_at datetime | None
        last_seen_at  datetime  (= synced_at from meta file)
        synced_at     datetime
"""

import json
import logging
import re
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
from typing import Iterator

from google.cloud import storage

from etl.common.endpoint_config import ARRAY_KEYS, RECORD_ID_FIELDS
from etl.xero import accounting_get
from etl.xero.attachment_metadata import (
    extract_records as extract_attachment_records,
    validate_parent_context,
)
from etl.xero.extended import EXTENDED_ENDPOINTS, LIST_SNAPSHOT_ENDPOINTS, extract_records

logger = logging.getLogger(__name__)

# Matches: raw/{vendor}/{tenant_id}/{api_version}/{endpoint}/{date}/{filename}
_PATH_RE = re.compile(
    r"raw/(?P<vendor>[^/]+)"
    r"/(?P<tenant_id>[^/]+)"
    r"/(?P<version>[^/]+)"    # 1.0, 2.0, or identity
    r"/(?P<endpoint>[^/]+)"
    r"/(?P<date>[^/]+)"
    r"/(?P<filename>[^/]+)"
)


def _api_version(endpoint: str) -> str:
    if endpoint in EXTENDED_ENDPOINTS or endpoint in LIST_SNAPSHOT_ENDPOINTS:
        family = endpoint.split("_", 1)[0]
        if family == "identity":
            return "identity"
        if family in {"assets", "bankfeeds", "finance"}:
            return "1.0"
    return "2.0"


class GCSReader:
    def __init__(self, bucket: str, project: str = "prj-dw-dev", storage_client=None):
        self.bucket_name = bucket
        self.project = project
        self._client = storage_client or storage.Client(project=project)
        self._bucket = self._client.bucket(bucket)

    # ------------------------------------------------------------------
    # Primary interface: process a single meta file (Cloud Function mode)
    # ------------------------------------------------------------------

    def iter_records_from_meta(self, meta_path: str) -> Iterator[dict]:
        """
        Given the GCS path of a .meta.json file, read both files and yield
        one record dict per item. Attachment lists yield one complete parent
        snapshot, including when the Attachments array is empty.

        Args:
            meta_path: GCS object path of the .meta.json file, e.g.
                       "raw/xero/19b25bd5.../2.0/accounts/2026-06-18/file.json.meta.json"
        """
        meta = self._read_meta(meta_path)
        if str(meta.get("x-http-status")) == "304":
            return
        # Fresh generations prepend `xero/<generation>/` to the durable raw
        # key. Keep the original path for lineage while routing uses the raw
        # Xero segment embedded within it.
        match = _PATH_RE.search(meta_path)
        if not match:
            raise ValueError(f"invalid Xero raw object path: {meta_path}")
        if meta.get("x-tenant-id") != match.group("tenant_id"):
            raise ValueError(f"sidecar tenant does not match object path: {meta_path}")
        if meta.get("x-endpoint") != match.group("endpoint"):
            raise ValueError(f"sidecar endpoint does not match object path: {meta_path}")
        if meta.get("x-api-version") != match.group("version"):
            raise ValueError(f"sidecar API version does not match object path: {meta_path}")
        endpoint = match.group("endpoint")
        data_path = meta_path.removesuffix(".meta.json")
        if endpoint.startswith("accounting_get_"):
            if match.group("vendor") != "xero" or match.group("version") != "2.0":
                raise ValueError("Accounting GET requires a Xero Accounting 2.0 path")
            operation_id, binary = accounting_get.validate_sidecar(endpoint, meta)
            if str(meta.get("x-http-status")) != "200":
                raise ValueError("Accounting GET requires an HTTP 200 response")
            content_type = meta["x-response-content-type"].split(";", 1)[0].strip().lower()
            is_pdf_route = operation_id.endswith("AsPdf")
            if binary and (
                (is_pdf_route and content_type != "application/pdf")
                or content_type in {"application/json", "text/html"}
                or "json" in content_type
                or "/" not in content_type
            ):
                raise ValueError("Accounting GET document has an unexpected content type")
            if not binary and content_type != "application/json":
                raise ValueError("Accounting GET JSON has an unexpected content type")
            if data_path.rsplit(".", 1)[-1] != (
                "pdf" if is_pdf_route else "bin" if binary else "json"
            ):
                raise ValueError("Accounting GET object extension does not match content type")
            synced_at = _parse_synced_at(meta["x-synced-at"], strict=True)
            yield {
                "tenant_id": meta["x-tenant-id"],
                "record_id": meta["x-run-id"],
                "payload": None if binary else self._read_data(data_path),
                "synced_at": synced_at,
                "meta": meta,
                "source_object_path": data_path,
                "source_bucket": getattr(self, "bucket_name", ""),
            }
            return
        if match.group("endpoint") == "attachment_metadata":
            if match.group("vendor") != "xero" or match.group("version") != "2.0":
                raise ValueError("attachment metadata requires a Xero Accounting 2.0 path")
            validate_parent_context(meta)
            if str(meta.get("x-http-status")) != "200":
                raise ValueError("attachment metadata requires a successful HTTP 200 list response")
        data = self._read_data(data_path)

        endpoint  = meta["x-endpoint"]
        tenant_id = meta["x-tenant-id"]
        synced_at = _parse_synced_at(
            meta["x-synced-at"],
            strict=endpoint == "attachment_metadata" or endpoint in LIST_SNAPSHOT_ENDPOINTS,
        )

        yield from self._extract_records(
            data, endpoint, tenant_id, synced_at, meta, meta_path,
        )

    # ------------------------------------------------------------------
    # Batch / replay interface: iterate all files for an endpoint + date
    # ------------------------------------------------------------------

    def iter_records(
        self,
        vendor: str,
        endpoint: str,
        tenant_id: str | None = None,
        date: str | None = None,
        max_workers: int = 16,
    ) -> Iterator[dict]:
        """
        Iterate all records for a vendor/endpoint, optionally filtered by
        tenant_id and/or date (YYYY-MM-DD). Reads from .meta.json files only.

        Useful for batch replays and backfills from GCS.

        The endpoint segment sits after {tenant_id}/{version} in the path, so a
        clean list prefix is only possible when tenant_id is known. When it is
        not, we list the whole vendor tree and match endpoint (and optionally
        date) via the parsed path.

        Concurrency (2026-07-24): matched files are fetched (meta + data, two
        GETs each) on a thread pool. A single tenant/endpoint history can be
        thousands of files with years of accumulated incremental syncs — fetching
        them one at a time serially was the actual bottleneck behind an overnight
        backfill stalling for hours on just a handful of large tenants, even
        after parallelizing across (tenant, endpoint) jobs one level up in
        backfill_gcs.py. Listing itself stays a single sequential pass first
        (cheap — no downloads); only the per-file fetch+parse is parallelized.
        """
        if tenant_id:
            # Tightest possible prefix for this API family's version.
            prefix = f"raw/{vendor}/{tenant_id}/{_api_version(endpoint)}/{endpoint}/"
            if date:
                prefix += f"{date}/"
        else:
            # Cannot pin endpoint in the prefix without tenant_id — list the
            # vendor tree and filter each blob by parsed path.
            prefix = f"raw/{vendor}/"

        meta_paths = []
        for blob in self._client.list_blobs(self.bucket_name, prefix=prefix):
            if not blob.name.endswith(".meta.json"):
                continue
            m = _PATH_RE.search(blob.name)
            if not m:
                continue
            if m.group("endpoint") != endpoint:
                continue
            if date and m.group("date") != date:
                continue
            meta_paths.append(blob.name)

        if not meta_paths:
            return

        with ThreadPoolExecutor(max_workers=max_workers) as pool:
            futures = [pool.submit(self._fetch_records, p) for p in meta_paths]
            for future in as_completed(futures):
                yield from future.result()

    def _fetch_records(self, meta_path: str) -> list[dict]:
        """
        Fetch + parse one meta/data file pair. Returns a materialised list
        (not a generator) so results cross thread boundaries cleanly via
        Future.result().
        """
        try:
            return list(self.iter_records_from_meta(meta_path))
        except Exception as e:
            logger.error("Failed processing %s: %s", meta_path, e)
            raise

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _read_meta(self, path: str) -> dict:
        blob = self._bucket.blob(path)
        return json.loads(blob.download_as_text())

    def _read_data(self, path: str) -> dict:
        blob = self._bucket.blob(path)
        return json.loads(blob.download_as_text())

    def _extract_records(
        self,
        data: dict | list,
        endpoint: str,
        tenant_id: str,
        synced_at: datetime,
        meta: dict | None = None,
        source_object_path: str | None = None,
    ) -> Iterator[dict]:
        if endpoint in LIST_SNAPSHOT_ENDPOINTS:
            yield {
                "tenant_id": tenant_id,
                "payload": data,
                "synced_at": synced_at,
                "meta": meta,
                "source_object_path": source_object_path,
            }
            return
        if endpoint == "attachment_metadata":
            _, parent_id = validate_parent_context(meta or {})
            extract_attachment_records(data)
            yield {
                "tenant_id": tenant_id,
                "record_id": parent_id,
                "payload": data,
                "first_seen_at": synced_at,
                "last_seen_at": synced_at,
                "synced_at": synced_at,
                "meta": meta,
                "source_object_path": source_object_path,
            }
            return
        if endpoint in EXTENDED_ENDPOINTS:
            for record_id, item in extract_records(data, endpoint, meta or {}):
                yield {
                    "tenant_id": tenant_id,
                    "record_id": record_id,
                    "payload": item,
                    "first_seen_at": synced_at,
                    "last_seen_at": synced_at,
                    "synced_at": synced_at,
                    "meta": meta or {},
                }
            return
        array_key = ARRAY_KEYS.get(endpoint)
        if not array_key:
            logger.warning("No array key configured for endpoint: %s", endpoint)
            return

        id_field = RECORD_ID_FIELDS.get(endpoint)
        if not id_field:
            logger.warning("No record ID field configured for endpoint: %s", endpoint)
            return

        records = data.get(array_key)
        if not records:
            logger.debug("No records found under key '%s' for endpoint %s", array_key, endpoint)
            return

        if isinstance(records, dict):
            records = [records]

        for item in records:
            record_id = str(item.get(id_field, ""))
            if not record_id:
                logger.warning("Record missing %s in endpoint %s — skipping", id_field, endpoint)
                continue

            yield {
                "tenant_id":    tenant_id,
                "record_id":    record_id,
                "payload":      item,
                "first_seen_at": synced_at,  # best approximation; not tracked per-record in GCS
                "last_seen_at":  synced_at,
                "synced_at":     synced_at,
                # Full sync-context sidecar (x-run-id, x-report-from/to, etc.).
                # Additive/optional — existing parsers ignore it. Needed by
                # parsers whose identity/params live only in the meta file,
                # not the payload (e.g. report snapshots — see etl/xero/reports.py).
                "meta":          meta or {},
            }


def _parse_synced_at(value: str, strict: bool = False) -> datetime:
    """Parse x-synced-at ISO timestamp (with or without nanoseconds) to UTC datetime."""
    try:
        # Truncate sub-microsecond precision Python can't handle
        # e.g. "2026-06-18T13:40:56.194109096+00:00" -> "2026-06-18T13:40:56.194109+00:00"
        truncated = re.sub(r"(\.\d{6})\d+([\+\-Z])", r"\1\2", value)
        parsed = datetime.fromisoformat(truncated)
        if strict and parsed.tzinfo is None:
            raise ValueError("attachment metadata requires a timezone-aware synced_at")
        return parsed.astimezone(timezone.utc)
    except Exception:
        if strict:
            raise ValueError("attachment metadata has invalid synced_at") from None
        return datetime.now(tz=timezone.utc)
