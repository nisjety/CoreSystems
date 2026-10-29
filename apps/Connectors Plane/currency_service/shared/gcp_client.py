"""GCP client helpers for BigQuery and Cloud Storage."""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Iterable, Optional

from google.api_core.exceptions import NotFound
from google.cloud import bigquery
from google.cloud import storage
from google.oauth2 import service_account


@dataclass
class GCPClient:
    """Small helper wrapper around BigQuery and Storage clients."""

    project_id: str
    dataset_id: str
    bucket_name: Optional[str]
    bigquery: bigquery.Client
    storage: storage.Client
    # BigQuery location of the dataset (GCP_BQ_LOCATION). Only used to create a missing dataset
    # where the warehouse reads it; unset keeps the BigQuery default.
    location: Optional[str] = None

    def create_bigquery_table(
        self,
        table_id: str,
        schema: list[bigquery.SchemaField],
        partitioning_field: Optional[str] = None,
    ) -> bigquery.Table:
        """Create a BigQuery table if it does not exist."""
        dataset_ref = bigquery.DatasetReference(self.project_id, self.dataset_id)
        try:
            self.bigquery.get_dataset(dataset_ref)
        except NotFound:
            dataset = bigquery.Dataset(dataset_ref)
            if self.location:
                dataset.location = self.location
            self.bigquery.create_dataset(dataset, exists_ok=True)

        table_ref = dataset_ref.table(table_id)
        try:
            return self.bigquery.get_table(table_ref)
        except NotFound:
            table = bigquery.Table(table_ref, schema=schema)
            if partitioning_field:
                table.time_partitioning = bigquery.TimePartitioning(
                    field=partitioning_field
                )
            return self.bigquery.create_table(table)

    def insert_rows_to_bigquery(self, table_id: str, rows: Iterable[dict]) -> None:
        """Insert rows into a BigQuery table."""
        table_ref = f"{self.project_id}.{self.dataset_id}.{table_id}"
        errors = self.bigquery.insert_rows_json(table_ref, list(rows))
        if errors:
            raise RuntimeError(f"BigQuery insert errors: {errors}")

    def query_bigquery(self, query: str):
        """Run a BigQuery query and return the results iterator."""
        job = self.bigquery.query(query)
        return job.result()

    def upload_to_storage(self, blob_name: str, data: bytes, content_type: str) -> str:
        """Upload a blob to Cloud Storage and return the public URL."""
        if not self.bucket_name:
            raise RuntimeError("GCP_BUCKET_NAME is not configured")
        bucket = self.storage.bucket(self.bucket_name)
        blob = bucket.blob(blob_name)
        blob.upload_from_string(data, content_type=content_type)
        return blob.public_url


def get_gcp_client() -> GCPClient:
    """Create a configured GCP client instance from environment."""
    project_id = os.getenv("GCP_PROJECT_ID")
    dataset_id = os.getenv("GCP_DATASET_ID")
    bucket_name = os.getenv("GCP_BUCKET_NAME")
    location = os.getenv("GCP_BQ_LOCATION") or None
    credentials_path = os.getenv("GOOGLE_APPLICATION_CREDENTIALS")

    if not project_id or not dataset_id:
        raise RuntimeError("GCP_PROJECT_ID and GCP_DATASET_ID must be set")

    credentials = None
    if credentials_path:
        if not os.path.exists(credentials_path):
            raise RuntimeError(
                f"GOOGLE_APPLICATION_CREDENTIALS file not found: {credentials_path}"
            )
        credentials = service_account.Credentials.from_service_account_file(
            credentials_path
        )

    return GCPClient(
        project_id=project_id,
        dataset_id=dataset_id,
        bucket_name=bucket_name,
        bigquery=bigquery.Client(
            project=project_id, credentials=credentials, location=location
        ),
        storage=storage.Client(project=project_id, credentials=credentials),
        location=location,
    )
