"""Cloud Functions entrypoint for the Xero GCS-to-BigQuery loader."""

from etl.cloud_function.main import process_gcs_upload, process_fresh_gcs_upload


__all__ = ["process_fresh_gcs_upload", "process_gcs_upload"]
