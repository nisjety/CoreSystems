"""`visma_price_classes`: the unique price classes in use, with their descriptions.

The Visma APIs have no price class list. v1 builds one from two places, and this reproduces its
rule (apps/visma_service/clients/tier1_operational.py `get_price_classes`):

1. every `priceClassId` on a customer location (Sales Order API
   `/Customers/{customerId}/locations`) gives a class with an empty description;
2. every `priceClass` object `{id, description}` on an ERP `/v1/customer` row adds the class or,
   when the class is known but has no description yet, supplies one.

A description that is already set is never replaced. Rows without a usable text id are skipped.
"""

from __future__ import annotations

from collections.abc import Iterable
from typing import Any


def _text(value: Any) -> str | None:
    return value if isinstance(value, str) and value else None


def price_classes(
    customer_locations: Iterable[dict[str, Any]], customers: Iterable[dict[str, Any]]
) -> list[dict[str, str]]:
    """Unique price classes, in the order they were first seen."""
    by_id: dict[str, dict[str, str]] = {}

    for location in customer_locations:
        if class_id := _text(location.get("priceClassId")):
            by_id.setdefault(class_id, {"id": class_id, "description": ""})

    for customer in customers:
        price_class = customer.get("priceClass")
        if not isinstance(price_class, dict):
            continue
        class_id = _text(price_class.get("id"))
        if not class_id:
            continue
        description = price_class.get("description")
        description = description if isinstance(description, str) else ""
        existing = by_id.get(class_id)
        if existing is None or (not existing["description"] and description):
            by_id[class_id] = {"id": class_id, "description": description}

    return list(by_id.values())
