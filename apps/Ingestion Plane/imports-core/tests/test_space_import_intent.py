from pathlib import Path

import pytest
from pydantic import ValidationError

from app.models import ImportJob
from app.schemas import SpaceImportIntent


def _intent(**overrides: object) -> dict[str, object]:
    value: dict[str, object] = {
        "space_ref": "space:org-1:user-1",
        "subject_id": "user-1",
        "resource_authorization_ref": "resource:import:1",
        "recipient_audience_ref": "audience:user-1:1",
        "privacy_policy_ref": "privacy:org-1:1",
        "authority_revision": 3,
        "membership_revision": 3,
        "privacy_revision": 2,
        "recipient_audience_revision": 3,
        "entitlement_revision": 4,
        "action_schema_hash": "sha256:ingestion-import-v1",
        "payload_digest": "sha256:payload",
        "idempotency_key": "import-1",
        "source_type": "notion",
        "purpose": "knowledge_import",
        "lawful_basis": "contract",
        "privacy_class": "internal",
        "third_party_allowed": False,
        "retention_class": "standard",
        "residency": "eu-north-1",
        "deletion_scope": "space",
        "zero_data_retention": False,
    }
    value.update(overrides)
    return value


def test_space_import_intent_is_non_secret_and_complete() -> None:
    intent = SpaceImportIntent.model_validate(_intent())

    assert intent.space_ref == "space:org-1:user-1"
    assert intent.action_id == "ingestion.import.write"
    assert intent.action_schema_hash == "sha256:ingestion-import-v1"
    assert "token" not in intent.model_dump(mode="json")

    with pytest.raises(ValidationError):
        SpaceImportIntent.model_validate(_intent(space_decision_token="secret"))


def test_import_jobs_have_durable_non_secret_space_intent_storage() -> None:
    assert "space_import_intent" in ImportJob.__table__.columns
    migration = (Path(__file__).parents[1] / "migrations" / "004_space_import_intent.sql").read_text(
        encoding="utf-8"
    ).lower()
    assert "space_import_intent jsonb" in migration
    assert "space_decision_token" not in migration


def test_additive_space_intent_migration_does_not_trigger_legacy_table_archive() -> None:
    # The compatibility guard runs before migrations. New additive columns
    # belong to the migration itself, otherwise every existing durable job
    # table would be archived during the rollout.
    from app.db import CURRENT_IMPORT_JOBS_COLUMNS

    assert "space_import_intent" not in CURRENT_IMPORT_JOBS_COLUMNS
