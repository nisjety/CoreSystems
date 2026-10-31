"""Subaccount normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, first_non_empty, keep_legacy_fields

LEGACY_FIELDS = [
    "active",
    "description",
    "lastModifiedDateTime",
    "metadata",
    "segments",
    "subaccountId",
    "subaccountNumber",
    "timeStamp",
]


def normalize_subaccount(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart Subaccount rows to the legacy bronze contract."""
    normalized = {
        "active": dm_record.get("active"),
        "description": dm_record.get("description"),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "metadata": compact_json(
            {
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "segments": compact_json(_build_segments(dm_record)),
        "subaccountId": dm_record.get("subaccountId"),
        "subaccountNumber": first_non_empty(dm_record, "subaccountCd", "subaccountNumber"),
        # ERP timeStamp is a row version; DataMart has none, and its timestamps are not one.
        "timeStamp": None,
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized


def _build_segments(dm_record: dict[str, Any]) -> list[dict[str, Any]]:
    segments: list[dict[str, Any]] = []
    for idx in range(1, 11):
        segment_value = dm_record.get(f"cd{idx:02d}")
        segment_value_description = dm_record.get(f"description{idx:02d}")
        if segment_value in (None, "") and segment_value_description in (None, ""):
            continue
        segments.append(
            {
                "segmentId": idx,
                "segmentValue": segment_value,
                "segmentValueDescription": segment_value_description,
            }
        )
    return segments
