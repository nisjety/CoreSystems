"""Credit term normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import erp_local_timestamp, keep_legacy_fields

LEGACY_FIELDS = [
    "id",
    "companyId",
    "updatedInDataMartDateTime",
    "termsId",
    "description",
    "dueDateType",
    "dayFrom1",
    "dayFrom2",
    "dayTo1",
    "dayTo2",
    "dueDay1",
    "dueDay2",
    "discountType",
    "discountDay",
    "discountPercent",
    "instalmentType",
    "numberOfInstalments",
    "instalmentFrequency",
    "instalmentMethod",
    "visibleTo",
    "delayDays",
    "moveDueDateIfOnTheWeekend",
    "calculationType",
    "lastModifiedDateTime",
    "deletedDatabaseRecord",
    "createdInDataMartDateTime",
]


# DataMart spells these enums out; the ERP holds one- or two-letter codes. Only values observed
# against the live ERP (all 22 terms of the pilot tenant, one code per DataMart value) are
# translated. A value not listed here passes through unchanged rather than getting a guessed code.
_ERP_CODES: dict[str, dict[str, str]] = {
    "calculationType": {"Gross": "G"},
    "discountType": {"FixedNumberOfDays": "N", "DayOfTheMonth": "T", "DayOfNextMonth": "D"},
    "dueDateType": {
        "FixedNumberOfDays": "N",
        "DayOfTheMonth": "T",
        "DayOfNextMonth": "D",
        "Custom": "C",
    },
    "instalmentFrequency": {"Weekly": "W"},
    "instalmentMethod": {"EqualParts": "E"},
    "instalmentType": {"Single": "S"},
    "moveDueDateIfOnTheWeekend": {"Keep the exact date": "1"},
    "visibleTo": {"All": "AL"},
}

# ERP names for the due-day windows; DataMart calls them dayFrom1 / dayTo1 / dayFrom2 / dayTo2.
_ERP_DAY_WINDOWS = {
    "dueDay1DayFrom1": "dayFrom1",
    "dueDay1DayTo1": "dayTo1",
    "dueDay2DayFrom1": "dayFrom2",
    "dueDay2DayTo1": "dayTo2",
}


def normalize_credit_term(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape DataMart Terms rows to the ERP contract (codes, ERP field names, ERP timestamps).

    ``createdDateTime`` stays null: ``createdInDataMartDateTime`` is when DataMart loaded the
    row (2026-02-15 for every live term), not when the term was created (ERP: 2023-09-12), so
    passing it on would be a wrong value on every row. It remains available as a DM-only field.
    """
    normalized = {field: dm_record.get(field) for field in LEGACY_FIELDS}
    for field, codes in _ERP_CODES.items():
        value = dm_record.get(field)
        normalized[field] = codes.get(value, value) if isinstance(value, str) else value
    for erp_name, dm_name in _ERP_DAY_WINDOWS.items():
        normalized[erp_name] = dm_record.get(dm_name)
    normalized["createdDateTime"] = None
    normalized["lastModifiedDateTime"] = erp_local_timestamp(dm_record.get("lastModifiedDateTime"))
    normalized["_source"] = "datamart"
    normalized["_updated_in_dm"] = dm_record.get("updatedInDataMartDateTime")
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
