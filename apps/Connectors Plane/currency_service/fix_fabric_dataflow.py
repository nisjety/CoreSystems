"""
Patch Fabric Dataflow Gen2: rewrite Rate/Historyrates table references
to use the RatePowerBI/HistoryratesPowerBI BigQuery views.

Workspace:  ValutaHub  (0953a26c-9b44-4504-8ebe-c96b03d22923)
Dataflow:   BigQuery Dataflow  (5cc614e5-a1e9-4b36-880f-e7d373234abc)

Usage:  python3 fix_fabric_dataflow.py [--dry-run]
"""

import argparse
import base64
import json
import re
import subprocess
import sys
import time
import copy

import requests

WORKSPACE_ID = "0953a26c-9b44-4504-8ebe-c96b03d22923"
DATAFLOW_ID  = "5cc614e5-a1e9-4b36-880f-e7d373234abc"
FABRIC_BASE  = "https://api.fabric.microsoft.com/v1"

VIEW_REPLACEMENTS = {
    "Rate":        "RatePowerBI",
    "Historyrates": "HistoryratesPowerBI",
}

# Matches  [Name="Rate"] or [Name="Rate",Kind="Table"] etc.
_TABLE_PATTERN = re.compile(
    r'\[Name\s*=\s*"(Rate|Historyrates)"\s*(?:,\s*Kind\s*=\s*"\w+")?\s*\]',
    re.IGNORECASE,
)


# ---------------------------------------------------------------------------
# Auth
# ---------------------------------------------------------------------------

def _get_token(resource: str) -> str:
    res = subprocess.run(
        ["az", "account", "get-access-token", "--resource", resource, "--output", "json"],
        capture_output=True, text=True,
    )
    if res.returncode != 0:
        print(f"ERROR: az CLI failed for resource {resource}:\n{res.stderr}")
        sys.exit(1)
    return json.loads(res.stdout)["accessToken"]


def get_fabric_token() -> str:
    return _get_token("https://api.fabric.microsoft.com")


def fab_headers(token: str) -> dict:
    return {"Authorization": f"Bearer {token}", "Content-Type": "application/json"}


# ---------------------------------------------------------------------------
# Export definition
# ---------------------------------------------------------------------------

def get_definition(token: str) -> dict:
    """
    POST .../getDefinition to retrieve the dataflow's mashup document.
    Returns the parsed definition as a dict with the raw parts list.
    """
    url = f"{FABRIC_BASE}/workspaces/{WORKSPACE_ID}/items/{DATAFLOW_ID}/getDefinition"
    resp = requests.post(url, headers=fab_headers(token), timeout=60)

    if resp.status_code == 202:
        # Long-running operation - poll Location header
        operation_url = resp.headers.get("Location") or resp.headers.get("location")
        print(f"Long-running op started, polling: {operation_url}")
        resp = _poll_lro(token, operation_url)

    if resp.status_code not in (200, 202):
        print(f"ERROR getDefinition: HTTP {resp.status_code}\n{resp.text}")
        sys.exit(1)

    return resp.json()


def _poll_lro(token: str, url: str, max_wait: int = 120):
    """Poll a Fabric long-running operation URL until it completes."""
    deadline = time.time() + max_wait
    while time.time() < deadline:
        r = requests.get(url, headers={"Authorization": f"Bearer {token}"}, timeout=30)
        state = r.json().get("status", "")
        print(f"  LRO status: {state}")
        if state in ("Succeeded", "succeeded"):
            # Follow createdTimeUtc body result URL if present
            result_url = r.json().get("resourceLocation")
            if result_url:
                return requests.get(result_url, headers={"Authorization": f"Bearer {token}"}, timeout=30)
            return r
        if state in ("Failed", "failed", "Canceled"):
            print(f"ERROR: LRO failed: {r.text}")
            sys.exit(1)
        time.sleep(4)
    print("ERROR: LRO timed out")
    sys.exit(1)


# ---------------------------------------------------------------------------
# M expression rewriting
# ---------------------------------------------------------------------------

def _rewrite_m(expr: str) -> tuple[str, list[str]]:
    changes = []

    def _replacer(m: re.Match) -> str:
        orig = m.group(1)
        repl = VIEW_REPLACEMENTS.get(orig)
        if not repl:
            return m.group(0)
        changes.append(f"{orig!r} -> {repl!r}")
        return f'[Name="{repl}",Kind="View"]'

    updated = _TABLE_PATTERN.sub(_replacer, expr)
    return updated, changes


def patch_definition(definition: dict) -> tuple[dict, list[str]]:
    """
    The Fabric getDefinition response has the form:
    {
      "definition": {
        "parts": [
          { "path": "dataflow-content.json", "payload": "<base64>", "payloadType": "InlineBase64" },
          ...
        ]
      }
    }
    The main payload is 'dataflow-content.json' (base64-encoded JSON).
    That JSON contains an "annotations" field with the Mashup/M document.

    Returns (patched_definition, list_of_changes).
    """
    patched = copy.deepcopy(definition)
    all_changes = []

    parts = patched.get("definition", {}).get("parts", [])
    for part in parts:
        path = part.get("path", "")
        payload_b64 = part.get("payload", "")
        if not payload_b64:
            continue

        raw_bytes = base64.b64decode(payload_b64)
        print(f"\n--- Part: {path} ({len(raw_bytes)} bytes) ---")

        # Try to parse as JSON (the main dataflow-content.json is JSON)
        try:
            content = json.loads(raw_bytes.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            # Could be the Mashup binary - try as text for inspection
            try:
                text = raw_bytes.decode("utf-8")
                print(f"  Text content (first 500 chars): {text[:500]}")
                new_text, changes = _rewrite_m(text)
                if changes:
                    all_changes.extend([f"  {path}: {c}" for c in changes])
                    part["payload"] = base64.b64encode(new_text.encode("utf-8")).decode("ascii")
                    print(f"  Applied {len(changes)} substitution(s): {changes}")
            except UnicodeDecodeError:
                print(f"  Binary content, skipping")
            continue

        # It's JSON - walk through it looking for M expressions
        content_str = json.dumps(content)
        print(f"  JSON keys: {list(content.keys()) if isinstance(content, dict) else type(content).__name__}")

        # The M expressions live in various places depending on format:
        # 1. content["entities"][n]["partitions"][m]["source"]["expression"]
        # 2. content["pq"] (Power Query)
        # 3. Inside "annotations" (e.g. key "pbidesktop:QueryDefinitions")
        content_str_new, direct_changes = _rewrite_m(content_str)
        if direct_changes:
            all_changes.extend([f"  {path}: {c}" for c in direct_changes])
            new_content = json.loads(content_str_new)
            new_payload = base64.b64encode(
                json.dumps(new_content).encode("utf-8")
            ).decode("ascii")
            part["payload"] = new_payload
            print(f"  Applied {len(direct_changes)} substitution(s) in JSON: {direct_changes}")

            # Show context of changes for review
            for dc in direct_changes:
                orig_name = dc.split("'")[1]
                # Find surrounding context in the JSON string
                idx = content_str.find(f'"Name":"{orig_name}"')
                if idx == -1:
                    idx = content_str.find(f'Name="{orig_name}"')
                if idx >= 0:
                    snippet = content_str[max(0, idx-80):idx+120]
                    print(f"  Context: ...{snippet}...")
        else:
            print(f"  No Rate/Historyrates references found")

    return patched, all_changes


# ---------------------------------------------------------------------------
# Re-import
# ---------------------------------------------------------------------------

def update_definition(token: str, patched_definition: dict, dry_run: bool) -> None:
    if dry_run:
        with open("fabric_dataflow_patched.json", "w") as f:
            json.dump(patched_definition, f, indent=2)
        print("\n[dry-run] Patched definition saved to: fabric_dataflow_patched.json")
        return

    url = f"{FABRIC_BASE}/workspaces/{WORKSPACE_ID}/items/{DATAFLOW_ID}/updateDefinition"
    resp = requests.post(url, headers=fab_headers(token),
                         json=patched_definition, timeout=120)

    if resp.status_code == 202:
        op_url = resp.headers.get("Location") or resp.headers.get("location")
        print(f"Update started, polling LRO: {op_url}")
        _poll_lro(token, op_url)
        print("Dataflow definition updated successfully.")
    elif resp.status_code in (200, 204):
        print("Dataflow definition updated successfully.")
    else:
        print(f"ERROR updateDefinition: HTTP {resp.status_code}\n{resp.text}")
        sys.exit(1)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="Patch Fabric Dataflow BigQuery table->view references")
    parser.add_argument("--dry-run", action="store_true",
                        help="Export and show changes without writing back")
    args = parser.parse_args()

    print("Getting Fabric API token via Azure CLI...")
    token = get_fabric_token()

    print(f"\nExporting definition for dataflow {DATAFLOW_ID}...")
    definition = get_definition(token)

    # Save original for reference
    with open("fabric_dataflow_original.json", "w") as f:
        json.dump(definition, f, indent=2)
    print("Original definition saved to: fabric_dataflow_original.json")

    print("\nPatching M expressions...")
    patched, changes = patch_definition(definition)

    if not changes:
        print(
            "\nNo Rate/Historyrates table references found in the dataflow definition.\n"
            "Inspect fabric_dataflow_original.json to find the actual M expression format."
        )
        sys.exit(0)

    print("\nChanges to be applied:")
    for c in changes:
        print(f"  {c}")

    if args.dry_run:
        with open("fabric_dataflow_patched.json", "w") as f:
            json.dump(patched, f, indent=2)
        print("\n[dry-run] Patched definition saved to: fabric_dataflow_patched.json")
        print("[dry-run] No changes written to Power BI.")
        return

    confirm = input("\nApply these changes to the live dataflow? [y/N] ").strip().lower()
    if confirm != "y":
        print("Aborted.")
        sys.exit(0)

    print("\nUploading patched definition...")
    update_definition(token, patched, dry_run=False)
    print("\nDone. Trigger a dataflow refresh in Power BI to confirm.")


if __name__ == "__main__":
    main()
