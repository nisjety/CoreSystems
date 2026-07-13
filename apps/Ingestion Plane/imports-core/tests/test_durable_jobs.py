from pathlib import Path

from app.models import ImportJob, ImportJobItem


def test_job_models_include_recovery_lease_and_durable_payload() -> None:
    job_columns = ImportJob.__table__.columns
    item_columns = ImportJobItem.__table__.columns

    assert {"lease_owner", "lease_expires_at", "attempts"}.issubset(job_columns.keys())
    assert "document_payload" in item_columns


def test_durable_job_migration_is_idempotent_and_indexed() -> None:
    migration = (
        Path(__file__).parents[1] / "migrations" / "003_durable_job_leases.sql"
    ).read_text(encoding="utf-8").lower()

    assert migration.count("add column if not exists") == 4
    assert "idx_import_jobs_recovery" in migration
