"""
Cloud Function — GCS meta file trigger.

Triggered when a .meta.json file is written to gs://prj-dw-dev-raw. Because the
raw bucket is europe-north2 (where Cloud Functions gen2 is unavailable), the
bucket publishes OBJECT_FINALIZE notifications to a Pub/Sub topic and this
function (running in europe-north1) is triggered off that topic. The handler
also accepts a direct Eventarc Cloud Storage event, so it still works if moved
to an in-region GCS trigger. Data file writes are ignored (filtered by the
.meta.json suffix check).

Path format:
    raw/{vendor}/{tenant_id}/2.0/{endpoint}/{date}/{ts}_{run}_{page}.json.meta.json

Flow:
    .meta.json written to GCS
    → Cloud Function fires
    → read meta file (tenant_id, endpoint, synced_at)
    → derive data file path (strip .meta.json)
    → GCSReader yields one record per item in the records array
    → route to correct entity parser
    → parser.run() MERGEs all rows into staging_{vendor}

_BatchReader wraps a pre-fetched list of records so every parser's run()
function works without modification — parsers do not know whether they
are reading from BQ bronze, GCS, or a test fixture.
"""

import logging
import os
from dataclasses import dataclass

import functions_framework

from etl.common.batched_delivery import BoundedBatcher
from etl.common.gcs_reader import GCSReader
from etl.common.bq_writer import BQWriter

# Xero parsers — one per endpoint currently present in the GCS bucket.
# Parsers are added only when their endpoint actually appears in the bucket;
# a new unparsed endpoint triggers a drift warning (see process_gcs_upload).
import etl.xero.accounts            as _accounts
import etl.xero.attachment_metadata as _attachment_metadata
import etl.xero.bank_transactions   as _bank_transactions
import etl.xero.bank_transfers      as _bank_transfers
import etl.xero.batch_payments      as _batch_payments
import etl.xero.branding_themes     as _branding_themes
import etl.xero.budgets             as _budgets
import etl.xero.contact_groups      as _contact_groups
import etl.xero.contacts            as _contacts
import etl.xero.credit_notes        as _credit_notes
import etl.xero.currencies          as _currencies
import etl.xero.invoices            as _invoices
import etl.xero.items               as _items
import etl.xero.journals            as _journals
import etl.xero.linked_transactions as _linked_transactions
import etl.xero.manual_journals     as _manual_journals
import etl.xero.organisations       as _organisations
import etl.xero.overpayments        as _overpayments
import etl.xero.payments            as _payments
import etl.xero.purchase_orders     as _purchase_orders
import etl.xero.quotes              as _quotes
import etl.xero.repeating_invoices  as _repeating_invoices
import etl.xero.reports             as _reports
import etl.xero.tax_rates           as _tax_rates
import etl.xero.tracking_categories as _tracking_categories
import etl.xero.users               as _users
import etl.xero.extended            as _extended
import etl.xero.extended_membership as _extended_membership
import etl.xero.accounting_get     as _accounting_get
from etl.xero.extended import EXTENDED_ENDPOINTS, LIST_SNAPSHOT_ENDPOINTS, warehouse_target
from etl.xero.run_history import manifest_rows, target_dataset, write_manifest_rows
from etl.xero.warehouse import ordinary_staging_dataset
from etl.xero.fresh_ingestion import (
    FreshConfig,
    prepare_fresh_observation,
    process_fresh,
    publish_prepared_observation,
    publish_prepared_observations,
)

# Emit INFO to Cloud Logging. Without this the root logger defaults to WARNING
# and the per-file "Processing/Processed" lines (and drift-detection info) are
# silently dropped — leaving the function invisible in logs even when it works.
logging.basicConfig(level=logging.INFO)
logging.getLogger().setLevel(logging.INFO)
logger = logging.getLogger(__name__)

GCS_BUCKET = os.environ.get("GCS_BUCKET", "prj-dw-dev-raw")
PROJECT     = os.environ.get("GCP_PROJECT", "prj-dw-dev")
FRESH_BATCH_SIZE = 32
FRESH_BATCH_WINDOW_SECONDS = 0.25

if (os.environ.get("FUNCTION_TARGET") == "process_fresh_gcs_upload"
        and FreshConfig.from_env() is None):
    raise RuntimeError("fresh-only loader requires baseline configuration at startup")

XERO_PARSERS: dict = {
    "accounts":             _accounts,
    "attachment_metadata":  _attachment_metadata,
    "bank_transactions":    _bank_transactions,
    "bank_transfers":       _bank_transfers,
    "batch_payments":       _batch_payments,
    "branding_themes":      _branding_themes,
    "budgets":              _budgets,
    "contact_groups":       _contact_groups,
    "contacts":             _contacts,
    "credit_notes":         _credit_notes,
    "currencies":           _currencies,
    "invoices":             _invoices,
    "items":                _items,
    "journals":             _journals,
    "linked_transactions":  _linked_transactions,
    "manual_journals":      _manual_journals,
    "organisations":        _organisations,
    "overpayments":         _overpayments,
    "payments":             _payments,
    "purchase_orders":      _purchase_orders,
    "quotes":               _quotes,
    "repeating_invoices":    _repeating_invoices,
    "report_balance_sheet":      _reports,
    "report_aged_payables_by_contact": _reports,
    "report_aged_receivables_by_contact": _reports,
    "report_bank_summary":       _reports,
    "report_budget_summary":     _reports,
    "report_executive_summary": _reports,
    "report_profit_and_loss":    _reports,
    "report_trial_balance":      _reports,
    "tax_rates":            _tax_rates,
    "tracking_categories":  _tracking_categories,
    "users":                _users,
}
XERO_PARSERS.update({endpoint: _extended for endpoint in EXTENDED_ENDPOINTS})
XERO_PARSERS.update({endpoint: _extended_membership for endpoint in LIST_SNAPSHOT_ENDPOINTS})

VENDOR_PARSERS: dict = {
    "xero": XERO_PARSERS,
    # "visma": VISMA_PARSERS,  # added when Visma parsers are built
}

# Bucket endpoints we knowingly do not parse (kept out of drift warnings).
KNOWN_UNPARSED = {
    "bills": "subset of invoices (ACCPAY) — covered by staging_xero.invoices",
}


@dataclass(frozen=True)
class _TypedProjection:
    project: str
    dataset: str
    vendor: str
    endpoint: str
    records: tuple[dict, ...]
    source_objects: tuple[str, ...]

    @property
    def batch_key(self):
        return self.project, self.dataset, self.vendor, self.endpoint


_raw_batcher = BoundedBatcher(max_items=FRESH_BATCH_SIZE, max_wait_seconds=FRESH_BATCH_WINDOW_SECONDS)
_typed_batcher = BoundedBatcher(max_items=FRESH_BATCH_SIZE, max_wait_seconds=FRESH_BATCH_WINDOW_SECONDS)


class _BatchReader:
    """
    Wraps a pre-fetched list of records so parser run() functions work
    without modification. The table argument to iter_records() is ignored —
    records are already filtered by entity type by the Cloud Function.
    """
    def __init__(self, records: list[dict], project: str):
        self.project = project
        self._records = records

    def iter_records(self, table: str, **kwargs):  # noqa: ARG002
        yield from self._records


def _object_event(cloud_event):
    """Return one finalized object event without reading the source object."""
    data = cloud_event.data
    if isinstance(data, dict) and "message" in data:
        attrs = (data.get("message") or {}).get("attributes") or {}
        if attrs.get("eventType") not in (None, "OBJECT_FINALIZE"):
            return None
        return attrs.get("bucketId"), attrs.get("objectId"), attrs.get("objectGeneration")
    if not isinstance(data, dict):
        raise ValueError("Xero loader event data must be an object")
    return data.get("bucket"), data.get("name"), data.get("generation")


def _fresh_prefix(fresh):
    namespace = "xero" if fresh.namespace == "main" else "v2"
    return f"{namespace}/{fresh.generation}/"


def current_projection_from_records(bucket_name, object_name, routing_object_name, records):
    """Build one typed projection from records read through a pinned sidecar."""
    parts = routing_object_name.split("/")
    vendor = parts[1] if len(parts) > 1 else None
    endpoint = parts[4] if len(parts) > 4 else None
    if not vendor or not endpoint:
        raise ValueError(f"invalid Xero object path: {object_name}")
    if endpoint.startswith("accounting_get_"):
        if vendor != "xero":
            raise ValueError("Accounting GET requires the xero vendor")
        project, staging_dataset = _accounting_get.target_dataset(endpoint, bucket_name)
    elif endpoint in EXTENDED_ENDPOINTS or endpoint in LIST_SNAPSHOT_ENDPOINTS:
        project, staging_dataset = warehouse_target(endpoint, bucket=bucket_name)
    elif vendor == "xero":
        project, staging_dataset = PROJECT, ordinary_staging_dataset()
    else:
        project, staging_dataset = PROJECT, f"staging_{vendor}"

    if not records:
        logger.info("No records extracted from %s — skipping", object_name)
        return None

    vendor_map = VENDOR_PARSERS.get(vendor)
    if not vendor_map:
        logger.warning("No parsers registered for vendor: %s", vendor)
        return None
    parser = _accounting_get if endpoint.startswith("accounting_get_") else vendor_map.get(endpoint)
    if parser is None:
        if endpoint in KNOWN_UNPARSED:
            logger.info("Skipping known-unparsed endpoint %s/%s (%s)", vendor, endpoint,
                        KNOWN_UNPARSED[endpoint])
        else:
            logger.warning("NEW ENDPOINT DETECTED: %s/%s has no parser (file: %s)",
                           vendor, endpoint, object_name)
        return None
    return _TypedProjection(
        project=project,
        dataset=staging_dataset,
        vendor=vendor,
        endpoint=endpoint,
        records=records,
        source_objects=(object_name,),
    )


def _prepare_current_projection(bucket_name, object_name, routing_object_name):
    """Legacy helper for non-fresh callers without an admitted pinned reader."""
    parts = routing_object_name.split("/")
    vendor = parts[1] if len(parts) > 1 else None
    endpoint = parts[4] if len(parts) > 4 else None
    if not vendor or not endpoint:
        raise ValueError(f"invalid Xero object path: {object_name}")
    if endpoint.startswith("accounting_get_"):
        project, _ = _accounting_get.target_dataset(endpoint, bucket_name)
    elif endpoint in EXTENDED_ENDPOINTS or endpoint in LIST_SNAPSHOT_ENDPOINTS:
        project, _ = warehouse_target(endpoint, bucket=bucket_name)
    else:
        project = PROJECT
    records = tuple(GCSReader(bucket=bucket_name, project=project).iter_records_from_meta(object_name))
    return current_projection_from_records(bucket_name, object_name, routing_object_name, records)


def _publish_current_projections(projections):
    """Batch compatible source records into one typed staging writer call."""
    pending = tuple(projections)
    if not pending:
        return
    first = pending[0]
    if any(projection.batch_key != first.batch_key for projection in pending):
        raise ValueError("typed projection batch has incompatible targets")
    parser = _accounting_get if first.endpoint.startswith("accounting_get_") else \
        VENDOR_PARSERS[first.vendor][first.endpoint]
    records = [record for projection in pending for record in projection.records]
    writer = BQWriter(project=first.project, dataset=first.dataset)
    reader = _BatchReader(records, project=first.project)
    if first.endpoint.startswith("accounting_get_") or first.endpoint in EXTENDED_ENDPOINTS:
        result = parser.run(reader, writer, endpoint=first.endpoint)
    else:
        result = parser.run(reader, writer)
    logger.info("Projected %d source objects / %d records for %s → %s", len(pending),
                len(records), first.endpoint, result)


def _publish_prepared_raw(prepared):
    """Commit a single manifest or a bounded group of ordinary raw responses."""
    observations = tuple(prepared)
    if not observations:
        return
    if len(observations) == 1 and observations[0].is_manifest:
        publish_prepared_observation(observations[0])
        return
    publish_prepared_observations(observations)


@functions_framework.cloud_event
def process_fresh_gcs_upload(cloud_event):
    """Commit raw history, then update the compatible current typed projection."""
    fresh = FreshConfig.from_env()
    if fresh is None:
        raise ValueError("fresh-only loader requires its generation and activation timestamp")
    event = _object_event(cloud_event)
    if event is None:
        return
    bucket_name, object_name, meta_generation = event
    if not object_name or not object_name.endswith(".meta.json"):
        return
    document_bucket = os.environ.get("XERO_DOCUMENT_GCS_BUCKET")
    if not bucket_name or (bucket_name != GCS_BUCKET and bucket_name != document_bucket):
        raise ValueError(f"unexpected GCS bucket for Xero loader: {bucket_name}")
    if not object_name.startswith(_fresh_prefix(fresh)):
        logger.info("Skipped object outside active Xero ingestion generation")
        return

    prepared = prepare_fresh_observation(
        fresh, bucket_name, object_name, str(meta_generation or ""), PROJECT,
    )
    if prepared is None:
        return
    raw_key = (prepared.layout, prepared.target_project, prepared.dataset, prepared.is_manifest)
    if prepared.is_manifest:
        _publish_prepared_raw((prepared,))
        return
    _raw_batcher.submit(raw_key, prepared, _publish_prepared_raw)

    logical_object_name = object_name.removeprefix(_fresh_prefix(fresh))
    projection = current_projection_from_records(
        bucket_name, object_name, logical_object_name, prepared.records,
    )
    # Catalogued direct GETs are immutable raw evidence, not a current-entity
    # feed. Their target table cannot enforce uniqueness across Eventarc retry
    # workers, whereas Bronze/Silver reconcile directly from raw history.
    if projection is not None and projection.endpoint.startswith("accounting_get_"):
        logger.info("Direct Accounting GET retained in raw history: %s", object_name)
        return
    if projection is not None:
        _typed_batcher.submit(projection.batch_key, projection, _publish_current_projections)


@functions_framework.cloud_event
def process_gcs_upload(cloud_event, _fresh_already_committed=False):
    """Entry point — fires on every GCS object finalise event."""
    data = cloud_event.data

    # Support both event transports:
    #  - Pub/Sub notification envelope (GCS OBJECT_FINALIZE → Pub/Sub topic → this fn).
    #    Used because the raw bucket is europe-north2 and Cloud Functions gen2 is not
    #    offered there; the fn runs in europe-north1 and receives events via Pub/Sub.
    #    GCS carries bucket/object in the Pub/Sub message attributes.
    #  - Direct Eventarc Cloud Storage event (bucket/name at the top level), kept so the
    #    fn still works if it is ever moved to an in-region Eventarc GCS trigger.
    if isinstance(data, dict) and "message" in data:
        attrs       = (data.get("message") or {}).get("attributes") or {}
        bucket_name = attrs.get("bucketId")
        object_name = attrs.get("objectId")
        event_type  = attrs.get("eventType")
        meta_generation = attrs.get("objectGeneration")
        if event_type and event_type != "OBJECT_FINALIZE":
            return
    else:
        bucket_name = data.get("bucket")
        object_name = data.get("name")
        meta_generation = data.get("generation")

    # Only process meta files; silently ignore data file triggers (and non-object events)
    if not object_name or not object_name.endswith(".meta.json"):
        return
    document_bucket = os.environ.get("XERO_DOCUMENT_GCS_BUCKET")
    if not bucket_name or (bucket_name != GCS_BUCKET and bucket_name != document_bucket):
        raise ValueError(f"unexpected GCS bucket for Xero loader: {bucket_name}")

    logger.info("Processing meta file: gs://%s/%s", bucket_name, object_name)

    fresh = FreshConfig.from_env()
    if fresh is not None and not _fresh_already_committed:
        process_fresh(fresh, bucket_name, object_name, str(meta_generation or ""), PROJECT)
        return

    routing_object_name = object_name
    if _fresh_already_committed:
        if fresh is None or not object_name.startswith(_fresh_prefix(fresh)):
            raise ValueError("typed Xero projection requires an active fresh generation")
        routing_object_name = object_name.removeprefix(_fresh_prefix(fresh))

    if routing_object_name.startswith("_manifests/xero/"):
        reader = GCSReader(bucket=bucket_name, project=PROJECT)
        manifest = reader._read_data(object_name.removesuffix(".meta.json"))
        run, outcomes = manifest_rows(routing_object_name, manifest)
        project, dataset = target_dataset(
            bucket_name, [row["endpoint"] for row in outcomes], PROJECT,
        )
        written = write_manifest_rows(BQWriter(project=project, dataset=dataset), run, outcomes)
        logger.info("Recorded Xero run %s: %d history rows", run["run_id"], written)
        return

    projection = _prepare_current_projection(bucket_name, object_name, routing_object_name)
    if projection is not None:
        _publish_current_projections((projection,))
