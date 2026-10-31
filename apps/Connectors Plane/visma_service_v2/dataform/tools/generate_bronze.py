#!/usr/bin/env python3
"""Generate the bronze layer: typed tables built from the raw JSON in dw_staging_visma.

Bronze keeps the columns of the v1 bronze tables (same names, same order, same types) so that the
silver and gold models written for v1 run on it unchanged. Where a v1 column has no counterpart in
what v2 fetches, it is NULL (and listed in the report); where v2 fetches a field v1 never had, it
is added after the v1 columns.

Inputs, all in tools/contract/ (so this runs without BigQuery):
  v1_bronze_schema.json   the v1 bronze tables as BigQuery had them (columns, nested paths, types)
  v1_descriptions.json    their descriptions
  staging_profile.json    per staging endpoint: payload keys, how many rows, which JSON types
                          (written by profile_staging.py)

Writes:
  definitions/bronze/visma/<table>.sqlx   one table per v1 bronze table that v2 can fill
  definitions/bronze/visma/visma_tenants.sqlx   the tenant names (from config/tenants/)
  definitions/sources/staging_visma.js    declarations for every staging table
  tools/contract/bronze_report.md         what is filled, NULL, renamed or added, per table

The files are meant to be edited by hand afterwards (see OVERRIDES for the ones that need more
than a field-for-field copy); running this again overwrites them.

Usage: dataform/tools/generate_bronze.py
"""

from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
CONTRACT = HERE / "contract"
TENANTS = ROOT.parent / "config" / "tenants"

BRONZE = "visma_bronze"
STAGING = "dw_staging_visma"

# v1 columns that describe the sync, not the record. Filled from the staging row's own columns.
LINEAGE = {
    "syncId": "s.last_run_id",
    "syncedAt": "s.updated_at",
    "firstSyncedAt": "s.first_seen_at",
    "refreshedAt": "s.updated_at",
    "seenInFullScopeRuns": "CAST(NULL AS INT64)",
    "consecutiveMissingFullRuns": "CAST(NULL AS INT64)",
    "isDeleted": "s.deleted_at IS NOT NULL",
    "deletedAt": "s.deleted_at",
    "tenantId": "s.tenant_id",
    "syncBatch": "CAST(NULL AS STRING)",
    "organizationName": "tenant.tenant_name",
}

# Columns of v1's own pipeline (batch partitioning, validation) that nothing in v2 has.
PIPELINE_INTERNAL = {"partitionDate", "_validator", "_validated_at"}

# v1 table -> staging endpoint, when the name is not simply the table name without `visma_`.
ENDPOINT = {}
# v1 tables that are not built from a staging endpoint by this generator.
NOT_GENERATED = {
    "visma_price_classes": "derived from customers and customer locations (hand-written file)",
    "visma_sales_orders_v2": "the v2 sales order API is retired; v2 only has sales_orders_v3",
}
JUNK = ("_stg_", "__predup_bak", "_backup_")

# A v1 column that v2 fetches under another name: v1 table -> {v1 column: payload key}.
RENAMES: dict[str, dict[str, str]] = {
    "visma_journal_transactions": {"batchNbr": "batchNumber"},
}

# Tables v1 built from two endpoints. `join` = (staging endpoint, ON condition on `s` and `o`);
# `from_other` = the v1 columns read from the joined row instead of the table's own endpoint.
# The Sales Order API lists customers and inventory with fields the ERP API does not have, and v1
# merged them into the ERP table on the customer number / inventory number.
OVERRIDES: dict[str, dict] = {
    "visma_customers": {
        "join": (
            "sales_order_customers",
            "o.tenant_id = s.tenant_id AND JSON_VALUE(o.payload, '$.id') = JSON_VALUE(s.payload, '$.number')",
        ),
        "from_other": [
            "addressLine1", "addressLine2", "country", "customerClassId", "defaultLocation",
            "exportType", "id", "postalCode", "salesAccountId", "salesEuAccountId",
            "salesExportAccountId", "salesNonTaxableAccountId", "salutation",
        ],
    },
    "visma_inventory_items": {
        "join": (
            "sales_order_inventory",
            "o.tenant_id = s.tenant_id AND TRIM(JSON_VALUE(o.payload, '$.inventoryId')) = TRIM(JSON_VALUE(s.payload, '$.inventoryNumber'))",
        ),
        "from_other": ["availability", "internalId"],
    },
}  # fmt: skip


def obj(base: str, **fields) -> str:
    """JS for bronze.object(...): a JSON text built from a nested object under v1's key names."""
    return f"bronze.object({json.dumps(base)}, {json.dumps(fields)})"


def named(base: str) -> str:
    """{id, description, name} from a nested {id, description}: how v1 stored a reference."""
    return obj(base, id="id", description="description", name="description")


_ADDRESS = {
    "addressId": ["id", "int"],
    "overrideAddress": ["overridesDefault", "bool"],
    "addressLine1": "line1",
    "addressLine2": "line2",
    "addressLine3": "line3",
    "postalCode": "postalCode",
    "city": "city",
    "country": "country.id",
    "county": "county.id",
}
_CONTACT = {
    "contactId": ["id", "int"], "overrideContact": ["overridesDefault", "bool"], "name": "name",
    "attention": "attention", "email": "email", "phone1": "phone1", "phone2": "phone2", "fax": "fax",
}  # fmt: skip

# v1 column -> where v2's payload has it, for columns that are not a top-level field of the payload.
# A path ("totals.taxTotal") reads one nested field, cast to the column's v1 type. Text starting
# with `bronze.` is a ready JS expression (see includes/bronze.js).
#
# visma_sales_orders_v3: v1 kept the Sales Order v3 fields and added the same data again under the
# names of the older v2 API (orderNo, canceled, soBillingAddress, vatTaxableTotal, ...), by
# flattening nested objects. The silver models read both, but several only the flattened names.
# `lines` and `shipments` are left out on purpose: silver takes `orderLines` first.
COLUMNS: dict[str, dict[str, str]] = {
    "visma_sales_orders_v3": {
        "orderNo": "orderId",
        "orderType": "type",
        "lastModifiedDateTime": "lastModified",
        "timeStamp": "version",
        "canceled": "statusDetails.cancelled",
        "hold": "statusDetails.hold",
        "cashAccountId": "paymentSettings.cashAccountId",
        "paymentReference": "paymentSettings.paymentReference",
        "invoiceSeparately": "financialInformation.invoiceSeparately",
        "invoiceDate": "financialInformation.invoiceDate",
        "dueDate": "financialInformation.dueDate",
        "cashDiscountDate": "financialInformation.cashDiscountDate",
        "postPeriod": "financialInformation.postPeriod",
        "invoiceId": "financialInformation.invoiceId",
        "priority": "shipping.priority",
        "residentialDelivery": "shipping.residentialDelivery",
        "saturdayDelivery": "shipping.saturdayDelivery",
        "shipSeparately": "shipping.shipSeparately",
        "insurance": "shipping.insurance",
        "shippingRule": "shipping.rule",
        "scheduledDate": "shipping.scheduledDate",
        "orderTotalInBaseCurrency": "totals.orderTotalInBaseCurrency",
        "taxTotalInBaseCurrency": "totals.taxTotalInBaseCurrency",
        "discountTotal": "totals.discountTotal",
        "discountTotalInBaseCurrency": "totals.discountTotalInBaseCurrency",
        "vatExemptTotal": "totals.taxExemptTotal",
        "vatExemptTotalInBaseCurrency": "totals.taxExemptTotalInBaseCurrency",
        "vatTaxableTotal": "totals.taxableTotal",
        "vatTaxableTotalInBaseCurrency": "totals.taxableTotalInBaseCurrency",
        "costTotal": "totals.costTotal",
        "costTotalInBaseCurrency": "totals.costTotalInBaseCurrency",
        "exciseDutyTotal": "totals.exciseDutyTotal",
        "exciseDutyTotalInBaseCurrency": "totals.exciseDutyTotalInBaseCurrency",
        "freightTotal": "totals.freightTotal",
        "freightTotalInBaseCurrency": "totals.freightTotalInBaseCurrency",
        "freightCost": "totals.freightCost",
        "freightCostInBaseCurrency": "totals.freightCostInBaseCurrency",
        "unbilledAmount": "totals.unbilledAmount",
        "unbilledAmountInBaseCurrency": "totals.unbilledAmountInBaseCurrency",
        "unshippedAmount": "totals.unshippedAmount",
        "unshippedAmountInBaseCurrency": "totals.unshippedAmountInBaseCurrency",
        "paymentTotal": "totals.paymentTotal",
        "paymentTotalInBaseCurrency": "totals.paymentTotalInBaseCurrency",
        "branchNumber": obj("branch", number="id", name="description"),
        "paymentMethod": named("paymentSettings.paymentMethod"),
        "terms": named("financialInformation.terms"),
        "preferredWarehouse": named("shipping.preferredWarehouse"),
        "shipVia": named("shipping.shipVia"),
        "shippingTerms": named("shipping.terms"),
        "shippingZone": named("shipping.zone"),
        "fobPoint": named("shipping.fobPoint"),
        "intrastatTransactionType": obj(
            "shipping.intrastatTransactionType", id=["id", "int"], description="description"
        ),
        "soBillingAddress": obj("billing.address", **_ADDRESS),
        "soBillingContact": obj("billing.contact", **_CONTACT),
        "soShippingAddress": obj("shipping.address", **_ADDRESS),
        "soShippingContact": obj("shipping.contact", **_CONTACT),
    },
    # The Sales Order API gave v1 the attributes as {id: value}; the ERP API gives [{id, value}].
    "visma_inventory_items": {"attributes": 'bronze.attributes("s.payload")'},
}

SQL_TYPES = {"STRING": "str", "INT64": "int", "FLOAT64": "float", "BOOL": "bool", "TIMESTAMP": "ts"}


def endpoint_of(table: str) -> str:
    return ENDPOINT.get(table) or table.removeprefix("visma_")


def infer(types: list[str]) -> str:
    """BigQuery type for a v2-only field, from the JSON types seen in a sample."""
    seen = set(types) - {"null"}
    if not seen:
        return "STRING"
    if seen <= {"int"}:
        return "INT64"
    if seen <= {"int", "float"}:
        return "FLOAT64"
    if seen <= {"bool"}:
        return "BOOL"
    return "STRING"


def top_level(columns: list[dict]) -> list[tuple[str, str]]:
    seen: dict[str, str] = {}
    for column in columns:
        if "." not in column["path"]:
            seen.setdefault(column["column"], column["type"])
    return list(seen.items())


def render(
    table: str, endpoint: str, columns: list[tuple[str, str]], profile: dict, notes: dict
) -> str:
    keys = profile["keys"]
    renames = RENAMES.get(table, {})
    from_other = set(OVERRIDES.get(table, {}).get("from_other", []))
    mapped = COLUMNS.get(table, {})
    # With a joined row there are two `payload` columns, so the table's own fields name theirs.
    own = ', "s.payload"' if table in OVERRIDES else ""
    lines: list[str] = []
    filled, nulled, renamed = [], [], []
    for name, sql_type in columns:
        if name in PIPELINE_INTERNAL or sql_type.startswith("STRUCT"):
            continue
        alias = f"`{name}`"
        if name in LINEAGE:
            lines.append(f"  {LINEAGE[name]} AS {alias},")
        elif name in mapped:
            spec = mapped[name]
            if spec.startswith("bronze."):
                lines.append(f"  ${{{spec}}} AS {alias},")
            else:
                kind = SQL_TYPES.get(sql_type, "str")
                lines.append(f'  ${{bronze.{kind}("{spec}"{own})}} AS {alias},')
            renamed.append(name)
        elif name in from_other:
            kind = SQL_TYPES.get(sql_type, "str")
            lines.append(f'  ${{bronze.{kind}("{name}", "o.payload")}} AS {alias},')
            filled.append(name)
        elif name in keys or name in renames:
            source = name if name in keys else renames[name]
            kind = SQL_TYPES.get(sql_type, "str")
            lines.append(f'  ${{bronze.{kind}("{source}"{own})}} AS {alias},')
            (filled if name in keys else renamed).append(name)
        else:
            lines.append(f"  CAST(NULL AS {sql_type}) AS {alias},  -- v2 does not fetch this field")
            nulled.append(name)
    known = {name for name, _ in columns} | set(renames.values())
    added = [
        k for k in keys if k not in known and not k.startswith("_") and k not in ("tenant_id",)
    ]
    # `_scan_period` is the period a row was fetched under. For the period-scanned endpoints it is
    # part of the staging key (a document that has lines in several periods is several staging
    # rows), so bronze keeps it as `scanPeriod`. The other underscore fields are lineage.
    period = "_scan_period" in keys and "scanPeriod" not in known
    if added or period:
        lines.append("")
        lines.append("  -- Fields v2 fetches that the v1 table never had")
        for k in added:
            kind = SQL_TYPES[infer(keys[k]["types"])]
            lines.append(f'  ${{bronze.{kind}("{k}"{own})}} AS `{k}`,')
        if period:
            lines.append(f'  ${{bronze.str("_scan_period"{own})}} AS `scanPeriod`,')
    notes[table] = {
        "filled": len(filled),
        "null": nulled,
        "renamed": renamed,
        "added": added + (["scanPeriod"] if period else []),
    }
    lines[-1] = lines[-1].rstrip(",").split("  --")[0].rstrip().rstrip(",") + (
        "  --" + lines[-1].split("  --", 1)[1] if "  --" in lines[-1] else ""
    )
    return "\n".join(lines)


def config_block(table: str, description: str) -> str:
    text = (
        description
        or f"Bronze: {table.removeprefix('visma_')} from Visma, typed from dw_staging_visma."
    )
    return (
        "config {\n"
        '  type: "table",\n'
        f'  schema: "{BRONZE}",\n'
        f'  name: "{table}",\n'
        f'  description: "{text}",\n'
        '  tags: ["bronze", "visma"]\n'
        "}\n"
    )


def tenants_file() -> str:
    rows = []
    for path in sorted(TENANTS.glob("*.json")):
        tenant = json.loads(path.read_text())

        def lit(value):
            return "CAST(NULL AS STRING)" if value in (None, "") else json.dumps(str(value))

        rows.append(
            "  STRUCT("
            + ", ".join(
                f"{lit(tenant.get(key))} AS {key}"
                for key in (
                    "tenant_id",
                    "tenant_name",
                    "status",
                    "valid_to",
                    "bu_label",
                    "datamart_company_id",
                )
            )
            + ")"
        )
    return (
        "config {\n"
        '  type: "table",\n'
        f'  schema: "{BRONZE}",\n'
        '  name: "visma_tenants",\n'
        '  description: "The Visma tenants (companies) the pipeline fetches, from config/tenants/ of visma_service_v2. '
        'Gives every bronze row its organizationName.",\n'
        '  tags: ["bronze", "visma"]\n'
        "}\n\n"
        "SELECT * FROM UNNEST([\n" + ",\n".join(rows) + "\n])\n"
    )


def sources_file(endpoints: list[str]) -> str:
    names = ",\n".join(f'  "{e}"' for e in endpoints)
    return (
        "// The raw staging tables the v2 pipeline (vsv2) writes: one table per endpoint, the latest\n"
        "// version of every record as JSON. Generated by tools/generate_bronze.py from the profile.\n"
        f"const endpoints = [\n{names},\n];\n\n"
        f'endpoints.forEach((name) => declare({{schema: "{STAGING}", name}}));\n'
    )


def main() -> int:
    schema = json.loads((CONTRACT / "v1_bronze_schema.json").read_text())
    descriptions = json.loads((CONTRACT / "v1_descriptions.json").read_text())
    profile = json.loads((CONTRACT / "staging_profile.json").read_text())
    out_dir = ROOT / "definitions" / "bronze" / "visma"
    out_dir.mkdir(parents=True, exist_ok=True)
    notes: dict[str, dict] = {}
    skipped: dict[str, str] = {}
    written = 0
    for table, columns in sorted(schema.items()):
        if any(j in table for j in JUNK):
            continue
        if table in NOT_GENERATED:
            skipped[table] = NOT_GENERATED[table]
            continue
        endpoint = endpoint_of(table)
        if endpoint not in profile:
            skipped[table] = f"no staging endpoint '{endpoint}'"
            continue
        body = render(table, endpoint, top_level(columns), profile[endpoint], notes)
        join = ""
        if table in OVERRIDES:
            other, condition = OVERRIDES[table]["join"]
            join = (
                f'LEFT JOIN (SELECT * FROM ${{ref("{STAGING}", "{other}")}} WHERE deleted_at IS NULL) AS o\n'
                f"  ON {condition}\n"
            )
        sql = (
            config_block(table, descriptions.get(table, ""))
            + "\nSELECT\n"
            + body
            + f'\nFROM ${{ref("{STAGING}", "{endpoint}")}} AS s\n'
            + join
            + 'LEFT JOIN ${ref("visma_tenants")} AS tenant ON tenant.tenant_id = s.tenant_id\n'
        )
        (out_dir / f"{table}.sqlx").write_text(sql)
        written += 1
    (out_dir / "visma_tenants.sqlx").write_text(tenants_file())
    (ROOT / "definitions" / "sources").mkdir(parents=True, exist_ok=True)
    (ROOT / "definitions" / "sources" / "staging_visma.js").write_text(
        sources_file(sorted(profile))
    )

    report = ["# Bronze generation report", "", f"{written} tables written.", ""]
    report += [
        "| Table | filled | NULL (v2 lacks) | renamed | added by v2 |",
        "|---|---|---|---|---|",
    ]
    for table, n in sorted(notes.items()):
        report.append(
            f"| `{table}` | {n['filled']} | {len(n['null'])} {', '.join(n['null'][:5])} "
            f"| {', '.join(n['renamed'])} | {len(n['added'])} |"
        )
    report += ["", "## Not generated", ""] + [
        f"* `{t}`: {why}" for t, why in sorted(skipped.items())
    ]
    (CONTRACT / "bronze_report.md").write_text("\n".join(report) + "\n")
    print(f"{written} bronze tables, {len(skipped)} skipped -> {out_dir}")
    for t, why in sorted(skipped.items()):
        print(f"  skipped {t}: {why}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
