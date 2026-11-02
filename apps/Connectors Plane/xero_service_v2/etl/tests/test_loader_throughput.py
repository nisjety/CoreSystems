"""Offline regression tests for bounded loader concurrency."""

import os
import subprocess
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest
from google.cloud import bigquery

from etl.common.bq_writer import BQWriter


ROOT = Path(__file__).resolve().parents[2]
TUNING_SCRIPT = ROOT / "deploy" / "xero-staging-loader-throughput"
RAW_NOTIFICATION_SCRIPT = ROOT / "deploy" / "provision-xero-raw-notification"
KNOWN_SCHEMA = [bigquery.SchemaField("tenant_id", "STRING"),
                bigquery.SchemaField("record_id", "STRING")]


def _writer_with_spy(schema, write_temp, run_merge):
    writer = object.__new__(BQWriter)
    writer.project = "project"
    writer.dataset = "dataset"
    writer._get_schema = lambda _target: schema
    writer._write_temp = write_temp
    writer._ensure_target_exists = lambda _target, _tmp: None
    writer._evolve_target_schema = lambda _target, _tmp, _schema: None
    writer._run_merge = run_merge
    writer._drop_temp = lambda _tmp: None
    return writer


def _merge_two(writer, table):
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [
            pool.submit(writer.merge, table, [{"tenant_id": "t", "record_id": str(i)}])
            for i in range(2)
        ]
        return [future.result(timeout=5) for future in futures]


def test_existing_table_temp_loads_overlap_but_merges_stay_serial():
    load_barrier = threading.Barrier(2, timeout=2)
    merge_guard = threading.Lock()
    active_merges = 0
    max_active_merges = 0

    def write_temp(_tmp, _rows, schema):
        assert schema == KNOWN_SCHEMA
        load_barrier.wait()

    def run_merge(_target, _tmp, _columns, _keys):
        nonlocal active_merges, max_active_merges
        with merge_guard:
            active_merges += 1
            max_active_merges = max(max_active_merges, active_merges)
        time.sleep(0.02)
        with merge_guard:
            active_merges -= 1

    writer = _writer_with_spy(KNOWN_SCHEMA, write_temp, run_merge)
    assert _merge_two(writer, "throughput_existing") == [1, 1]
    assert max_active_merges == 1


def test_new_table_keeps_load_and_creation_inside_lock():
    guard = threading.Lock()
    active_loads = 0
    max_active_loads = 0

    def write_temp(_tmp, _rows, schema):
        nonlocal active_loads, max_active_loads
        assert schema is None
        with guard:
            active_loads += 1
            max_active_loads = max(max_active_loads, active_loads)
        time.sleep(0.02)
        with guard:
            active_loads -= 1

    writer = _writer_with_spy(None, write_temp, lambda *_args: None)
    assert _merge_two(writer, "throughput_new") == [1, 1]
    assert max_active_loads == 1


def test_new_table_rechecks_schema_before_loading():
    used_schemas = []
    writer = _writer_with_spy(
        None, lambda _tmp, _rows, schema: used_schemas.append(schema), lambda *_args: None
    )
    checks = iter([None, ["created-by-other-request"]])
    writer._get_schema = lambda _target: next(checks)

    assert writer.merge("throughput_new_schema", [{"tenant_id": "t", "record_id": "r"}]) == 1
    assert used_schemas == [["created-by-other-request"]]


def test_existing_table_rechecks_schema_before_evolution():
    observed = []
    writer = _writer_with_spy(
        KNOWN_SCHEMA,
        lambda _tmp, _rows, schema: observed.append(("load", schema)),
        lambda *_args: None,
    )
    updated_schema = [*KNOWN_SCHEMA, bigquery.SchemaField("other", "STRING")]
    checks = iter([KNOWN_SCHEMA, updated_schema])
    writer._get_schema = lambda _target: next(checks)
    writer._evolve_target_schema = lambda _target, _tmp, schema: observed.append(
        ("evolve", schema)
    )

    assert writer.merge("throughput_evolution", [{"tenant_id": "t", "record_id": "r"}]) == 1
    assert observed == [
        ("load", KNOWN_SCHEMA),
        ("evolve", updated_schema),
    ]


def test_new_column_loads_remain_serial_for_schema_evolution():
    guard = threading.Lock()
    active_loads = 0
    max_active_loads = 0

    def write_temp(_tmp, _rows, schema):
        nonlocal active_loads, max_active_loads
        assert schema == KNOWN_SCHEMA
        with guard:
            active_loads += 1
            max_active_loads = max(max_active_loads, active_loads)
        time.sleep(0.02)
        with guard:
            active_loads -= 1

    writer = _writer_with_spy(KNOWN_SCHEMA, write_temp, lambda *_args: None)
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(writer.merge, "new_field", [
            {"tenant_id": "t", "record_id": str(i), "new_field": "text"}
        ]) for i in range(2)]
        assert [future.result(timeout=5) for future in futures] == [1, 1]
    assert max_active_loads == 1


def test_failed_parallel_temp_load_is_cleaned_up():
    def fail_load(_tmp, _rows, schema):
        raise RuntimeError("load failed")

    writer = _writer_with_spy(
        KNOWN_SCHEMA,
        fail_load,
        lambda *_args: None,
    )
    dropped = []
    writer._drop_temp = dropped.append

    with pytest.raises(RuntimeError, match="load failed"):
        writer.merge("throughput_failure", [{"tenant_id": "t", "record_id": "r"}])
    assert len(dropped) == 1
    assert "_tmp_throughput_failure_" in dropped[0]


def test_snapshot_replay_path_still_deduplicates_and_inserts_only():
    calls = []
    writer = _writer_with_spy(
        KNOWN_SCHEMA,
        lambda _tmp, rows, schema: calls.append(("load", rows, schema)),
        lambda *_args: pytest.fail("snapshot path must not call update MERGE"),
    )
    writer._run_insert_if_absent = lambda _target, _tmp, columns, keys: calls.append(
        ("insert_if_absent", columns, keys)
    )
    writer._drop_temp = lambda _tmp: calls.append(("drop",))
    row = {"tenant_id": "t", "snapshot_id": "s", "payload": "same"}

    result = writer.insert_if_absent(
        "snapshots", [row, row.copy()], ("tenant_id", "snapshot_id"), ["fallback-schema"]
    )

    assert result == 1
    assert calls[0] == ("load", [row], KNOWN_SCHEMA)
    assert calls[1] == ("insert_if_absent", list(row), ("tenant_id", "snapshot_id"))
    assert calls[2] == ("drop",)


def test_throughput_deploy_requires_explicit_apply_and_uses_bounded_parallelism(tmp_path):
    fake_gcloud = tmp_path / "gcloud"
    output = tmp_path / "args"
    fake_gcloud.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$GCLOUD_ARGS_FILE"\n')
    fake_gcloud.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{tmp_path}:{os.environ['PATH']}",
        "GCLOUD_ARGS_FILE": str(output),
    }
    source_dir = tmp_path / "isolated"
    (source_dir / "etl" / "common").mkdir(parents=True)
    for path in ("main.py", "requirements.txt", "etl/common/bq_writer.py"):
        (source_dir / path).write_text("test")
    main_env = tmp_path / "main.env"
    main_env.write_text("\n".join((
        "GCP_PROJECT=aquatiq-datavarehus",
        "GCS_BUCKET=aquatiq-datavarehus-xero-raw",
        "XERO_STAGING_DATASET=dw_staging_xero",
        "XERO_STAGING_LAYOUT=warehouse",
        "XERO_INGESTION_NAMESPACE=main",
        "XERO_INGESTION_GENERATION=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "XERO_FRESH_START_AT=2026-10-15T23:19:31Z",
    )))

    preview = subprocess.run(
        ["bash", str(TUNING_SCRIPT), "--preview", str(source_dir), str(main_env)],
        env=env, check=True, capture_output=True, text=True
    )
    assert not output.exists()
    assert f"--source={source_dir}" in preview.stdout
    assert "--concurrency=80" in preview.stdout
    assert "--memory=1Gi" in preview.stdout
    assert "--cpu=1" in preview.stdout
    assert "--max-instances=3" in preview.stdout
    assert "--retry" in preview.stdout
    assert "--entry-point=process_fresh_gcs_upload" in preview.stdout
    assert "--project=aquatiq-datavarehus" in preview.stdout
    assert "THREADS" not in preview.stdout

    subprocess.run(
        ["bash", str(TUNING_SCRIPT), "--apply", str(source_dir), str(main_env)],
        env=env,
        check=True,
    )
    assert output.read_text().splitlines() == [
        "functions",
        "deploy",
        "xero-staging-loader",
        "--gen2",
        "--project=aquatiq-datavarehus",
        "--region=europe-north1",
        f"--source={source_dir}",
        "--runtime=python312",
        "--entry-point=process_fresh_gcs_upload",
        "--memory=1Gi",
        "--cpu=1",
        "--concurrency=80",
        "--max-instances=3",
        "--retry",
        "--set-env-vars=GCP_PROJECT=aquatiq-datavarehus,GCS_BUCKET=aquatiq-datavarehus-xero-raw,XERO_STAGING_DATASET=dw_staging_xero,XERO_STAGING_LAYOUT=warehouse,XERO_INGESTION_NAMESPACE=main,XERO_INGESTION_GENERATION=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa,XERO_FRESH_START_AT=2026-10-15T23:19:31Z",
    ]


def test_throughput_deploy_rejects_checkout_root():
    result = subprocess.run(
        ["bash", str(TUNING_SCRIPT), "--preview", str(ROOT), str(ROOT / ".env.main")],
        capture_output=True, text=True,
    )
    assert result.returncode == 2


def test_raw_notification_provisioner_is_scoped_and_idempotent(tmp_path):
    fake_gcloud = tmp_path / "gcloud"
    calls = tmp_path / "calls"
    fake_gcloud.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$*\" >> \"$GCLOUD_ARGS_FILE\"\n"
        "if [ \"$1 $2 $3\" = 'projects describe aquatiq-datavarehus' ]; then\n"
        "  printf '949017285874\\n'\n"
        "elif [ \"$1 $2 $3 $4\" = 'storage buckets notifications list' ]; then\n"
        "  printf '%s\\n' \"$GCLOUD_NOTIFICATIONS_JSON\"\n"
        "fi\n"
    )
    fake_gcloud.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{tmp_path}:{os.environ['PATH']}",
        "GCLOUD_ARGS_FILE": str(calls),
        "GCLOUD_NOTIFICATIONS_JSON": (
            '[{"Notification Configuration":{"event_types":["OBJECT_FINALIZE"],'
            '"id":"3",'
            '"payload_format":"JSON_API_V1",'
            '"topic":"//pubsub.googleapis.com/projects/aquatiq-datavarehus/'
            'topics/xero-raw-finalized"}}]'
        ),
    }
    main_env = tmp_path / "main.env"
    main_env.write_text("\n".join((
        "GCP_PROJECT=aquatiq-datavarehus",
        "GCS_BUCKET=aquatiq-datavarehus-xero-raw",
        "XERO_RAW_FINALIZED_TOPIC=xero-raw-finalized",
    )))

    preview = subprocess.run(
        ["bash", str(RAW_NOTIFICATION_SCRIPT), "--preview", str(main_env)],
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    assert "aquatiq-datavarehus-xero-raw" in preview.stdout
    assert "xero-raw-finalized" in preview.stdout
    assert not calls.exists()

    subprocess.run(
        ["bash", str(RAW_NOTIFICATION_SCRIPT), "--apply", str(main_env)],
        env=env,
        check=True,
    )
    applied = calls.read_text()
    assert "pubsub topics add-iam-policy-binding xero-raw-finalized" in applied
    assert "service-949017285874@gs-project-accounts.iam.gserviceaccount.com" in applied
    assert "storage buckets notifications list gs://aquatiq-datavarehus-xero-raw" in applied
    assert "storage buckets notifications create" not in applied


def test_raw_notification_provisioner_creates_json_notification_when_absent(tmp_path):
    fake_gcloud = tmp_path / "gcloud"
    calls = tmp_path / "calls"
    fake_gcloud.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$*\" >> \"$GCLOUD_ARGS_FILE\"\n"
        "if [ \"$1 $2 $3\" = 'projects describe aquatiq-datavarehus' ]; then\n"
        "  printf '949017285874\\n'\n"
        "elif [ \"$1 $2 $3 $4\" = 'storage buckets notifications list' ]; then\n"
        "  printf '[]\\n'\n"
        "fi\n"
    )
    fake_gcloud.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{tmp_path}:{os.environ['PATH']}",
        "GCLOUD_ARGS_FILE": str(calls),
    }
    main_env = tmp_path / "main.env"
    main_env.write_text("\n".join((
        "GCP_PROJECT=aquatiq-datavarehus",
        "GCS_BUCKET=aquatiq-datavarehus-xero-raw",
        "XERO_RAW_FINALIZED_TOPIC=xero-raw-finalized",
    )))

    subprocess.run(
        ["bash", str(RAW_NOTIFICATION_SCRIPT), "--apply", str(main_env)],
        env=env,
        check=True,
    )
    assert (
        "storage buckets notifications create gs://aquatiq-datavarehus-xero-raw "
        "--topic=xero-raw-finalized --event-types=OBJECT_FINALIZE "
        "--payload-format=json --project=aquatiq-datavarehus"
    ) in calls.read_text()


def test_raw_notification_provisioner_removes_only_unfiltered_legacy_duplicate(tmp_path):
    fake_gcloud = tmp_path / "gcloud"
    calls = tmp_path / "calls"
    fake_gcloud.write_text(
        "#!/bin/sh\n"
        "printf '%s\\n' \"$*\" >> \"$GCLOUD_ARGS_FILE\"\n"
        "if [ \"$1 $2 $3\" = 'projects describe aquatiq-datavarehus' ]; then\n"
        "  printf '949017285874\\n'\n"
        "elif [ \"$1 $2 $3 $4\" = 'storage buckets notifications list' ]; then\n"
        "  printf '%s\\n' \"$GCLOUD_NOTIFICATIONS_JSON\"\n"
        "fi\n"
    )
    fake_gcloud.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{tmp_path}:{os.environ['PATH']}",
        "GCLOUD_ARGS_FILE": str(calls),
        "GCLOUD_NOTIFICATIONS_JSON": (
            '[{"Notification Configuration":{"event_types":["OBJECT_FINALIZE"],'
            '"id":"2","payload_format":"NONE",'
            '"topic":"//pubsub.googleapis.com/projects/aquatiq-datavarehus/'
            'topics/xero-raw-finalized"}},'
            '{"Notification Configuration":{"event_types":["OBJECT_FINALIZE"],'
            '"id":"3","payload_format":"JSON_API_V1",'
            '"topic":"//pubsub.googleapis.com/projects/aquatiq-datavarehus/'
            'topics/xero-raw-finalized"}}]'
        ),
    }
    main_env = tmp_path / "main.env"
    main_env.write_text("\n".join((
        "GCP_PROJECT=aquatiq-datavarehus",
        "GCS_BUCKET=aquatiq-datavarehus-xero-raw",
        "XERO_RAW_FINALIZED_TOPIC=xero-raw-finalized",
    )))

    subprocess.run(
        ["bash", str(RAW_NOTIFICATION_SCRIPT), "--apply", str(main_env)],
        env=env,
        check=True,
    )
    applied = calls.read_text()
    assert (
        "storage buckets notifications delete "
        "projects/_/buckets/aquatiq-datavarehus-xero-raw/notificationConfigs/2 "
        "--project=aquatiq-datavarehus --quiet"
    ) in applied
    assert "storage buckets notifications create" not in applied
