#!/usr/bin/env python3
"""Provision a private, OAuth-only MCP runtime env from an existing BFF env."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

if __package__:
    from .provision_custom_env import ASSIGNMENT, ProvisionError, _parse_value, _read_source, _stage
else:
    from provision_custom_env import ASSIGNMENT, ProvisionError, _parse_value, _read_source, _stage


SOURCE_KEYS = ("XERO_PKCE_CLIENT_ID", "XERO_PKCE_CLIENT_SECRET")
OAUTH_SCOPES = (
    "openid profile email offline_access "
    "accounting.transactions.read accounting.contacts.read accounting.settings.read "
    "accounting.reports.read accounting.journals.read accounting.attachments.read "
    "accounting.budgets.read"
)
WRITE_SCOPES = (
    "accounting.transactions accounting.contacts accounting.invoices accounting.payments "
    "accounting.banktransactions accounting.manualjournals accounting.settings "
    "payroll.timesheets"
)
WRITE_ACTIONS = frozenset({
    "create_contact", "create_draft_invoice", "create_invoice", "create_payment",
    "create_bank_transaction", "create_credit_note", "create_item",
    "create_manual_journal", "create_quote", "create_payroll_timesheet",
    "create_tracking_category", "create_tracking_options", "update_contact",
    "update_invoice", "update_bank_transaction", "update_credit_note", "update_item",
    "update_manual_journal", "update_quote", "update_tracking_category",
    "update_tracking_options", "approve_payroll_timesheet",
    "add_payroll_timesheet_line", "update_payroll_timesheet_line",
    "revert_payroll_timesheet", "delete_payroll_timesheet",
})
WAREHOUSE_PROJECT = "aquatiq-datavarehus"
WAREHOUSE_DATASET = "dw_staging_xero"
WAREHOUSE_LOCATION = "europe-north2"
RAW_BUCKET = "aquatiq-datavarehus-xero-raw"


def _credentials(source: Path) -> dict[str, str]:
    if not hasattr(os, "O_NOFOLLOW"):
        raise ProvisionError("safe source opening is unavailable on this platform")
    if source.is_symlink():
        raise ProvisionError("source must not be a symlink")
    values: dict[str, str] = {}
    for line in _read_source(source).splitlines():
        match = ASSIGNMENT.fullmatch(line)
        if match is None or match.group(1) not in SOURCE_KEYS:
            continue
        key = match.group(1)
        if key in values:
            raise ProvisionError(f"{key} is duplicated")
        values[key] = _parse_value(match.group(2), key)
    missing = set(SOURCE_KEYS) - values.keys()
    if missing:
        raise ProvisionError(f"missing required values: {', '.join(sorted(missing))}")
    return values


def _runtime_env(
    values: dict[str, str],
    *,
    allow_extract: bool = False,
    allow_warehouse: bool = False,
    retain_results: bool = False,
    write_actions: tuple[str, ...] = (),
) -> bytes:
    if (allow_extract or write_actions) and not retain_results:
        raise ProvisionError("--allow-extract and --allow-write require --retain-results")
    if len(set(write_actions)) != len(write_actions) or not set(write_actions) <= WRITE_ACTIONS:
        raise ProvisionError("write actions must be unique supported Xero MCP actions")
    scopes = OAUTH_SCOPES if not write_actions else f"{OAUTH_SCOPES} {WRITE_SCOPES}"
    settings = {
        "XERO_AUTH_MODE": "pkce",
        "XERO_PKCE_CLIENT_ID": values["XERO_PKCE_CLIENT_ID"],
        "XERO_PKCE_CLIENT_SECRET": values["XERO_PKCE_CLIENT_SECRET"],
        "XERO_MCP_OAUTH_ISSUER": "https://tools.aquatiq.com",
        "XERO_MCP_OAUTH_CLIENT_ID": "claude-xero",
        "XERO_MCP_OAUTH_XERO_REDIRECT_URI": "https://tools.aquatiq.com/oauth/xero/callback",
        "XERO_MCP_OAUTH_SCOPES": f'"{scopes}"',
        "XERO_CONNECTION_STORE_PATH": "/app/out/mcp/connections.json",
        "XERO_MCP_STORAGE_ROOT": "/app/out/mcp",
        "XERO_MCP_BIND": "0.0.0.0:5003",
        "XERO_MCP_ALLOWED_HOSTS": "tools.aquatiq.com,localhost,127.0.0.1",
        "XERO_MCP_TLS_PROXY": "true",
        "XERO_MCP_TOOL_MODE": "compact",
        "XERO_MCP_OAUTH_ZERO_DATA_RETENTION": str(not retain_results).lower(),
        "XERO_MCP_OAUTH_ALLOW_EXTRACT": str(allow_extract).lower(),
        "XERO_MCP_OAUTH_ALLOW_WAREHOUSE": str(allow_warehouse).lower(),
        "XERO_MCP_OAUTH_WRITE_ACTIONS": ",".join(write_actions),
        "XERO_MCP_WAREHOUSE_PROJECT": WAREHOUSE_PROJECT,
        "XERO_MCP_WAREHOUSE_DATASET": WAREHOUSE_DATASET,
        "XERO_MCP_WAREHOUSE_LOCATION": WAREHOUSE_LOCATION,
        "XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED": "536870912",
        "GCP_PROJECT": WAREHOUSE_PROJECT,
        "GCS_BUCKET": RAW_BUCKET,
        "GCS_PREFIX": "raw/xero",
        "GOOGLE_APPLICATION_CREDENTIALS": "/run/secrets/gcp-mcp.json",
        "XERO_MCP_ENABLE_WRITES": str(bool(write_actions)).lower(),
    }
    return "".join(f"{key}={value}\n" for key, value in settings.items()).encode("ascii")


def provision(
    source: Path,
    runtime_env: Path,
    *,
    runtime_uid: int = 10001,
    runtime_gid: int = 10001,
    allow_extract: bool = False,
    allow_warehouse: bool = False,
    retain_results: bool = False,
    write_actions: tuple[str, ...] = (),
) -> None:
    """Create an owner-only runtime env without replacing an existing output."""
    source, runtime_env = Path(source), Path(runtime_env)
    if min(runtime_uid, runtime_gid) < 0:
        raise ProvisionError("UID and GID values must be non-negative")
    if source.is_symlink():
        raise ProvisionError("source must not be a symlink")
    if source.resolve() == runtime_env.resolve():
        raise ProvisionError("source and output must be distinct paths")
    if os.path.lexists(runtime_env):
        raise FileExistsError("output already exists")
    if not runtime_env.parent.is_dir():
        raise ProvisionError("output parent directory does not exist")
    if runtime_env.parent.stat().st_mode & 0o022:
        raise ProvisionError("output parent directory must not be group or world writable")

    values = _credentials(source)
    temporary = _stage(
        runtime_env,
        _runtime_env(
            values,
            allow_extract=allow_extract,
            allow_warehouse=allow_warehouse,
            retain_results=retain_results,
            write_actions=write_actions,
        ),
        runtime_uid,
        runtime_gid,
    )
    try:
        os.link(temporary, runtime_env)
    finally:
        temporary.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-env", type=Path, default=Path(".env.bff"))
    parser.add_argument("--output-env", required=True, type=Path)
    parser.add_argument("--uid", type=int, default=10001, help="container runtime file UID")
    parser.add_argument("--gid", type=int, default=10001, help="container runtime file GID")
    parser.add_argument(
        "--allow-extract",
        action="store_true",
        help="allow tenant-scoped raw extraction for OAuth-issued MCP tokens",
    )
    parser.add_argument(
        "--allow-warehouse",
        action="store_true",
        help="allow tenant-scoped warehouse summaries for OAuth-issued MCP tokens",
    )
    parser.add_argument(
        "--retain-results",
        action="store_true",
        help="allow private ten-minute MCP result handles and durable extraction receipts",
    )
    parser.add_argument(
        "--allow-write",
        action="append",
        default=[],
        metavar="ACTION",
        help="grant one approval-gated Xero write action to OAuth-issued MCP tokens; repeat as needed",
    )
    args = parser.parse_args(argv)
    try:
        provision(
            args.source_env,
            args.output_env,
            runtime_uid=args.uid,
            runtime_gid=args.gid,
            allow_extract=args.allow_extract,
            allow_warehouse=args.allow_warehouse,
            retain_results=args.retain_results,
            write_actions=tuple(args.allow_write),
        )
    except FileExistsError:
        print("Provisioning failed: output already exists", file=sys.stderr)
        return 1
    except ProvisionError as exc:
        print(f"Provisioning failed: {exc}", file=sys.stderr)
        return 1
    except OSError as exc:
        print(f"Provisioning failed: {exc.strerror or type(exc).__name__}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
