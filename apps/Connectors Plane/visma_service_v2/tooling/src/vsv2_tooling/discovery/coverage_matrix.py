"""Bronze ↔ DataMart coverage matrix — Phase 0 deliverable.

Reads:
    - v1 Dataform bronze SQLX directory (one row per `visma_*.sqlx`)
    - DataMart introspection JSON (`core/queries/_generated/datamart.introspection.json`)

Writes a Markdown table to `out_path`. Each row scores parity:
    - `full`    — DM type exists, bulk readable, has `updatedInDataMartDateTime`
    - `partial` — DM type exists but missing watermark / fields
    - `none`    — no DM equivalent

`recommendation` ∈ { migrate, hold, skip }.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from pathlib import Path

from rich import print as rprint

# Hand-curated bronze_table → likely DM type-name mapping.
# Names are a starting hint; coverage step verifies each against the live SDL.
_BRONZE_TO_DM: dict[str, str] = {
    "visma_accounts": "Account",
    "visma_branches": "Branch",
    "visma_cash_accounts": "CashAccount",
    "visma_currencies": "Currency",
    "visma_currency_rates": "CurrencyRate",
    "visma_customer_classes": "CustomerClass",
    "visma_customer_documents": "CustomerDocument",
    "visma_customer_locations": "Location",
    "visma_customer_payments": "CustomerPayment",
    "visma_customers": "Customer",
    "visma_departments": "Department",
    "visma_dimensions": "Dimension",
    "visma_employees": "Employee",
    "visma_financial_periods": "FinancialPeriod",
    "visma_general_ledger_balances_v2": "GeneralLedgerBalance",
    "visma_general_ledger_transactions": "GeneralLedgerTransaction",
    "visma_inventory_items": "InventoryItem",
    "visma_journal_transactions": "JournalTransaction",
    "visma_ledgers": "Ledger",
    "visma_locations": "Location",
    "visma_organizations": "Organization",
    "visma_payment_methods": "PaymentMethod",
    "visma_projects": "Project",
    "visma_sales_orders_v3": "SalesOrder",
    "visma_subaccounts": "Subaccount",
    "visma_supplier_documents": "SupplierDocument",
    "visma_supplier_payments": "SupplierPayment",
    "visma_suppliers": "Supplier",
    "visma_vat_categories": "VatCategory",
    "visma_vat_zones": "VatZone",
    "visma_vats": "Vat",
    "visma_warehouses": "Warehouse",
}

_INTROSPECTION_PATH = Path("core/queries/_generated/datamart.introspection.json")


@dataclass
class Row:
    bronze_table: str
    dm_type: str | None
    has_cursor: bool
    has_id: bool
    field_parity: str
    recommendation: str


def run(bronze_dir: str, out_path: str) -> None:
    bronze_path = Path(bronze_dir)
    if not bronze_path.is_dir():
        raise SystemExit(f"bronze dir not found: {bronze_path}")

    bronze_tables = sorted(p.stem for p in bronze_path.glob("visma_*.sqlx"))
    rprint(f"[cyan]→[/cyan] {len(bronze_tables)} bronze tables")

    dm_index = _load_introspection_index()
    rprint(
        f"[cyan]→[/cyan] {len(dm_index)} DataMart types"
        if dm_index
        else "[yellow]·[/yellow] no DataMart introspection JSON yet "
        "— field parity column will be blank"
    )

    rows: list[Row] = []
    for tbl in bronze_tables:
        dm_name = _BRONZE_TO_DM.get(tbl)
        dm_type = dm_index.get(dm_name) if (dm_name and dm_index) else None
        if dm_type:
            field_set = {f.get("name") for f in (dm_type.get("fields") or [])}
            has_cursor = "updatedInDataMartDateTime" in field_set
            has_id = "id" in field_set
            parity = "full" if (has_cursor and has_id) else "partial"
        elif dm_name and dm_index:
            has_cursor = has_id = False
            parity = "none"
        else:
            has_cursor = has_id = False
            parity = "" if dm_index else "?"

        recommendation = _recommend(tbl, parity)
        rows.append(
            Row(
                bronze_table=tbl,
                dm_type=dm_name,
                has_cursor=has_cursor,
                has_id=has_id,
                field_parity=parity,
                recommendation=recommendation,
            )
        )

    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(_render(rows))
    rprint(f"[green]✓[/green] coverage matrix → {out}")


def _load_introspection_index() -> dict[str, dict]:
    if not _INTROSPECTION_PATH.exists():
        return {}
    raw = json.loads(_INTROSPECTION_PATH.read_text())
    schema = raw.get("__schema") or raw.get("data", {}).get("__schema") or {}
    return {t["name"]: t for t in schema.get("types") or [] if t.get("name")}


# Hourly-Tier-1 endpoints stay on REST per locked decision.
_HOURLY_REST_ONLY = {
    "visma_sales_orders_v3",
    "visma_shipments",
    "visma_customer_payments",
    "visma_supplier_payments",
    "visma_customer_documents",
    "visma_supplier_documents",
}


def _recommend(table: str, parity: str) -> str:
    if table in _HOURLY_REST_ONLY:
        return "hold"
    if parity == "full":
        return "migrate"
    if parity == "partial":
        return "hold"
    if parity == "none":
        return "skip"
    return ""


def _render(rows: list[Row]) -> str:
    n_full = sum(1 for r in rows if r.field_parity == "full")
    n_partial = sum(1 for r in rows if r.field_parity == "partial")
    n_none = sum(1 for r in rows if r.field_parity == "none")
    n_total = len(rows)

    lines = [
        "# Phase 0 — Bronze ↔ DataMart coverage matrix",
        "",
        f"- bronze tables: {n_total}",
        f"- full parity:   {n_full}",
        f"- partial:       {n_partial}",
        f"- none:          {n_none}",
        "",
        "| bronze_table | dm_type | id | cursor | parity | recommend |",
        "|---|---|:-:|:-:|---|---|",
    ]
    for r in rows:
        lines.append(
            f"| `{r.bronze_table}` "
            f"| {r.dm_type or ''} "
            f"| {'✓' if r.has_id else ''} "
            f"| {'✓' if r.has_cursor else ''} "
            f"| {r.field_parity} "
            f"| {r.recommendation} |"
        )
    lines.append("")
    if n_total and (n_partial + n_none) / max(n_total, 1) > 0.4:
        lines.append(
            "> **Gate:** > 40% partial-or-none — "
            "reduce migration scope to GL + customers/locations + accounts."
        )
    return "\n".join(lines)


# Re-exported for symmetry with introspect/probe modules.
_RE_VISMA = re.compile(r"^visma_")
