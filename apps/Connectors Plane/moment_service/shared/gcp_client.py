"""
GCP client wrapper for the Moment service.

Thin wrapper around google-cloud-bigquery providing the project/dataset
context and helpers (ensure_table, load_rows, query_bigquery) used by
bigquery_sync.py and app.py.

Credentials are resolved from GOOGLE_APPLICATION_CREDENTIALS (a service
account JSON file) if set, otherwise from Application Default Credentials.
"""

import logging
import os
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

from dotenv import load_dotenv
from google.cloud import bigquery
from google.cloud.exceptions import NotFound
from google.oauth2 import service_account

logger = logging.getLogger(__name__)

REPO_ROOT = Path(__file__).resolve().parent.parent

# Load ./.env if present so this module also works when imported standalone,
# not just as a side effect of importing config.settings first.
load_dotenv(REPO_ROOT / ".env")


class GCPClient:
    """Wraps a BigQuery client with the project/dataset context used by this service."""

    def __init__(
        self,
        project_id: Optional[str] = None,
        dataset_id: Optional[str] = None,
        credentials_path: Optional[str] = None,
    ):
        self.project_id = project_id or os.getenv("GCP_PROJECT_ID")
        self.dataset_id = dataset_id or os.getenv("GCP_DATASET_ID", "moment_data")
        self.region = os.getenv("GCP_REGION", "europe-north1")

        credentials_path = credentials_path or os.getenv("GOOGLE_APPLICATION_CREDENTIALS")
        credentials = None
        if credentials_path:
            resolved_path = Path(credentials_path)
            if not resolved_path.is_absolute():
                # Resolve relative to the repo root, not the process cwd, so
                # this works the same whether the app is launched from the
                # project directory or from elsewhere (e.g. a supervisor).
                resolved_path = REPO_ROOT / resolved_path
            if resolved_path.exists():
                credentials = service_account.Credentials.from_service_account_file(str(resolved_path))
                if not self.project_id:
                    self.project_id = credentials.project_id
            else:
                logger.warning(f"GOOGLE_APPLICATION_CREDENTIALS file not found: {resolved_path}")

        if not self.project_id:
            raise ValueError("GCP_PROJECT_ID is required (or a credentials file with a project id)")

        self.bigquery = bigquery.Client(project=self.project_id, credentials=credentials)
        logger.info(f"Initialized GCPClient for project={self.project_id} dataset={self.dataset_id}")

    def _table_ref(self, table_id: str, dataset_id: Optional[str] = None) -> str:
        return f"{self.project_id}.{dataset_id or self.dataset_id}.{table_id}"

    def ensure_dataset(self, dataset_id: Optional[str] = None, location: Optional[str] = None) -> None:
        """Create the dataset if it doesn't already exist."""
        dataset_id = dataset_id or self.dataset_id
        location = location or self.region
        dataset_ref = f"{self.project_id}.{dataset_id}"
        try:
            self.bigquery.get_dataset(dataset_ref)
        except NotFound:
            logger.info(f"Creating BigQuery dataset {dataset_ref}")
            dataset = bigquery.Dataset(dataset_ref)
            dataset.location = location
            self.bigquery.create_dataset(dataset)

    def ensure_table(
        self,
        table_id: str,
        schema: Sequence[Dict[str, str]],
        dataset_id: Optional[str] = None,
    ) -> bigquery.Table:
        """
        Create the table with the given schema if it doesn't already exist.

        If the table exists but is missing columns the schema has since gained
        (e.g. the merge-tracking columns), the missing NULLABLE columns are
        added in place so existing data survives schema evolution.
        """
        table_ref = self._table_ref(table_id, dataset_id)
        try:
            table = self.bigquery.get_table(table_ref)
        except NotFound:
            self.ensure_dataset(dataset_id)
            logger.info(f"Creating BigQuery table {table_ref}")
            bq_schema = [
                bigquery.SchemaField(f["name"], f["type"], mode=f.get("mode", "NULLABLE"))
                for f in schema
            ]
            return self.bigquery.create_table(bigquery.Table(table_ref, schema=bq_schema))

        existing = {f.name for f in table.schema}
        missing = [f for f in schema if f["name"] not in existing]
        if missing:
            clauses = ", ".join(
                f"ADD COLUMN IF NOT EXISTS `{f['name']}` {f['type']}" for f in missing
            )
            logger.info(f"Adding columns to {table_ref}: {[f['name'] for f in missing]}")
            self.bigquery.query(f"ALTER TABLE `{table_ref}` {clauses}").result()
            table = self.bigquery.get_table(table_ref)
        return table

    def load_rows(
        self,
        table_id: str,
        rows: List[Dict[str, Any]],
        schema: Sequence[Dict[str, str]],
        dataset_id: Optional[str] = None,
        write_disposition: str = "WRITE_TRUNCATE",
    ) -> Dict[str, Any]:
        """
        Batch-load rows into BigQuery via a load job (not streaming inserts).

        A load job lets a full sync WRITE_TRUNCATE (full-refresh) so re-running
        it never accumulates duplicate rows, and avoids the streaming buffer's
        90-minute delete/update lockout that insert_rows_json is subject to.
        """
        if not rows:
            return {"loaded": 0, "table": self._table_ref(table_id, dataset_id)}

        self.ensure_table(table_id, schema, dataset_id)
        table_ref = self._table_ref(table_id, dataset_id)

        bq_schema = [
            bigquery.SchemaField(f["name"], f["type"], mode=f.get("mode", "NULLABLE"))
            for f in schema
        ]
        job_config = bigquery.LoadJobConfig(
            schema=bq_schema,
            write_disposition=write_disposition,
            source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        )
        job = self.bigquery.load_table_from_json(rows, table_ref, job_config=job_config)
        job.result()  # wait for completion; raises google.api_core.exceptions on failure
        return {
            "loaded": len(rows),
            "table": table_ref,
            "write_disposition": write_disposition,
            "job_id": job.job_id,
        }

    def merge_rows(
        self,
        table_id: str,
        rows: List[Dict[str, Any]],
        schema: Sequence[Dict[str, str]],
        dataset_id: Optional[str] = None,
        key_column: str = "id",
    ) -> Dict[str, Any]:
        """
        Upsert a full entity snapshot into the target table via MERGE.

        - New rows are inserted (first_synced_at stamped from the row's synced_at)
        - Existing rows are updated in place (first_synced_at preserved)
        - Rows present in the target but absent from this snapshot are soft-
          deleted: is_deleted_in_moment=TRUE + deleted_detected_at stamped,
          the row itself is NEVER removed
        - A row that reappears in Moment gets its deletion flags cleared again

        The snapshot is staged into <table>__stage (truncated each run) and the
        MERGE runs as one atomic statement, so a failed run can't half-apply.

        The tracking columns (first_synced_at, is_deleted_in_moment,
        deleted_detected_at) must be part of `schema`; they're computed here,
        not read from the rows.
        """
        tracking = {"first_synced_at", "is_deleted_in_moment", "deleted_detected_at"}
        data_cols = [f["name"] for f in schema if f["name"] not in tracking]
        if key_column not in data_cols:
            raise ValueError(f"key column '{key_column}' missing from schema for {table_id}")

        target_ref = self._table_ref(table_id, dataset_id)
        stage_ref = self._table_ref(f"{table_id}__stage", dataset_id)

        self.ensure_table(table_id, schema, dataset_id)

        # Stage the snapshot (schema without tracking cols; those live only on
        # the target). WRITE_TRUNCATE keeps the stage self-cleaning.
        stage_schema = [f for f in schema if f["name"] not in tracking]
        bq_schema = [
            bigquery.SchemaField(f["name"], f["type"], mode=f.get("mode", "NULLABLE"))
            for f in stage_schema
        ]
        job_config = bigquery.LoadJobConfig(
            schema=bq_schema,
            write_disposition="WRITE_TRUNCATE",
            source_format=bigquery.SourceFormat.NEWLINE_DELIMITED_JSON,
        )
        load_job = self.bigquery.load_table_from_json(rows, stage_ref, job_config=job_config)
        load_job.result()

        update_cols = [c for c in data_cols if c != key_column]
        update_set = ", ".join(f"T.`{c}` = S.`{c}`" for c in update_cols)
        insert_cols = ", ".join(f"`{c}`" for c in data_cols)
        insert_vals = ", ".join(f"S.`{c}`" for c in data_cols)

        merge_sql = f"""
        MERGE `{target_ref}` T
        USING `{stage_ref}` S
        ON T.`{key_column}` = S.`{key_column}`
        WHEN MATCHED THEN UPDATE SET
            {update_set},
            T.first_synced_at = COALESCE(T.first_synced_at, S.synced_at),
            T.is_deleted_in_moment = FALSE,
            T.deleted_detected_at = NULL
        WHEN NOT MATCHED THEN INSERT ({insert_cols}, first_synced_at, is_deleted_in_moment, deleted_detected_at)
            VALUES ({insert_vals}, S.synced_at, FALSE, NULL)
        WHEN NOT MATCHED BY SOURCE AND T.is_deleted_in_moment IS NOT TRUE THEN UPDATE SET
            T.is_deleted_in_moment = TRUE,
            T.deleted_detected_at = CURRENT_TIMESTAMP()
        """
        merge_job = self.bigquery.query(merge_sql)
        merge_job.result()

        result = {
            "table": target_ref,
            "merged": len(rows),
            "job_id": merge_job.job_id,
        }
        stats = merge_job.dml_stats
        if stats:
            # Soft-deletes are UPDATEs to BigQuery, so updated_row_count
            # includes them; the follow-up count reports them explicitly.
            result["inserted"] = stats.inserted_row_count
            result["updated"] = stats.updated_row_count
        flagged = self.query_bigquery(
            f"SELECT COUNTIF(is_deleted_in_moment) AS n, COUNT(*) AS total FROM `{target_ref}`"
        )[0]
        result["soft_deleted_total"] = flagged.n
        result["table_total_rows"] = flagged.total
        return result

    def insert_rows_to_bigquery(
        self,
        table_id: str,
        rows: List[Dict[str, Any]],
        dataset_id: Optional[str] = None,
        schema: Optional[Sequence[Dict[str, str]]] = None,
    ) -> Dict[str, Any]:
        """
        Append rows to a table.

        Prefer `load_rows(..., write_disposition="WRITE_TRUNCATE")` for full
        entity syncs. This append path exists for the ad-hoc single-batch sync
        endpoints and will create duplicate rows if called repeatedly with
        overlapping data.
        """
        if schema:
            return self.load_rows(table_id, rows, schema, dataset_id, write_disposition="WRITE_APPEND")

        if not rows:
            return {"loaded": 0}
        table_ref = self._table_ref(table_id, dataset_id)
        errors = self.bigquery.insert_rows_json(table_ref, rows)
        if errors:
            raise RuntimeError(f"BigQuery streaming insert errors for {table_ref}: {errors}")
        return {"loaded": len(rows), "table": table_ref}

    def query_bigquery(self, query: str):
        """Run a SQL query and return the completed result rows."""
        return list(self.bigquery.query(query).result())
