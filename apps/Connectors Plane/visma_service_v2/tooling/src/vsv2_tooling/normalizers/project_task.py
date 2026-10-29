"""Project task normalizer (`/v1/projecttask` bronze shape)."""

from __future__ import annotations

from typing import Any

from ._shared import compact_json, erp_local_timestamp, keep_legacy_fields, stringify_scalar

LEGACY_FIELDS = [
    "attributes",
    "branch",
    "createdDateTime",
    "defAccount",
    "defAccrualAccount",
    "defSub",
    "description",
    "employees",
    "endDate",
    "internalId",
    "lastModifiedDateTime",
    "metadata",
    "plannedEnd",
    "plannedStart",
    "projectInternalId",
    "rateTable",
    "restrictEmployees",
    "startDate",
    "status",
    "taskId",
    "taxCategory",
    "visibility",
]


def normalize_project_task(dm_record: dict[str, Any]) -> dict[str, Any]:
    """Shape a DataMart ProjectTask row to the ERP REST bronze contract.

    ERP ``taskId`` is the task *code* (DataMart ``taskCd``), which repeats across projects
    (66 codes over 3,108 tasks), so the row key is ``(projectInternalId, taskId)``. DataMart's
    own ``taskId`` is the internal id, i.e. the ERP ``internalId``.

    ``createdDateTime`` stays null: DataMart only knows when it loaded the row, which is not
    when the task was created. That value is kept in ``metadata``.
    """
    rate_table = dm_record.get("rateTableId")
    normalized = {
        "taskId": stringify_scalar(dm_record.get("taskCd")),
        "projectInternalId": dm_record.get("projectId"),
        "internalId": dm_record.get("taskId"),
        "description": dm_record.get("description"),
        "status": dm_record.get("status"),
        "plannedStart": erp_local_timestamp(dm_record.get("plannedStartDate")),
        "plannedEnd": erp_local_timestamp(dm_record.get("plannedEndDate")),
        "startDate": erp_local_timestamp(dm_record.get("startDate")),
        "endDate": erp_local_timestamp(dm_record.get("endDate")),
        "lastModifiedDateTime": erp_local_timestamp(dm_record.get("lastModifiedDateTime")),
        "rateTable": compact_json({"id": rate_table}) if rate_table else None,
        "metadata": compact_json(
            {
                "branchId": dm_record.get("branchId"),
                "companyId": dm_record.get("companyId"),
                "createdInDataMartDateTime": dm_record.get("createdInDataMartDateTime"),
                "dataMartId": dm_record.get("id"),
                "rateTableDescription": dm_record.get("rateTableDescription"),
                "updatedInDataMartDateTime": dm_record.get("updatedInDataMartDateTime"),
            }
        ),
        "_source": "datamart",
        "_updated_in_dm": dm_record.get("updatedInDataMartDateTime"),
    }
    keep_legacy_fields(normalized, LEGACY_FIELDS)
    return normalized
