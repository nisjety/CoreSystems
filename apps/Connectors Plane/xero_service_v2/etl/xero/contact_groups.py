"""Stage contact group details and per-member snapshots."""

import json

HEADER_TABLE = "contact_groups"
MEMBER_TABLE = "contact_group_member_snapshots"
BRONZE_TABLE = "xero_contact_groups"


def parse_header(record: dict) -> dict:
    payload = record["payload"]
    contacts = payload.get("Contacts") or []
    return {
        "tenant_id": record["tenant_id"],
        "record_id": record["record_id"],
        "synced_at": record["last_seen_at"],
        "first_seen_at": record["first_seen_at"],
        "contact_group_id": payload.get("ContactGroupID"),
        "name": payload.get("Name"),
        "status": payload.get("Status"),
        "member_count": len(contacts),
        "contacts_json": json.dumps(contacts),
    }


def parse_members(record: dict) -> list[dict]:
    rows = []
    for contact in record["payload"].get("Contacts") or []:
        contact_id = contact.get("ContactID")
        if not contact_id:
            continue
        rows.append({
            "tenant_id": record["tenant_id"],
            "record_id": record["record_id"],
            "synced_at": record["last_seen_at"],
            "contact_group_id": record["payload"].get("ContactGroupID"),
            "contact_id": contact_id,
            "contact_name": contact.get("Name"),
        })
    return rows


def run(reader, writer, tenant_id=None, limit=None) -> dict:
    headers, members = [], []
    for record in reader.iter_records(BRONZE_TABLE, tenant_id=tenant_id, limit=limit):
        headers.append(parse_header(record))
        members.extend(parse_members(record))
    if headers:
        writer.merge(HEADER_TABLE, headers)
    if members:
        writer.merge(
            MEMBER_TABLE, members,
            key_columns=("tenant_id", "record_id", "contact_id", "synced_at"),
        )
    return {"headers": len(headers), "members": len(members)}
