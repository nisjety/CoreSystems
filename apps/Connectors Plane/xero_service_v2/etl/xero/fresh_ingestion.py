"""Admission and pinned reads for a newly fetched, isolated Xero generation."""

import base64
import json
import logging
import os
import re
from dataclasses import dataclass
from datetime import datetime, timezone
from uuid import UUID

import google_crc32c

from etl.common.endpoint_config import ARRAY_KEYS
from etl.common.gcs_reader import GCSReader


logger = logging.getLogger(__name__)


def timestamp(value):
    try:
        parsed = datetime.fromisoformat(value)
    except (TypeError, ValueError) as exc:
        raise ValueError("invalid fresh source timestamp") from exc
    if parsed.tzinfo is None:
        raise ValueError("fresh source timestamp requires a timezone")
    return parsed.astimezone(timezone.utc)


def object_generation(value):
    if not isinstance(value, str) or not re.fullmatch(r"[1-9][0-9]*", value):
        raise ValueError("fresh input requires a positive pinned object generation")
    return int(value)


def has_pinned_data_identity(meta):
    """Return whether a sidecar has the immutable data identity required to read it."""
    try:
        object_generation(meta.get("x-data-generation"))
    except ValueError:
        return False
    checksum = meta.get("x-data-crc32c")
    return isinstance(checksum, str) and bool(re.fullmatch(r"[A-Za-z0-9+/]{6}==", checksum))


@dataclass(frozen=True)
class FreshConfig:
    generation: str
    started_at: datetime
    namespace: str = "v2"

    @classmethod
    def from_env(cls):
        generation = os.environ.get("XERO_INGESTION_GENERATION")
        start = os.environ.get("XERO_FRESH_START_AT")
        if generation is None and start is None:
            return None
        try:
            canonical = str(UUID(generation))
        except (TypeError, ValueError, AttributeError) as exc:
            raise ValueError("invalid fresh ingestion generation") from exc
        if canonical != generation:
            raise ValueError("fresh ingestion generation must be a canonical UUID")
        if not start:
            raise ValueError("fresh ingestion start timestamp is required")
        namespace = os.environ.get("XERO_INGESTION_NAMESPACE", "v2")
        if namespace not in {"v2", "main"}:
            raise ValueError("unsupported XERO_INGESTION_NAMESPACE")
        return cls(canonical, timestamp(start), namespace)


@dataclass(frozen=True)
class PreparedFreshObservation:
    """One admitted, checksum-verified raw source observation ready to publish."""

    layout: str
    target_project: str
    dataset: str
    batches: dict
    is_manifest: bool
    records: tuple[dict, ...] = ()


def admit(config, meta):
    source_system = "xero" if config.namespace == "main" else "xero_v2"
    if (meta.get("x-source-system") != source_system
            or meta.get("x-ingestion-generation") != config.generation):
        return False
    # Old sidecars in the bucket can share this path but lack pinned data
    # identity. They are permanently ineligible, so do not retry them.
    if not has_pinned_data_identity(meta):
        return False
    observed = timestamp(meta.get("x-synced-at"))
    started = timestamp(meta["x-run-started-at"]) if "x-run-started-at" in meta else observed
    return min(observed, started) >= config.started_at


class PinnedReader(GCSReader):
    def __init__(self, bucket, project, meta_path, meta_generation, storage_client=None):
        pinned = object_generation(meta_generation)
        super().__init__(bucket, project, storage_client)
        self.meta_path = meta_path
        self.meta_generation = pinned
        self.prefix = meta_path.split("raw/xero/", 1)[0] if "/raw/xero/" in meta_path else (
            meta_path.split("_manifests/xero/", 1)[0] if "/_manifests/xero/" in meta_path else ""
        )
        self._meta = None

    def _actual(self, path):
        return path if path.startswith(self.prefix) else self.prefix + path

    def _read_meta(self, path):
        if self._actual(path) != self.meta_path:
            raise ValueError("unexpected fresh sidecar path")
        if self._meta is None:
            blob = self._bucket.blob(self.meta_path, generation=self.meta_generation)
            self._meta = json.loads(blob.download_as_text())
        return dict(self._meta)

    def verify_data_identity(self, meta):
        generation = object_generation(meta.get("x-data-generation"))
        checksum = meta.get("x-data-crc32c")
        if not isinstance(checksum, str) or not re.fullmatch(r"[A-Za-z0-9+/]{6}==", checksum):
            raise ValueError("fresh source checksum is missing or invalid")
        blob = self._bucket.get_blob(self.meta_path.removesuffix(".meta.json"), generation=generation)
        if blob is None or blob.generation != generation or blob.crc32c != checksum:
            raise ValueError("fresh data object identity does not match its sidecar")
        self._meta = dict(meta)

    def _read_data(self, path):
        if self._actual(path) != self.meta_path.removesuffix(".meta.json"):
            raise ValueError("unexpected fresh data path")
        if self._meta is None:
            raise ValueError("fresh sidecar must be read before data")
        generation = object_generation(self._meta.get("x-data-generation"))
        body = self._bucket.blob(self._actual(path), generation=generation).download_as_bytes()
        checksum = base64.b64encode(google_crc32c.Checksum(body).digest()).decode("ascii")
        if checksum != self._meta.get("x-data-crc32c"):
            raise ValueError("fresh data checksum mismatch")
        return json.loads(body)

    def _extract_records(self, data, endpoint, tenant_id, synced_at, meta=None, source_object_path=None):
        if endpoint in ARRAY_KEYS:
            if not isinstance(data, dict) or not isinstance(data.get(ARRAY_KEYS[endpoint]), (list, dict)):
                raise ValueError("fresh response does not contain its declared collection")
        yield from super()._extract_records(
            data, endpoint, tenant_id, synced_at, meta, source_object_path,
        )


def prepare_fresh_observation(config, bucket, object_name, meta_generation, project, storage_client=None):
    """Verify and stage one admitted source object without writing BigQuery."""
    from etl.xero.fresh_staging import build_observation
    from etl.xero import accounting_get
    from etl.xero.extended import EXTENDED_ENDPOINTS, LIST_SNAPSHOT_ENDPOINTS, warehouse_target
    from etl.xero.run_history import manifest_rows, target_dataset
    from etl.xero.warehouse import ordinary_staging_dataset

    layout = os.environ.get("XERO_STAGING_LAYOUT", "legacy")
    if layout not in {"legacy", "endpoint", "warehouse"}:
        raise ValueError("unsupported XERO_STAGING_LAYOUT")

    namespace = "xero" if config.namespace == "main" else "v2"
    prefix = f"{namespace}/{config.generation}/"
    if not object_name.startswith(prefix):
        logger.info("Skipped non-baseline input; no staging writes")
        return None
    if not object_name.endswith(".meta.json"):
        raise ValueError("fresh input must be a metadata sidecar")
    logical = object_name.removeprefix(prefix)
    reader = PinnedReader(bucket, project, object_name, meta_generation, storage_client)
    meta = reader._read_meta(object_name)
    if not admit(config, meta):
        logger.info("Skipped input without active fresh-source provenance")
        return None
    is_manifest = logical.startswith("_manifests/xero/")
    records: tuple[dict, ...] = ()
    if is_manifest:
        reader.verify_data_identity(meta)
        manifest = reader._read_data(logical.removesuffix(".meta.json"))
        run, outcomes = manifest_rows(logical, manifest)
        if run["started_at"] < config.started_at:
            raise ValueError("fresh run started before baseline activation")
        target_project, dataset = target_dataset(bucket, [row["endpoint"] for row in outcomes], project)
        batches = build_observation(meta, [], bucket, object_name, str(meta_generation))
        batches = {**batches,
                   "v2_sync_runs": [{**run, "generation": config.generation}],
                   "v2_sync_run_entities": [{**row, "generation": config.generation} for row in outcomes]}
    else:
        parts = logical.split("/")
        if (len(parts) != 7 or parts[:2] != ["raw", "xero"]
                or not parts[2] or not parts[3] or not parts[4]
                or not parts[6].endswith(".meta.json")):
            raise ValueError("invalid fresh Xero path")
        endpoint = parts[4]
        if meta.get("x-tenant-id") != parts[2]:
            raise ValueError("fresh sidecar tenant does not match its raw object path")
        if meta.get("x-endpoint") != endpoint:
            raise ValueError("fresh sidecar endpoint does not match its raw object path")
        if meta.get("x-api-version") != parts[3]:
            raise ValueError("fresh sidecar API version does not match its raw object path")
        if endpoint.startswith("accounting_get_"):
            target_project, dataset = accounting_get.target_dataset(endpoint, bucket)
        elif endpoint in EXTENDED_ENDPOINTS or endpoint in LIST_SNAPSHOT_ENDPOINTS:
            target_project, dataset = warehouse_target(endpoint, bucket=bucket)
        else:
            if endpoint not in ARRAY_KEYS:
                raise ValueError("fresh endpoint lacks a declared raw response contract")
            forbidden = {os.environ.get("XERO_DOCUMENT_GCS_BUCKET"),
                         os.environ.get("XERO_SENSITIVE_GCS_BUCKET")}
            if bucket != os.environ.get("GCS_BUCKET") or bucket in forbidden:
                raise ValueError("ordinary fresh Xero payload requires its ordinary raw bucket")
            target_project, dataset = project, ordinary_staging_dataset()
        reader.verify_data_identity(meta)
        records = tuple(reader.iter_records_from_meta(logical))
        batches = build_observation(meta, list(records), bucket, object_name, str(meta_generation))
    return PreparedFreshObservation(
        layout=layout,
        target_project=target_project,
        dataset=dataset,
        batches=batches,
        is_manifest=is_manifest,
        records=records,
    )


def process_fresh(config, bucket, object_name, meta_generation, project):
    """All configured fresh deliveries are handled here, never by the legacy writer."""
    prepared = prepare_fresh_observation(config, bucket, object_name, meta_generation, project)
    if prepared is None:
        return
    publish_prepared_observation(prepared)


def publish_prepared_observation(prepared):
    """Commit one prepared observation using the target's immutable contract."""
    from etl.common.fresh_writer import EndpointBQWriter, FreshBQWriter, WarehouseRawBQWriter
    from etl.xero.warehouse import ordinary_staging_dataset

    if prepared.layout == "warehouse" and prepared.dataset == ordinary_staging_dataset():
        WarehouseRawBQWriter(prepared.target_project, prepared.dataset).publish_observation(prepared.batches)
    elif prepared.layout == "endpoint" and prepared.dataset == ordinary_staging_dataset():
        EndpointBQWriter(prepared.target_project, prepared.dataset).publish_observation(prepared.batches)
    else:
        FreshBQWriter(prepared.target_project, prepared.dataset).publish(prepared.batches)
    logger.info(
        "Fresh Xero observation committed: %d records, %d child rows",
        len(prepared.batches["v2_record_versions"]),
        len(prepared.batches["v2_child_versions"]),
    )


def publish_prepared_observations(prepared_observations):
    """Commit compatible ordinary raw observations as one fenced batch."""
    from etl.common.fresh_writer import WarehouseRawBQWriter
    from etl.xero.warehouse import ordinary_staging_dataset

    observations = tuple(prepared_observations)
    if not observations:
        return
    first = observations[0]
    if (first.is_manifest or first.layout != "warehouse"
            or first.dataset != ordinary_staging_dataset()):
        raise ValueError("only ordinary warehouse response observations can be batched")
    if any(
        observation.is_manifest
        or observation.layout != first.layout
        or observation.target_project != first.target_project
        or observation.dataset != first.dataset
        for observation in observations
    ):
        raise ValueError("fresh observation batch has incompatible targets")
    WarehouseRawBQWriter(first.target_project, first.dataset).publish_observations_many(
        [observation.batches for observation in observations]
    )
    logger.info("Fresh Xero observation batch committed: %d source objects", len(observations))
