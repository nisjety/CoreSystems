#!/usr/bin/env python3
"""Set up the Visma Dataform repository and everything it needs in a GCP project.

Mirrors how the Xero repository in the same project is set up: a Dataform repository whose
workflows run as their own service account, a `Dev` workspace, and BigQuery datasets that the
service account may write (and, for staging, only read). Safe to run again: every step looks
first and creates only what is missing. Nothing is deleted or overwritten.

What it creates in PROJECT:
  * service account  visma-dataform@PROJECT       ("Visma Dataform execution")
  * project role     roles/bigquery.jobUser        for that account
  * on the account   serviceAccountTokenCreator + serviceAccountUser for the Dataform service agent
  * datasets         the layers in LAYER_DATASETS (location europe-north2), the account is WRITER
  * dataset access   the account is READER on the input datasets (INPUT_DATASETS)
  * Dataform         repository `Visma` ("Visma warehouse", europe-north1) + workspace `Dev`

Needs: gcloud logged in as an owner of the project, and `google-cloud-bigquery` (the v2 venv
has it).

Usage: dataform/tools/setup_gcp.py [--project aquatiq-datavarehus] [--dry-run]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

from google.api_core.exceptions import NotFound
from google.cloud import bigquery

# BigQuery data location. Dataform itself does not run in europe-north2, so the repository is in
# europe-north1 and the compiled project says `defaultLocation: europe-north2`.
BQ_LOCATION = "europe-north2"
DATAFORM_LOCATION = "europe-north1"
REPOSITORY = "Visma"
REPOSITORY_NAME = "Visma warehouse"
WORKSPACE = "Dev"
ACCOUNT_ID = "visma-dataform"
# Datasets the project only reads: the service account is READER.
INPUT_DATASETS = {
    "dw_staging_visma": "Raw staging of Visma v2: one JSON table per endpoint (vsv2)",
    "dw_staging_currency": "Currency rates written by the currency service (Norges Bank and others)",
}

# One dataset per layer. The same names are in the `schema` of every definition in dataform/definitions.
LAYER_DATASETS = {
    "visma_bronze": "Bronze: Visma endpoints typed from raw staging JSON",
    "visma_bronze_excel": "Bronze: mapping workbooks loaded from Excel",
    "visma_silver": "Silver: cleaned Visma dimensions and facts",
    "visma_silver_common": "Silver: cross-source bridges and quality reports",
    "visma_silver_excel": "Silver: mapping seeds from the Excel workbooks",
    "visma_gold_common": "Gold: shared dimensions",
    "visma_gold_finance": "Gold: finance facts and dimensions",
    "visma_gold_sales": "Gold: sales and purchasing facts and dimensions",
    "visma_datamart": "Datamart: tables shaped for Power BI",
    "visma_assertions": "Dataform assertion results for the Visma project",
}


def sh(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(args[:4])} ... failed: {result.stderr.strip()[:300]}")
    return result.stdout.strip()


def token() -> str:
    return sh("gcloud", "auth", "print-access-token")


def api(method: str, url: str, body: dict | None = None) -> tuple[int, dict]:
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"Authorization": f"Bearer {token()}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            return response.status, json.loads(response.read() or b"{}")
    except urllib.error.HTTPError as error:
        return error.code, {"error": error.read().decode()[:500]}


def say(done: bool, text: str) -> None:
    print(f"  {'ok     ' if done else 'created'} {text}")


def service_account(project: str, dry: bool) -> tuple[str, bool]:
    email = f"{ACCOUNT_ID}@{project}.iam.gserviceaccount.com"
    existing = sh(
        "gcloud",
        "iam",
        "service-accounts",
        "list",
        "--project",
        project,
        "--filter",
        f"email={email}",
        "--format",
        "value(email)",
    )
    if existing:
        say(True, f"service account {email}")
        return email, True
    if dry:
        print(f"  would create service account {email}")
        return email, False
    else:
        sh(
            "gcloud",
            "iam",
            "service-accounts",
            "create",
            ACCOUNT_ID,
            "--project",
            project,
            "--display-name",
            "Visma Dataform execution",
        )
        say(False, f"service account {email}")
    return email, True


def project_role(project: str, email: str, dry: bool) -> None:
    role = "roles/bigquery.jobUser"
    policy = json.loads(sh("gcloud", "projects", "get-iam-policy", project, "--format", "json"))
    members = next((b["members"] for b in policy["bindings"] if b["role"] == role), [])
    if f"serviceAccount:{email}" in members:
        say(True, f"{role} for the service account")
    elif dry:
        print(f"  would grant {role}")
    else:
        sh(
            "gcloud",
            "projects",
            "add-iam-policy-binding",
            project,
            "--member",
            f"serviceAccount:{email}",
            "--role",
            role,
            "--condition",
            "None",
            "--format",
            "none",
        )
        say(False, f"{role} for the service account")


def agent_roles(project: str, email: str, exists: bool, dry: bool) -> None:
    number = sh("gcloud", "projects", "describe", project, "--format", "value(projectNumber)")
    agent = f"serviceAccount:service-{number}@gcp-sa-dataform.iam.gserviceaccount.com"
    if not exists:
        for role in ("roles/iam.serviceAccountTokenCreator", "roles/iam.serviceAccountUser"):
            print(f"  would grant {role} to the Dataform service agent")
        return
    policy = json.loads(
        sh(
            "gcloud",
            "iam",
            "service-accounts",
            "get-iam-policy",
            email,
            "--project",
            project,
            "--format",
            "json",
        )
    )
    for role in ("roles/iam.serviceAccountTokenCreator", "roles/iam.serviceAccountUser"):
        members = next((b["members"] for b in policy.get("bindings", []) if b["role"] == role), [])
        if agent in members:
            say(True, f"{role} for the Dataform service agent")
        elif dry:
            print(f"  would grant {role} to the Dataform service agent")
        else:
            sh(
                "gcloud",
                "iam",
                "service-accounts",
                "add-iam-policy-binding",
                email,
                "--project",
                project,
                "--member",
                agent,
                "--role",
                role,
                "--format",
                "none",
            )
            say(False, f"{role} for the Dataform service agent")


def dataset(
    client: bigquery.Client, project: str, name: str, email: str, role: str, dry: bool
) -> None:
    ref = f"{project}.{name}"
    try:
        found = client.get_dataset(ref)
        say(True, f"dataset {name}")
    except NotFound:
        if dry:
            print(f"  would create dataset {name}")
            return
        found = bigquery.Dataset(ref)
        found.location = BQ_LOCATION
        found.description = LAYER_DATASETS.get(name) or INPUT_DATASETS.get(name, "")
        found = client.create_dataset(found)
        say(False, f"dataset {name}")
    entries = list(found.access_entries)
    if any(e.entity_id == email and e.role == role for e in entries):
        return
    if dry:
        print(f"  would give the service account {role} on {name}")
        return
    entries.append(bigquery.AccessEntry(role, "userByEmail", email))
    found.access_entries = entries
    client.update_dataset(found, ["access_entries"])
    say(False, f"{role} on {name} for the service account")


def repository(project: str, email: str, dry: bool) -> None:
    base = (
        f"https://dataform.googleapis.com/v1beta1/projects/{project}/locations/{DATAFORM_LOCATION}"
    )
    repo = f"{base}/repositories/{REPOSITORY}"
    status, current = api("GET", repo)
    if status == 200:
        say(True, f"Dataform repository {REPOSITORY} (runs as {current.get('serviceAccount')})")
    elif dry:
        print(f"  would create Dataform repository {REPOSITORY}")
        return
    else:
        status, created = api(
            "POST",
            f"{base}/repositories?{urllib.parse.urlencode({'repositoryId': REPOSITORY})}",
            {"displayName": REPOSITORY_NAME, "serviceAccount": email},
        )
        if status != 200:
            raise RuntimeError(f"could not create the repository: {created}")
        say(False, f"Dataform repository {REPOSITORY}")
    status, _ = api("GET", f"{repo}/workspaces/{WORKSPACE}")
    if status == 200:
        say(True, f"workspace {WORKSPACE}")
    elif not dry:
        status, created = api(
            "POST", f"{repo}/workspaces?{urllib.parse.urlencode({'workspaceId': WORKSPACE})}", {}
        )
        if status != 200:
            raise RuntimeError(f"could not create the workspace: {created}")
        say(False, f"workspace {WORKSPACE}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--project", default="aquatiq-datavarehus")
    ap.add_argument(
        "--dry-run", action="store_true", help="say what would be created, create nothing"
    )
    args = ap.parse_args()
    client = bigquery.Client(project=args.project)

    print(f"project {args.project}{' (dry run)' if args.dry_run else ''}")
    email, exists = service_account(args.project, args.dry_run)
    project_role(args.project, email, args.dry_run)
    agent_roles(args.project, email, exists, args.dry_run)
    for name in INPUT_DATASETS:
        dataset(client, args.project, name, email, "READER", args.dry_run)
    for name in LAYER_DATASETS:
        dataset(client, args.project, name, email, "WRITER", args.dry_run)
    repository(args.project, email, args.dry_run)
    return 0


if __name__ == "__main__":
    sys.exit(main())
