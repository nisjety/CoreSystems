"""DataMart record → bronze legacy shape normalizers.

Each normalizer is registered by name (e.g. `normalize_customer_with_locations`)
in [`REGISTRY`]. Endpoint configs reference these names via
`datamart.join_normalizer`.

Bronze contract is byte-stable during pilot — every legacy field stays,
DM-only fields stay null until Phase 8b.
"""

from __future__ import annotations

from collections.abc import Callable

from . import (
    _shared,
    account,
    branch,
    budget,
    contact,
    credit_term,
    customer,
    customer_document,
    customer_payment,
    customer_sales_price,
    general_ledger_balance,
    journal_transaction,
    location,
    project_task,
    sales_order,
    sales_person,
    subaccount,
    supplier,
    supplier_payment,
    vat_category,
    vat_zone,
    warehouse,
)

Normalizer = Callable[..., dict]

REGISTRY: dict[str, Normalizer] = {
    "normalize_customer_with_locations": customer.normalize_customer_with_locations,
    "normalize_location": location.normalize_location,
    "normalize_account": account.normalize_account,
    "normalize_budget": budget.normalize_budget,
    "normalize_branch": branch.normalize_branch,
    "normalize_contact": contact.normalize_contact,
    "normalize_credit_term": credit_term.normalize_credit_term,
    "normalize_customer_document": customer_document.normalize_customer_document,
    "normalize_customer_payment": customer_payment.normalize_customer_payment,
    "normalize_customer_sales_price": customer_sales_price.normalize_customer_sales_price,
    "normalize_general_ledger_balance": general_ledger_balance.normalize_general_ledger_balance,
    "normalize_journal_transaction": journal_transaction.normalize_journal_transaction,
    "normalize_sales_order": sales_order.normalize_sales_order,
    "normalize_sales_person": sales_person.normalize_sales_person,
    "normalize_project_task": project_task.normalize_project_task,
    "normalize_subaccount": subaccount.normalize_subaccount,
    "normalize_supplier": supplier.normalize_supplier,
    "normalize_supplier_payment": supplier_payment.normalize_supplier_payment,
    "normalize_vat_category": vat_category.normalize_vat_category,
    "normalize_vat_zone": vat_zone.normalize_vat_zone,
    "normalize_warehouse": warehouse.normalize_warehouse,
}

__all__ = ["REGISTRY", "Normalizer", "_shared"]
