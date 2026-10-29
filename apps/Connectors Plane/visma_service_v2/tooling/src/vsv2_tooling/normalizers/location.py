"""Location normalizer."""

from __future__ import annotations

from typing import Any


def normalize_location(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart Location to v1 visma_locations bronze schema.

    The ERP row key is ``(baccount.number, locationId)``: a business-account *number* and a
    location *code* ("HOVED", "MAIN", ...). DataMart's Location has neither; ``locationId`` and
    ``bAccountId`` are internal integers (measured: all 20,290 live rows, none equal to any of
    the ERP's 116 codes). ``locationId`` is therefore left null so an internal number never
    masquerades under the ERP key name, and the internal ids are kept under their own names.
    The location code has no source in DataMart, so this endpoint cannot be keyed against the
    ERP and must stay ERP-sourced.
    """
    return {
        "id": dm_record.get("id"),
        "companyId": dm_record.get("companyId"),
        "locationId": None,
        "locationInternalId": dm_record.get("locationId"),
        "locationName": dm_record.get("locationName"),
        "active": dm_record.get("active"),
        "priceClassId": dm_record.get("priceClassId"),
        "bAccountInternalId": dm_record.get("bAccountId"),
        "bAccountId": dm_record.get("bAccountId"),
        "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
