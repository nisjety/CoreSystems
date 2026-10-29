"""
Full-history sync orchestrator: Moment API -> BigQuery.

For each configured entity this fetches every page from the Moment API (the
bare/no-include representation where that's richer - see bigquery_sync.py's
module docstring for why) and does a full-refresh load into BigQuery, so
re-running a sync is always idempotent and reflects the complete current
state of that entity in Moment.
"""

import logging
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)

PAGE_SIZE = 500

# Each entry: entity key -> (fetch function, BigQuerySync method name)
# fetch functions take the MomentAPIClient and return the full list of items.
ENTITY_FETCHERS = {
    "projects": lambda client: client.get_all_pages("projects", page_size=PAGE_SIZE),
    "customers": lambda client: client.get_all_pages("customers", page_size=PAGE_SIZE),
    "user_accounts": lambda client: client.get_all_pages("userAccounts", page_size=PAGE_SIZE),
    # timeRecords is the one endpoint where the default `id,asc` is unusable:
    # the server cancels that page-0 query at ~30s with a 400
    # QueryTimeoutException (at every page size), while `createdDate,asc`
    # answers in about a second. Leading with it skips a probe that always
    # fails; `id,asc` stays in the chain as the fallback.
    "time_records": lambda client: client.get_all_pages(
        "timeRecords", page_size=PAGE_SIZE, sort_candidates=("createdDate,asc", "id,asc")
    ),
    "tags": lambda client: client.get_all_pages("tags", page_size=PAGE_SIZE),
    "invoices": lambda client: client.get_all_pages("invoices", page_size=PAGE_SIZE),
    "offers": lambda client: client.get_all_pages("offers", page_size=PAGE_SIZE),
    "invoicing_currency_setups": lambda client: client.get_all_pages("invoicingCurrencySetups", page_size=PAGE_SIZE),
    "project_membership_roles": lambda client: client.get_all_pages("projectMembershipRoles", page_size=PAGE_SIZE),
    "expense_categories": lambda client: client.get_all_pages("expenseCategories", page_size=PAGE_SIZE),
    "custom_fields": lambda client: client.get_all_pages("customFields", page_size=PAGE_SIZE),
    "departments": lambda client: client.get_all_pages("departments", page_size=PAGE_SIZE),
    "absences": lambda client: client.get_all_pages("absences", page_size=PAGE_SIZE),
}

ENTITY_SYNC_METHODS = {
    "projects": "sync_projects",
    "customers": "sync_customers",
    "user_accounts": "sync_user_accounts",
    "time_records": "sync_time_records",
    "tags": "sync_tags",
    "invoices": "sync_invoices",
    "offers": "sync_offers",
    "invoicing_currency_setups": "sync_invoicing_currency_setups",
    "project_membership_roles": "sync_project_membership_roles",
    "expense_categories": "sync_expense_categories",
    "custom_fields": "sync_custom_fields",
    "departments": "sync_departments",
    "absences": "sync_absences",
}

ALL_ENTITIES = list(ENTITY_FETCHERS.keys())


def sync_entity(moment_client, bigquery_sync, entity: str) -> Dict[str, Any]:
    """Fetch every page of one entity from Moment and load it into BigQuery."""
    fetcher = ENTITY_FETCHERS.get(entity)
    sync_method_name = ENTITY_SYNC_METHODS.get(entity)
    if not fetcher or not sync_method_name:
        return {"success": False, "entity": entity, "error": f"Unknown entity: {entity}"}

    try:
        items = fetcher(moment_client)
    except Exception as e:
        logger.error(f"Failed to fetch {entity} from Moment: {e}")
        return {"success": False, "entity": entity, "error": f"fetch failed: {e}"}

    sync_method = getattr(bigquery_sync, sync_method_name)
    result = sync_method(items)
    return {"entity": entity, "fetched": len(items), **result}


def sync_all(moment_client, bigquery_sync, entities: Optional[List[str]] = None) -> Dict[str, Any]:
    """Sync every entity in `entities` (default: all configured entities)."""
    targets = entities or ALL_ENTITIES
    unknown = [e for e in targets if e not in ENTITY_FETCHERS]
    if unknown:
        return {"success": False, "error": f"Unknown entities: {unknown}", "valid_entities": ALL_ENTITIES}

    results = {}
    for entity in targets:
        logger.info(f"Syncing Moment entity: {entity}")
        results[entity] = sync_entity(moment_client, bigquery_sync, entity)

    overall_success = all(r.get("success", False) for r in results.values())
    return {
        "success": overall_success,
        "entities_synced": len(targets),
        "total_records_fetched": sum(r.get("fetched", 0) for r in results.values()),
        "results": results,
    }
