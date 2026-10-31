import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BOOTSTRAP = ROOT / "deploy" / "bootstrap-gcp-main"


def _run_bootstrap(
    tmp_path, *, existing_parent=None, bucket_exists=False,
    bucket_in_project=False, datasets_exist=False,
):
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    fake_cli = """#!/usr/bin/env python3
import os
import sys
from pathlib import Path

args = sys.argv[1:]
with Path(os.environ['FAKE_LOG']).open('a') as log:
    log.write(Path(sys.argv[0]).name + ' ' + ' '.join(args) + '\\n')
if args[:2] == ['projects', 'describe']:
    if '--format=value(projectNumber)' in args:
        print('123456')
        sys.exit(0)
    parent = os.environ.get('FAKE_PROJECT_PARENT')
    if parent:
        print(parent)
        sys.exit(0)
    sys.exit(1)
if args[:2] == ['billing', 'projects'] and 'describe' in args:
    print('')
if args[:2] == ['storage', 'buckets'] and 'describe' in args:
    if os.environ.get('FAKE_BUCKET_EXISTS'):
        print('{"location":"EUROPE-NORTH2","uniform_bucket_level_access":true,"public_access_prevention":"enforced"}')
        sys.exit(0)
    sys.exit(1)
if args[:2] == ['storage', 'buckets'] and 'list' in args:
    if os.environ.get('FAKE_BUCKET_IN_PROJECT'):
        print('prj-dw-main-raw')
if Path(sys.argv[0]).name == 'bq' and 'show' in args:
    if os.environ.get('FAKE_DATASETS_EXIST'):
        print('{"location":"EUROPE-NORTH2"}')
        sys.exit(0)
    sys.exit(1)
"""
    for name in ("gcloud", "bq"):
        path = bin_dir / name
        path.write_text(fake_cli)
        path.chmod(0o755)
    log_path = tmp_path / "calls.log"
    env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}", FAKE_LOG=str(log_path))
    if existing_parent:
        env["FAKE_PROJECT_PARENT"] = existing_parent
    if bucket_exists:
        env["FAKE_BUCKET_EXISTS"] = "1"
    if bucket_in_project:
        env["FAKE_BUCKET_IN_PROJECT"] = "1"
    if datasets_exist:
        env["FAKE_DATASETS_EXIST"] = "1"
    result = subprocess.run([str(BOOTSTRAP)], env=env, text=True, capture_output=True)
    calls = log_path.read_text() if log_path.exists() else ""
    return result, calls


def test_bootstrap_provisions_only_empty_main_resources(tmp_path):
    result, calls = _run_bootstrap(tmp_path)

    assert result.returncode == 0, result.stderr
    assert "projects create prj-dw-main --organization=224665845666" in calls
    assert "billing projects link prj-dw-main --billing-account=01C215-AFF5CF-7558C9" in calls
    assert "storage buckets create gs://prj-dw-main-raw" in calls
    for dataset in ("dw_staging_xero", "dw_1_bronze_xero", "dw_1_silver_xero"):
        assert f"prj-dw-main:{dataset}" in calls
    assert "--location=europe-north2" in calls
    assert "--labels" not in calls
    assert "cp " not in calls


def test_bootstrap_rejects_existing_project_under_other_parent(tmp_path):
    result, calls = _run_bootstrap(tmp_path, existing_parent="999")

    assert result.returncode != 0
    assert "unexpected parent" in result.stderr
    assert "billing projects link" not in calls
    assert "services enable" not in calls


def test_bootstrap_rejects_bucket_outside_main_project(tmp_path):
    result, calls = _run_bootstrap(tmp_path, bucket_exists=True)

    assert result.returncode != 0
    assert "unexpected owner" in result.stderr
    assert "storage buckets list --project=prj-dw-main" in calls


def test_bootstrap_accepts_existing_main_resources_without_recreating(tmp_path):
    result, calls = _run_bootstrap(
        tmp_path, existing_parent="224665845666", bucket_exists=True,
        bucket_in_project=True, datasets_exist=True,
    )

    assert result.returncode == 0, result.stderr
    assert "projects create" not in calls
    assert "storage buckets create" not in calls
    assert "bq mk " not in calls
