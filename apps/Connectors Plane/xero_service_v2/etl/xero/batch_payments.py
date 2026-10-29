"""Stage Xero batch-payment headers and their per-payment snapshots."""

from etl.common.date_parser import parse_iso_date, parse_xero_datetime

BRONZE_TABLE = "xero_batch_payments"
HEADER_TABLE = "batch_payments"
LINE_TABLE = "batch_payment_line_snapshots"


def _object(value):
    return value if isinstance(value, dict) else {}


def _payments(payload):
    payments = payload.get("Payments") or []
    if not isinstance(payments, list):
        raise ValueError("BatchPayment.Payments must be an array")
    return payments


def parse_header(record: dict) -> dict:
    payload = record["payload"]
    account = _object(payload.get("Account"))
    return {
        "tenant_id": record["tenant_id"],
        "record_id": record["record_id"],
        "synced_at": record["last_seen_at"],
        "first_seen_at": record["first_seen_at"],
        "batch_payment_id": payload.get("BatchPaymentID"),
        "bank_account_id": account.get("AccountID"),
        "account_currency_code": account.get("CurrencyCode"),
        "batch_payment_type": payload.get("Type"),
        "status": payload.get("Status"),
        "payment_date": parse_xero_datetime(payload.get("Date")),
        "payment_date_local": parse_iso_date(payload.get("DateString")),
        "updated_at": parse_xero_datetime(payload.get("UpdatedDateUTC")),
        "reference": str(payload["Reference"]) if payload.get("Reference") is not None else None,
        "particulars": payload.get("Particulars"),
        "code": payload.get("Code"),
        "details": payload.get("Details"),
        "narrative": payload.get("Narrative"),
        "reconciled_statement_line_id": payload.get("ReconciledStatementLineId"),
        "total_amount": payload.get("TotalAmount"),
        "payment_count": len(_payments(payload)),
        "is_reconciled": payload.get("IsReconciled"),
    }


def parse_lines(record: dict) -> list[dict]:
    payload = record["payload"]
    account = _object(payload.get("Account"))
    rows = []
    for index, payment in enumerate(_payments(payload)):
        if not isinstance(payment, dict):
            raise ValueError("BatchPayment.Payments entries must be objects")
        invoice = _object(payment.get("Invoice"))
        rows.append({
            "tenant_id": record["tenant_id"],
            "record_id": record["record_id"],
            "synced_at": record["last_seen_at"],
            "batch_payment_id": payload.get("BatchPaymentID"),
            "line_index": index,
            "payment_id": payment.get("PaymentID"),
            "invoice_id": invoice.get("InvoiceID"),
            "invoice_currency_code": invoice.get("CurrencyCode"),
            "account_currency_code": account.get("CurrencyCode"),
            "amount": payment.get("Amount"),
            "bank_amount": payment.get("BankAmount"),
            "details": payment.get("Details"),
            "invoice_has_errors": invoice.get("HasErrors"),
            "invoice_is_discounted": invoice.get("IsDiscounted"),
        })
    return rows


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    headers, lines = [], []
    for record in reader.iter_records(BRONZE_TABLE, tenant_id=tenant_id, limit=limit):
        headers.append(parse_header(record))
        lines.extend(parse_lines(record))
    if headers:
        writer.merge(HEADER_TABLE, headers)
    if lines:
        writer.merge(
            LINE_TABLE, lines,
            key_columns=("tenant_id", "record_id", "line_index", "synced_at"),
        )
    return {"headers": len(headers), "lines": len(lines)}
