"""Stage budget details and their per-account, per-period balance snapshots."""

import json
import logging
from datetime import datetime, timezone

from etl.common.date_parser import (
    parse_iso_date,
    parse_iso_datetime,
    parse_xero_date,
    parse_xero_datetime,
)

logger = logging.getLogger(__name__)

BRONZE_TABLE = "xero_budgets"
HEADER_TABLE = "budgets"
BALANCE_TABLE = "budget_balance_snapshots"


def _updated_at(value):
    if not isinstance(value, str):
        return None
    parsed = parse_xero_datetime(value) or parse_iso_datetime(value)
    if parsed is None:
        return None
    return parsed.replace(tzinfo=timezone.utc) if parsed.tzinfo is None else parsed.astimezone(timezone.utc)


def _period_date(balance):
    for value in (balance.get("PeriodDate"), balance.get("Period")):
        if not isinstance(value, str):
            continue
        parsed = parse_xero_date(value) or parse_iso_date(value)
        if parsed is not None:
            return parsed
        try:
            return datetime.strptime(value, "%Y-%m").date()
        except ValueError:
            continue
    return None


def parse_header(record: dict) -> dict:
    payload = record["payload"]
    return {
        "tenant_id": record["tenant_id"],
        "record_id": record["record_id"],
        "synced_at": record["last_seen_at"],
        "first_seen_at": record["first_seen_at"],
        "budget_id": payload.get("BudgetID"),
        "description": payload.get("Description"),
        "budget_type": payload.get("Type"),
        "status": payload.get("Status"),
        "updated_at": _updated_at(payload.get("UpdatedDateUTC")),
        "tracking_json": json.dumps(payload.get("Tracking") or []),
        "budget_line_count": len(payload.get("BudgetLines") or []),
    }


def parse_balances(record: dict) -> list[dict]:
    rows = []
    for line in record["payload"].get("BudgetLines") or []:
        account_id = line.get("AccountID")
        account_code = line.get("AccountCode")
        account_key = account_id or account_code
        if not account_key:
            logger.warning("budget %s has a line without account ID/code", record["record_id"])
            continue
        for balance in line.get("BudgetBalances") or []:
            period = balance.get("Period")
            if not period:
                logger.warning("budget %s has a balance without period", record["record_id"])
                continue
            rows.append({
                "tenant_id": record["tenant_id"],
                "record_id": record["record_id"],
                "synced_at": record["last_seen_at"],
                "budget_id": record["payload"].get("BudgetID"),
                "account_key": account_key,
                "account_id": account_id,
                "account_code": account_code,
                "period": period,
                "period_date_local": _period_date(balance),
                "amount": str(balance["Amount"]) if balance.get("Amount") is not None else None,
                "unit_amount": str(balance["UnitAmount"]) if balance.get("UnitAmount") is not None else None,
                "notes": balance.get("Notes"),
            })
    return rows


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    headers, balances = [], []
    for record in reader.iter_records(BRONZE_TABLE, tenant_id=tenant_id, limit=limit):
        headers.append(parse_header(record))
        balances.extend(parse_balances(record))
    if headers:
        writer.merge(HEADER_TABLE, headers)
    if balances:
        writer.merge(
            BALANCE_TABLE, balances,
            key_columns=("tenant_id", "record_id", "account_key", "period", "synced_at"),
        )
    return {"headers": len(headers), "balances": len(balances)}
