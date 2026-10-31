"""Contact normalizer."""

from __future__ import annotations

from typing import Any

from ._shared import erp_local_timestamp, keep_legacy_fields

LEGACY_FIELDS = [
    "id",
    "companyId",
    "updatedInDataMartDateTime",
    "contactId",
    "bAccountId",
    "contactType",
    "fullName",
    "displayName",
    "title",
    "salutation",
    "firstName",
    "midName",
    "lastName",
    "email",
    "webSite",
    "fax",
    "faxType",
    "phone1",
    "phone1Type",
    "phone2",
    "phone2Type",
    "phone3",
    "phone3Type",
    "defAddressId",
    "isActive",
    "languageId",
    "extRefNbr",
    "isDefaultContact",
    "deletedDatabaseRecord",
    "lastModifiedDateTime",
    "createdInDataMartDateTime",
]


def normalize_contact(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Pass DataMart Contact rows through to the bronze contract."""
    normalized = {field: dm_record.get(field) for field in LEGACY_FIELDS}
    normalized["lastModifiedDateTime"] = erp_local_timestamp(dm_record.get("lastModifiedDateTime"))
    normalized["_source"] = "datamart"
    normalized["_updated_in_dm"] = dm_record.get("updatedInDataMartDateTime")
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
