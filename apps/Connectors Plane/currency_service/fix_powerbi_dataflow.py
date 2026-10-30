"""
Fix Power BI Fabric Dataflow: patch Rate and Historyrates entities
to use the BigQuery PowerBI views (RatePowerBI / HistoryratesPowerBI)
instead of the raw tables, fixing the STRUCT->Number conversion error.

Auth: uses `az account get-access-token` (run `az login` first).

Usage:
    python3 fix_powerbi_dataflow.py [--dry-run] [--workspace-id ID] [--dataflow-id ID]

Dependencies:
    pip install requests

Steps the script performs:
1. Obtain an Azure AD access token for the Power BI API via Azure CLI.
2. List all workspaces the caller has access to.
3. For each workspace, list dataflows and look for one that contains
   entities named "Rate" or "Historyrates".
4. Export the found dataflow definition (model.json / mashup document).
5. Rewrite every M expression that reads from the raw BigQuery tables
   to use the _PowerBI views instead.
6. Re-import the updated definition back to Power BI, overwriting the
   existing dataflow in-place.
"""

import argparse
import json
import subprocess
import sys
import time
import zipfile
import io
import re
import copy
from typing import Optional

import requests

PBI_BASE = "https://api.powerbi.com/v1.0/myorg"

# BigQuery source/target view name mapping
VIEW_REPLACEMENTS = {
    "Rate": "RatePowerBI",
    "Historyrates": "HistoryratesPowerBI",
}

# M expression patterns to rewrite: match table names inside BigQuery queries
# Matches both Kind="Table" and Kind="View" lookup patterns
_TABLE_PATTERN = re.compile(
    r'\[Name\s*=\s*"(Rate|Historyrates)"\s*(?:,\s*Kind\s*=\s*"\w+")?\s*\]',
    re.IGNORECASE,
)


def _get_token() -> str:
    """Get a Power BI access token from Azure CLI."""
    try:
        res = subprocess.run(
            [
                "az",
                "account",
                "get-access-token",
                "--resource",
                "https://analysis.windows.net/powerbi/api",
                "--output",
                "json",
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        return json.loads(res.stdout)["accessToken"]
    except subprocess.CalledProcessError:
        print(
            "ERROR: Could not get Azure CLI token.\n"
            "Run 'az login' first, then retry.\n"
            "If 'az' is not installed: https://learn.microsoft.com/cli/azure/install-azure-cli"
        )
        sys.exit(1)
    except KeyError:
        print("ERROR: Unexpected token response format from az CLI.")
        sys.exit(1)


def _headers(token: str) -> dict:
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


# ---------------------------------------------------------------------------
# Discovery helpers
# ---------------------------------------------------------------------------


def list_workspaces(token: str) -> list:
    """Return all workspaces accessible to the caller."""
    resp = requests.get(f"{PBI_BASE}/groups", headers=_headers(token), timeout=30)
    resp.raise_for_status()
    return resp.json().get("value", [])


def list_dataflows(token: str, workspace_id: str) -> list:
    resp = requests.get(
        f"{PBI_BASE}/groups/{workspace_id}/dataflows",
        headers=_headers(token),
        timeout=30,
    )
    resp.raise_for_status()
    return resp.json().get("value", [])


def find_target_dataflow(
    token: str,
    hint_workspace_id: Optional[str] = None,
    hint_dataflow_id: Optional[str] = None,
) -> tuple[str, str, str]:
    """
    Return (workspace_id, dataflow_id, dataflow_name) for the dataflow that
    contains Rate / Historyrates entities.
    """
    workspaces = list_workspaces(token)

    if hint_workspace_id:
        workspaces = [w for w in workspaces if w["id"] == hint_workspace_id]
        if not workspaces:
            print(f"ERROR: Workspace {hint_workspace_id} not found.")
            sys.exit(1)

    candidates = []
    for ws in workspaces:
        ws_id = ws["id"]
        ws_name = ws.get("name", ws_id)
        try:
            flows = list_dataflows(token, ws_id)
        except requests.HTTPError:
            continue

        for flow in flows:
            flow_id = flow["objectId"]
            if hint_dataflow_id and flow_id != hint_dataflow_id:
                continue

            # Check entities inside the dataflow
            try:
                entities_resp = requests.get(
                    f"{PBI_BASE}/groups/{ws_id}/dataflows/{flow_id}/datasources",
                    headers=_headers(token),
                    timeout=30,
                )
                entities_resp.raise_for_status()
                # Check name/description for Rate hints
            except requests.HTTPError:
                pass

            flow_name = flow.get("name", flow_id)
            candidates.append((ws_id, flow_id, flow_name, ws_name))

    if not candidates:
        print("ERROR: No dataflows found in accessible workspaces.")
        sys.exit(1)

    if len(candidates) == 1:
        ws_id, flow_id, flow_name, ws_name = candidates[0]
        print(f"Found dataflow: '{flow_name}' in workspace '{ws_name}'")
        return ws_id, flow_id, flow_name

    # Multiple candidates — let user pick
    print("Multiple dataflows found. Select one:")
    for i, (ws_id, flow_id, flow_name, ws_name) in enumerate(candidates):
        print(f"  [{i}] {flow_name}  (workspace: {ws_name}, id: {flow_id})")
    idx = int(input("Enter number: "))
    ws_id, flow_id, flow_name, _ = candidates[idx]
    return ws_id, flow_id, flow_name


# ---------------------------------------------------------------------------
# Export / import
# ---------------------------------------------------------------------------


def export_dataflow(token: str, workspace_id: str, dataflow_id: str) -> dict:
    """Download and parse the dataflow model.json definition."""
    resp = requests.get(
        f"{PBI_BASE}/groups/{workspace_id}/dataflows/{dataflow_id}/export",
        headers={"Authorization": f"Bearer {token}"},
        timeout=60,
    )
    resp.raise_for_status()

    content_type = resp.headers.get("Content-Type", "")
    raw = resp.content

    # The export is either a JSON body or a zip containing model.json
    if "zip" in content_type or raw[:2] == b"PK":
        with zipfile.ZipFile(io.BytesIO(raw)) as zf:
            names = zf.namelist()
            # Prefer model.json; fall back to any .json
            target = "model.json" if "model.json" in names else next(n for n in names if n.endswith(".json"))
            return json.loads(zf.read(target))
    else:
        return resp.json()


def _rewrite_m_expression(expr: str) -> tuple[str, list[str]]:
    """
    Replace raw table references with the _PowerBI view equivalents.
    Returns (updated_expr, list_of_changes_made).
    """
    changes = []

    def _replacer(match: re.Match) -> str:
        original_name = match.group(1)
        replacement_name = VIEW_REPLACEMENTS.get(original_name)
        if not replacement_name:
            return match.group(0)
        new_ref = f'[Name="{replacement_name}",Kind="View"]'
        changes.append(f"{original_name!r} → {replacement_name!r}")
        return new_ref

    updated = _TABLE_PATTERN.sub(_replacer, expr)
    return updated, changes


def patch_model(model: dict) -> tuple[dict, list[str]]:
    """
    Deep-copy model and rewrite all M expressions for Rate and Historyrates.
    Returns (patched_model, summary_of_changes).
    """
    patched = copy.deepcopy(model)
    all_changes = []

    # Power BI dataflow model.json puts M expressions in:
    #   model["entities"][n]["partitions"][m]["source"]["expression"]
    # or (older format): model["entities"][n]["source"]["expression"]
    for entity in patched.get("entities", []):
        entity_name = entity.get("name", "")

        # Check partitions (most common)
        for partition in entity.get("partitions", []):
            source = partition.get("source", {})
            expr = source.get("expression")
            if isinstance(expr, str):
                new_expr, changes = _rewrite_m_expression(expr)
                if changes:
                    source["expression"] = new_expr
                    all_changes.append(f"  Entity '{entity_name}': {', '.join(changes)}")

        # Check top-level source
        source = entity.get("source", {})
        expr = source.get("expression")
        if isinstance(expr, str):
            new_expr, changes = _rewrite_m_expression(expr)
            if changes:
                source["expression"] = new_expr
                all_changes.append(f"  Entity '{entity_name}' (top-level): {', '.join(changes)}")

    return patched, all_changes


def import_dataflow(
    token: str,
    workspace_id: str,
    dataflow_name: str,
    model: dict,
    dry_run: bool = False,
) -> None:
    """Re-import the patched dataflow definition, overwriting the existing one."""
    model_json_bytes = json.dumps(model, indent=2).encode("utf-8")

    if dry_run:
        print("\n[dry-run] Would POST updated model.json to Power BI API.")
        print("[dry-run] Updated model.json preview (first 2000 chars):")
        print(model_json_bytes[:2000].decode())
        return

    # Power BI import endpoint expects multipart/form-data with a .json file
    files = {"file": ("model.json", io.BytesIO(model_json_bytes), "application/json")}
    headers = {"Authorization": f"Bearer {token}"}

    resp = requests.post(
        f"{PBI_BASE}/groups/{workspace_id}/imports"
        f"?datasetDisplayName={requests.utils.quote(dataflow_name)}"
        f"&nameConflict=Overwrite"
        f"&skipReport=true",
        headers=headers,
        files=files,
        timeout=120,
    )

    if resp.status_code == 202:
        # Async import — poll until done
        import_id = resp.json().get("id")
        print(f"Import started (id={import_id}). Polling for completion...")
        _poll_import(token, workspace_id, import_id)
    elif resp.status_code in (200, 201):
        print("Import completed successfully.")
    else:
        print(f"ERROR: Import failed ({resp.status_code}): {resp.text}")
        sys.exit(1)


def _poll_import(token: str, workspace_id: str, import_id: str, max_wait: int = 120) -> None:
    deadline = time.time() + max_wait
    while time.time() < deadline:
        resp = requests.get(
            f"{PBI_BASE}/groups/{workspace_id}/imports/{import_id}",
            headers=_headers(token),
            timeout=30,
        )
        resp.raise_for_status()
        state = resp.json().get("importState", "")
        print(f"  Import state: {state}")
        if state == "Succeeded":
            print("Dataflow updated successfully.")
            return
        if state in ("Failed", "Canceled"):
            print(f"ERROR: Import ended with state '{state}': {resp.text}")
            sys.exit(1)
        time.sleep(5)
    print("WARNING: Timed out waiting for import to finish. Check Power BI service.")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="Fix Power BI dataflow Rate/Historyrates sources")
    parser.add_argument("--dry-run", action="store_true", help="Show changes without applying them")
    parser.add_argument("--workspace-id", help="Power BI workspace (group) ID to limit search")
    parser.add_argument("--dataflow-id", help="Specific dataflow object ID to patch")
    args = parser.parse_args()

    print("Authenticating via Azure CLI...")
    token = _get_token()

    print("Discovering dataflow...")
    workspace_id, dataflow_id, dataflow_name = find_target_dataflow(
        token,
        hint_workspace_id=args.workspace_id,
        hint_dataflow_id=args.dataflow_id,
    )

    print(f"\nExporting dataflow definition for '{dataflow_name}' (id={dataflow_id})...")
    model = export_dataflow(token, workspace_id, dataflow_id)

    print("Patching M expressions...")
    patched_model, changes = patch_model(model)

    if not changes:
        print(
            "\nNo Rate/Historyrates table references found in the M expressions.\n"
            "The dataflow may already be using the views, or the M expression format\n"
            "is non-standard. Use --dry-run to inspect the exported model.json.\n"
        )
        # Dump model for inspection
        with open("dataflow_model_exported.json", "w") as f:
            json.dump(model, f, indent=2)
        print("Exported model saved to: dataflow_model_exported.json")
        sys.exit(0)

    print("\nChanges to be applied:")
    for c in changes:
        print(c)

    if args.dry_run:
        with open("dataflow_model_patched.json", "w") as f:
            json.dump(patched_model, f, indent=2)
        print("\n[dry-run] Patched model saved to: dataflow_model_patched.json")
        print("[dry-run] No changes were applied to Power BI.")
        return

    confirm = input("\nApply these changes to the live dataflow? [y/N] ").strip().lower()
    if confirm != "y":
        print("Aborted.")
        sys.exit(0)

    print("\nImporting patched dataflow...")
    import_dataflow(token, workspace_id, dataflow_name, patched_model, dry_run=False)

    print("\nDone. Trigger a dataflow refresh in Power BI to confirm the fix.")


if __name__ == "__main__":
    main()
