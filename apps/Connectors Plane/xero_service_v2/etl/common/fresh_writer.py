"""Atomic, insert-only publication into the isolated v2 staging tables."""

import hashlib
import re
import threading
import time
import uuid

from google.api_core.exceptions import BadRequest, Conflict, NotFound
from google.cloud import bigquery

from etl.common.bq_writer import BQWriter
from etl.common.endpoint_config import ARRAY_KEYS
from etl.xero.run_history import RUN_SCHEMA, ENTITY_SCHEMA


# A duplicate Eventarc delivery always resolves to the same range partition.
# This keeps receipt idempotency serial for one source object while allowing
# unrelated raw observations to commit in parallel.
RECEIPT_LOCK_SHARDS = 128


def _schema(fields):
    return [bigquery.SchemaField(name, kind) for name, kind in fields]


_IDENTITY = [(name, "STRING") for name in
             ("generation", "tenant_id", "endpoint", "record_id", "version_id")]
_ORDER = [("observed_at", "TIMESTAMP"), ("source_updated_at", "TIMESTAMP")]
SCHEMAS = {
    "v2_record_versions": _schema(_IDENTITY + _ORDER + [
        ("run_id", "STRING"), ("receipt_id", "STRING"), ("source_status", "STRING"),
        ("source_deleted", "BOOLEAN"), ("payload_hash", "STRING"), ("payload_json", "STRING"),
    ]),
    "v2_collection_versions": _schema(_IDENTITY + _ORDER + [
        ("collection_name", "STRING"), ("child_count", "INTEGER"),
    ]),
    "v2_child_versions": _schema(_IDENTITY + [
        ("collection_name", "STRING"), ("child_key", "STRING"), ("line_position", "INTEGER"),
        ("native_id", "STRING"), ("payload_hash", "STRING"), ("payload_json", "STRING"),
    ]),
    "v2_ingest_receipts": _schema([
        (name, "STRING") for name in ("receipt_id", "generation", "tenant_id", "endpoint", "run_id")
    ] + [("observed_at", "TIMESTAMP")] + [
        (name, "STRING") for name in
        ("source_bucket", "source_object", "meta_generation", "data_generation")
    ] + [(name, "INTEGER") for name in
         ("http_status", "expected_records", "record_count", "child_count")]
      + [("metadata_json", "STRING")]),
}
SCHEMAS = {
    **SCHEMAS,
    "v2_sync_runs": [*RUN_SCHEMA, bigquery.SchemaField("generation", "STRING")],
    "v2_sync_run_entities": [*ENTITY_SCHEMA, bigquery.SchemaField("generation", "STRING")],
}


class FreshBQWriter(BQWriter):
    """Use a preseeded operational fence, never the legacy business MERGE."""

    schemas = SCHEMAS
    receipt_table = "v2_ingest_receipts"
    control_table = "v2_control"
    receipt_lock_table = "v2_receipt_locks"
    _receipt_locks_guard = threading.Lock()
    _receipt_locks = {}
    labels = {"source": "xero_v2", "layer": "staging"}
    description = "Fresh Xero v2 append-only staging; no legacy import or row expiration."

    def __init__(self, project, dataset):
        if (not re.fullmatch(r"[A-Za-z][A-Za-z0-9-]*", project)
                or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", dataset)):
            raise ValueError("invalid BigQuery identifier")
        super().__init__(project, dataset)

    def bootstrap(self):
        """Provision once before enabling delivery; never copy existing data."""
        for name, schema in self.schemas.items():
            table_ref = f"{self.project}.{self.dataset}.{name}"
            table = bigquery.Table(table_ref, schema=schema)
            if "observed_at" in {field.name for field in schema}:
                table.time_partitioning = bigquery.TimePartitioning(field="observed_at")
            clustering_fields = [
                name for name in ("generation", "tenant_id", "endpoint")
                if name in {field.name for field in schema}
            ]
            if clustering_fields:
                table.clustering_fields = clustering_fields
            if self.labels:
                table.labels = self.labels
            table.description = self.description
            try:
                existing = self.client.get_table(table_ref)
            except NotFound:
                self.client.create_table(table)
            else:
                existing_names = {field.name for field in existing.schema}
                missing = [field for field in schema if field.name not in existing_names]
                if missing:
                    if any(field.mode == "REQUIRED" for field in missing):
                        raise ValueError("fresh staging cannot add required fields to an existing table")
                    existing.schema = [*existing.schema, *missing]
                    self.client.update_table(existing, ["schema"])
        control = bigquery.Table(f"{self.project}.{self.dataset}.{self.control_table}", schema=_schema([
            ("control_id", "STRING"), ("revision", "INTEGER"),
        ]))
        self.client.create_table(control, exists_ok=True)
        receipt_locks = bigquery.Table(
            f"{self.project}.{self.dataset}.{self.receipt_lock_table}",
            schema=_schema([("shard_id", "INTEGER"), ("revision", "INTEGER")]),
        )
        receipt_locks.range_partitioning = bigquery.RangePartitioning(
            field="shard_id",
            range_=bigquery.PartitionRange(
                start=0,
                end=RECEIPT_LOCK_SHARDS,
                interval=1,
            ),
        )
        receipt_locks.require_partition_filter = True
        self.client.create_table(receipt_locks, exists_ok=True)
        controls = ",\n        ".join(
            f"STRUCT('publish-{index:02d}' AS control_id, 0 AS revision)"
            for index in range(16)
        )
        self.client.query(f"""
MERGE {self._full_table(self.control_table)} T
USING UNNEST([
        {controls}
]) S
ON T.control_id = S.control_id
WHEN NOT MATCHED THEN INSERT (control_id, revision) VALUES (S.control_id, S.revision)
""").result()
        self.client.query(f"""
MERGE {self._full_table(self.receipt_lock_table)} T
USING (
  SELECT shard_id, 0 AS revision
  FROM UNNEST(GENERATE_ARRAY(0, {RECEIPT_LOCK_SHARDS - 1})) AS shard_id
) S
ON T.shard_id = S.shard_id
 AND T.shard_id BETWEEN 0 AND {RECEIPT_LOCK_SHARDS - 1}
WHEN NOT MATCHED THEN INSERT (shard_id, revision) VALUES (S.shard_id, S.revision)
""").result()

    def transaction_sql(self, temps):
        statements = []
        ordered_temps = [
            *(item for item in temps.items() if item[0] != self.receipt_table),
            *(item for item in temps.items() if item[0] == self.receipt_table),
        ]
        for table, temp in ordered_temps:
            if table not in self.schemas:
                raise ValueError("unapproved fresh staging target")
            if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", temp):
                raise ValueError("invalid temporary table identifier")
            columns = ", ".join(field.name for field in self.schemas[table])
            statements.append(
                f"INSERT INTO {self._full_table(table)} ({columns}) "
                f"SELECT {columns} FROM {self._full_table(temp)};"
            )
        guards = self.preinsert_assertions_sql(temps)
        return f"""
BEGIN TRANSACTION;
UPDATE {self._full_table(self.control_table)}
SET revision = revision + 1
WHERE control_id = 'publish-00';
ASSERT @@row_count = 1 AS 'Fresh staging publication fence must be provisioned exactly once';
UPDATE {self._full_table(self.receipt_lock_table)}
SET revision = revision + 1
WHERE shard_id = @receipt_shard;
ASSERT @@row_count = 1 AS 'Fresh staging receipt lock must be provisioned exactly once';
IF NOT EXISTS (SELECT 1 FROM {self._full_table(self.receipt_table)} WHERE receipt_id = @receipt_id) THEN
  {guards}
  {chr(10).join(statements)}
END IF;
COMMIT TRANSACTION;
"""

    def preinsert_assertions_sql(self, temps):
        """Return source-specific transaction assertions before publication."""
        return ""

    def batch_transaction_sql(self, temps):
        """Insert a bounded set of unseen observations as one raw-history commit.

        A batch is only suitable for response observations: each staged row
        must carry the receipt that proves its pinned source object. Run
        manifests remain independently published because their operational
        history has a different key and no receipt reference.
        """
        receipt_temp = temps.get(self.receipt_table)
        if not receipt_temp:
            raise ValueError("fresh batch publication requires receipt rows")
        statements = []
        ordered_temps = [
            *(item for item in temps.items() if item[0] != self.receipt_table),
            *(item for item in temps.items() if item[0] == self.receipt_table),
        ]
        for table, temp in ordered_temps:
            if table not in self.schemas:
                raise ValueError("unapproved fresh staging target")
            if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", temp):
                raise ValueError("invalid temporary table identifier")
            fields = self.schemas[table]
            columns = ", ".join(field.name for field in fields)
            select_columns = ", ".join(f"source.{field.name}" for field in fields)
            receipt_column = self._batch_receipt_column(table)
            if table == self.receipt_table:
                statements.append(f"""
INSERT INTO {self._full_table(table)} ({columns})
SELECT {select_columns}
FROM {self._full_table(temp)} AS source
WHERE NOT EXISTS (
  SELECT 1 FROM {self._full_table(self.receipt_table)} AS existing
  WHERE existing.receipt_id = source.receipt_id
);""")
            else:
                statements.append(f"""
INSERT INTO {self._full_table(table)} ({columns})
SELECT {select_columns}
FROM {self._full_table(temp)} AS source
WHERE NOT EXISTS (
  SELECT 1 FROM {self._full_table(self.receipt_table)} AS existing
  WHERE existing.receipt_id = source.{receipt_column}
);""")
        return f"""
BEGIN TRANSACTION;
UPDATE {self._full_table(self.control_table)}
SET revision = revision + 1
WHERE control_id = 'publish-00';
ASSERT @@row_count = 1 AS 'Fresh staging publication fence must be provisioned exactly once';
{chr(10).join(statements)}
COMMIT TRANSACTION;
"""

    @staticmethod
    def receipt_shard_for(receipt_id):
        if not isinstance(receipt_id, str) or not receipt_id:
            raise ValueError("fresh publication requires a nonempty receipt id")
        digest = hashlib.sha256(receipt_id.encode("utf-8")).digest()
        return int.from_bytes(digest[:8], "big") % RECEIPT_LOCK_SHARDS

    def _receipt_lock_for(self, receipt_shard):
        key = (self.project, self.dataset, self.receipt_lock_table, receipt_shard)
        with self._receipt_locks_guard:
            lock = self._receipt_locks.get(key)
            if lock is None:
                lock = threading.Lock()
                self._receipt_locks[key] = lock
            return lock

    def commit(self, sql, receipt_id, receipt_shard):
        if type(receipt_shard) is not int or not 0 <= receipt_shard < RECEIPT_LOCK_SHARDS:
            raise ValueError("invalid fresh staging receipt lock shard")
        config = bigquery.QueryJobConfig(query_parameters=[
            bigquery.ScalarQueryParameter("receipt_id", "STRING", receipt_id),
            bigquery.ScalarQueryParameter("receipt_shard", "INTEGER", receipt_shard),
        ])
        # Cloud Run is intentionally limited to one instance. Serialize only
        # colliding receipt shards in that process; unrelated shards retain
        # bounded request parallelism, while the partitioned BigQuery lock
        # preserves the same guarantee if the runtime topology changes.
        with self._receipt_lock_for(receipt_shard):
            for attempt in range(5):
                try:
                    self.client.query(sql, job_config=config).result()
                    return
                except (BadRequest, Conflict) as exc:
                    concurrency = any(fragment in str(exc).lower() for fragment in
                                      ("concurrent update", "transaction aborted", "serialize access"))
                    if not concurrency or attempt == 4:
                        raise
                    time.sleep(0.25 * (2 ** attempt))

    def commit_many(self, sql):
        """Run a fenced batch transaction, retrying only serialization races."""
        with self._receipt_lock_for(0):
            for attempt in range(5):
                try:
                    self.client.query(sql).result()
                    return
                except (BadRequest, Conflict) as exc:
                    concurrency = any(fragment in str(exc).lower() for fragment in
                                      ("concurrent update", "transaction aborted", "serialize access"))
                    if not concurrency or attempt == 4:
                        raise
                    time.sleep(0.25 * (2 ** attempt))

    def publish(self, batches):
        receipts = batches.get(self.receipt_table, [])
        if len(receipts) != 1:
            raise ValueError("fresh publication requires exactly one receipt")
        for table, rows in batches.items():
            if table not in self.schemas:
                raise ValueError("unapproved fresh staging target")
            expected = {field.name for field in self.schemas[table]}
            if any(set(row) != expected for row in rows):
                raise ValueError(f"fresh staging schema mismatch: {table}")
            if (table != self.receipt_table and "receipt_id" in expected
                    and any(row["receipt_id"] != receipts[0]["receipt_id"] for row in rows)):
                raise ValueError("fresh publication row receipt mismatch")
        temps = {}
        try:
            for table, rows in batches.items():
                if rows:
                    temp = f"_tmp_fresh_{table}_{uuid.uuid4().hex}"
                    temps[table] = temp
                    self._write_temp(self._full_table(temp), rows, schema=self.schemas[table])
            receipt_id = receipts[0]["receipt_id"]
            self.commit(
                self.transaction_sql(temps),
                receipt_id,
                self.receipt_shard_for(receipt_id),
            )
        finally:
            for temp in temps.values():
                self._drop_temp(self._full_table(temp))

    def publish_many(self, observations):
        """Publish multiple raw response observations without losing provenance.

        Each observation keeps its original receipt identity. A retry skips an
        entire already-committed observation, so this is suitable for initial
        load/backfill work but cannot rewrite or delete history.
        """
        combined = {}
        receipt_ids = set()
        count = 0
        for batches in observations:
            receipts = batches.get(self.receipt_table, [])
            if len(receipts) != 1:
                raise ValueError("fresh batch observation requires exactly one receipt")
            receipt_id = receipts[0].get("receipt_id")
            if not isinstance(receipt_id, str) or not receipt_id:
                raise ValueError("fresh batch observation requires a receipt id")
            if receipt_id in receipt_ids:
                raise ValueError("fresh batch observation has a duplicate receipt")
            receipt_ids.add(receipt_id)
            count += 1
            for table, rows in batches.items():
                if table not in self.schemas:
                    raise ValueError("unapproved fresh staging target")
                expected = {field.name for field in self.schemas[table]}
                if any(set(row) != expected for row in rows):
                    raise ValueError(f"fresh staging schema mismatch: {table}")
                receipt_column = self._batch_receipt_column(table)
                if (receipt_column == "receipt_id" and table != self.receipt_table
                        and any(row["receipt_id"] != receipt_id for row in rows)):
                    raise ValueError("fresh batch row receipt mismatch")
                combined.setdefault(table, []).extend(
                    row if receipt_column == "receipt_id" else row | {
                        "_batch_receipt_id": receipt_id,
                    }
                    for row in rows
                )
        if not count:
            raise ValueError("fresh batch publication requires at least one observation")

        temps = {}
        try:
            for table, rows in combined.items():
                if rows:
                    temp = f"_tmp_fresh_{table}_{uuid.uuid4().hex}"
                    temps[table] = temp
                    self._write_temp(
                        self._full_table(temp),
                        rows,
                        schema=self._batch_temp_schema(table),
                    )
            self.commit_many(self.batch_transaction_sql(temps))
        finally:
            for temp in temps.values():
                self._drop_temp(self._full_table(temp))

    def _batch_receipt_column(self, table):
        fields = {field.name for field in self.schemas[table]}
        if "receipt_id" in fields:
            return "receipt_id"
        if {"collection_name", "version_id"} <= fields:
            return "_batch_receipt_id"
        raise ValueError("fresh batch rows require receipt provenance")

    def _batch_temp_schema(self, table):
        schema = self.schemas[table]
        if self._batch_receipt_column(table) == "receipt_id":
            return schema
        return [*schema, bigquery.SchemaField("_batch_receipt_id", "STRING")]


ENDPOINT_SCHEMAS = {
    **{endpoint: SCHEMAS["v2_record_versions"] for endpoint in ARRAY_KEYS},
    "accounting_get_responses": SCHEMAS["v2_record_versions"],
    "collection_versions": SCHEMAS["v2_collection_versions"],
    "child_versions": SCHEMAS["v2_child_versions"],
    "ingest_receipts": SCHEMAS["v2_ingest_receipts"],
    "sync_runs": SCHEMAS["v2_sync_runs"],
    "sync_run_entities": SCHEMAS["v2_sync_run_entities"],
}

_ENDPOINT_METADATA_TABLES = {
    "v2_collection_versions": "collection_versions",
    "v2_child_versions": "child_versions",
    "v2_ingest_receipts": "ingest_receipts",
    "v2_sync_runs": "sync_runs",
    "v2_sync_run_entities": "sync_run_entities",
}

WAREHOUSE_RAW_SCHEMAS = {
    "raw_records": SCHEMAS["v2_record_versions"],
    "raw_collections": SCHEMAS["v2_collection_versions"],
    "raw_children": SCHEMAS["v2_child_versions"],
    "ingest_receipts": SCHEMAS["v2_ingest_receipts"],
    "sync_runs": RUN_SCHEMA,
    "sync_run_entities": ENTITY_SCHEMA,
}

_WAREHOUSE_RAW_TABLES = {
    "v2_record_versions": "raw_records",
    "v2_collection_versions": "raw_collections",
    "v2_child_versions": "raw_children",
    "v2_ingest_receipts": "ingest_receipts",
    "v2_sync_runs": "sync_runs",
    "v2_sync_run_entities": "sync_run_entities",
}


class EndpointBQWriter(FreshBQWriter):
    """Isolated natural-name staging layout for the main Xero pipeline."""

    schemas = ENDPOINT_SCHEMAS
    receipt_table = "ingest_receipts"
    control_table = "control"
    labels = None
    description = "Immutable raw Xero observations; no legacy source import."

    def __init__(self, project, dataset):
        if dataset != "dw_staging_xero_main":
            raise ValueError("endpoint layout requires the isolated main Xero staging dataset")
        super().__init__(project, dataset)

    @staticmethod
    def _record_target(endpoint):
        if endpoint in ARRAY_KEYS:
            return endpoint
        if isinstance(endpoint, str) and endpoint.startswith("accounting_get_"):
            from etl.xero.accounting_get import operation_info

            operation, binary = operation_info(endpoint)
            if binary or operation in {"getPaymentServices", "getBrandingThemePaymentServices"}:
                raise ValueError("endpoint is not an ordinary Xero staging record")
            return "accounting_get_responses"
        raise ValueError("unsupported Xero staging endpoint")

    def publish_observation(self, batches):
        mapped = {}
        for source_table, rows in batches.items():
            if source_table == "v2_record_versions":
                for row in rows:
                    target = self._record_target(row.get("endpoint"))
                    mapped.setdefault(target, []).append(row)
            else:
                target = _ENDPOINT_METADATA_TABLES.get(source_table)
                if target is None:
                    raise ValueError("unapproved fresh staging target")
                mapped[target] = rows
        self.publish(mapped)


class WarehouseRawBQWriter(FreshBQWriter):
    """Write immutable raw history beside the current typed Xero staging tables.

    The public BigQuery contract deliberately uses natural names. Internal
    batch keys remain stable while the extractor migration is in progress.
    """

    schemas = WAREHOUSE_RAW_SCHEMAS
    receipt_table = "ingest_receipts"
    control_table = "raw_control"
    receipt_lock_table = "raw_receipt_locks"
    labels = None
    description = "Immutable raw Xero observations and run provenance; no legacy import or row expiration."

    def __init__(self, project, dataset):
        if dataset != "dw_staging_xero":
            raise ValueError("warehouse raw layout requires dw_staging_xero")
        super().__init__(project, dataset)

    def _map_observation(self, batches):
        mapped = {}
        for source_table, rows in batches.items():
            target = _WAREHOUSE_RAW_TABLES.get(source_table)
            if target is None:
                raise ValueError("unapproved fresh staging target")
            if not isinstance(rows, list):
                raise ValueError("fresh staging batches must be lists")
            source_fields = {field.name for field in SCHEMAS[source_table]}
            target_fields = {field.name for field in self.schemas[target]}
            allowed_extra = {"generation"} if source_table in {
                "v2_sync_runs", "v2_sync_run_entities"
            } else set()
            if source_fields - target_fields - allowed_extra:
                raise ValueError("fresh staging schema mapping is incomplete")
            normalized = []
            for row in rows:
                if not isinstance(row, dict) or set(row) != source_fields:
                    raise ValueError("fresh staging schema mismatch")
                normalized.append({field.name: row.get(field.name) for field in self.schemas[target]})
            mapped[target] = normalized
        return mapped

    def preinsert_assertions_sql(self, temps):
        sync_runs_temp = temps.get("sync_runs")
        if sync_runs_temp is None:
            return ""
        if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", sync_runs_temp):
            raise ValueError("invalid temporary table identifier")
        return f"""
ASSERT NOT EXISTS (
  SELECT 1
  FROM {self._full_table("sync_runs")} AS existing
  INNER JOIN {self._full_table(sync_runs_temp)} AS incoming
    USING (run_id)
) AS 'Fresh staging manifest run ID already exists';"""

    def publish_observation(self, batches):
        self.publish(self._map_observation(batches))

    def publish_observations_many(self, observations):
        self.publish_many([self._map_observation(batches) for batches in observations])
