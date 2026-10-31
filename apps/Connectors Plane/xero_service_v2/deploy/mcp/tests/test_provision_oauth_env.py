import os
import stat
from pathlib import Path

import pytest

from deploy.mcp import provision_oauth_env as utility
from deploy.mcp.provision_oauth_env import ProvisionError, provision


CLIENT_ID = "fixture-pkce-client"
CLIENT_SECRET = "fixture-pkce-secret"
SCOPES = (
    "openid profile email offline_access "
    "accounting.transactions.read accounting.contacts.read accounting.settings.read "
    "accounting.reports.read accounting.journals.read accounting.attachments.read "
    "accounting.budgets.read"
)


def source_file(tmp_path: Path, content: str | None = None) -> Path:
    source = tmp_path / ".env.bff"
    source.write_text(
        content
        if content is not None
        else (
            f'export XERO_PKCE_CLIENT_ID="{CLIENT_ID}"\n'
            f"XERO_PKCE_CLIENT_SECRET='{CLIENT_SECRET}' # private\n"
            "XERO_MCP_TOKEN=old-static-bearer\n"
            "XERO_ORG_1_CLIENT_ID=old-custom-id\n"
            "XERO_ORG_1_CLIENT_SECRET=old-custom-secret\n"
            "XERO_ORG_1_TENANT_ID=old-custom-tenant\n"
            "XERO_MCP_CLIENTS_JSON=old-service-clients\n"
        ),
        encoding="utf-8",
    )
    source.chmod(0o600)
    return source


def run_provision(source: Path, output: Path) -> None:
    provision(source, output, runtime_uid=os.getuid(), runtime_gid=os.getgid())


def read_env(output: Path) -> dict[str, str]:
    return dict(line.split("=", 1) for line in output.read_text().splitlines())


def test_provisions_private_oauth_only_runtime(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    output = tmp_path / "server.env"

    run_provision(source, output)

    assert read_env(output) == {
        "XERO_AUTH_MODE": "pkce",
        "XERO_PKCE_CLIENT_ID": CLIENT_ID,
        "XERO_PKCE_CLIENT_SECRET": CLIENT_SECRET,
        "XERO_MCP_OAUTH_ISSUER": "https://tools.aquatiq.com",
        "XERO_MCP_OAUTH_CLIENT_ID": "claude-xero",
        "XERO_MCP_OAUTH_XERO_REDIRECT_URI": "https://tools.aquatiq.com/oauth/xero/callback",
        "XERO_MCP_OAUTH_SCOPES": f'"{SCOPES}"',
        "XERO_CONNECTION_STORE_PATH": "/app/out/mcp/connections.json",
        "XERO_MCP_STORAGE_ROOT": "/app/out/mcp",
        "XERO_MCP_BIND": "0.0.0.0:5003",
        "XERO_MCP_ALLOWED_HOSTS": "tools.aquatiq.com,localhost,127.0.0.1",
        "XERO_MCP_TLS_PROXY": "true",
        "XERO_MCP_TOOL_MODE": "compact",
        "XERO_MCP_OAUTH_ZERO_DATA_RETENTION": "true",
        "XERO_MCP_OAUTH_ALLOW_EXTRACT": "false",
        "XERO_MCP_OAUTH_ALLOW_WAREHOUSE": "false",
        "XERO_MCP_OAUTH_WRITE_ACTIONS": "",
        "XERO_MCP_WAREHOUSE_PROJECT": "aquatiq-datavarehus",
        "XERO_MCP_WAREHOUSE_DATASET": "dw_staging_xero",
        "XERO_MCP_WAREHOUSE_LOCATION": "europe-north2",
        "XERO_MCP_WAREHOUSE_MAX_BYTES_BILLED": "536870912",
        "GCP_PROJECT": "aquatiq-datavarehus",
        "GCS_BUCKET": "aquatiq-datavarehus-xero-raw",
        "GCS_PREFIX": "raw/xero",
        "GOOGLE_APPLICATION_CREDENTIALS": "/run/secrets/gcp-mcp.json",
        "XERO_MCP_ENABLE_WRITES": "false",
    }
    assert stat.S_IMODE(output.stat().st_mode) == 0o600
    assert (output.stat().st_uid, output.stat().st_gid) == (os.getuid(), os.getgid())
    assert source.read_text().startswith("export XERO_PKCE_CLIENT_ID=")
    assert not any(output.name.startswith(".server.env.") for output in tmp_path.iterdir())


@pytest.mark.parametrize(
    "content",
    [
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\n",
        f"XERO_PKCE_CLIENT_SECRET={CLIENT_SECRET}\n",
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\nXERO_PKCE_CLIENT_SECRET=\n",
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\nXERO_PKCE_CLIENT_SECRET='unterminated\n",
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\nXERO_PKCE_CLIENT_SECRET={CLIENT_SECRET}\nXERO_PKCE_CLIENT_SECRET=duplicate\n",
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\nXERO_PKCE_CLIENT_SECRET='unsafe value'\n",
    ],
)
def test_bad_source_does_not_publish(tmp_path: Path, content: str) -> None:
    source = source_file(tmp_path, content)
    output = tmp_path / "server.env"

    with pytest.raises(ProvisionError):
        run_provision(source, output)

    assert not output.exists()


def test_unrelated_source_values_are_not_copied(tmp_path: Path) -> None:
    source = source_file(
        tmp_path,
        f"XERO_PKCE_CLIENT_ID={CLIENT_ID}\nXERO_PKCE_CLIENT_SECRET={CLIENT_SECRET}\nINJECT=bad\n",
    )
    output = tmp_path / "server.env"

    run_provision(source, output)

    assert "INJECT" not in read_env(output)


def test_privileged_oauth_capabilities_require_private_result_retention(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    denied_output = tmp_path / "denied.env"

    with pytest.raises(ProvisionError, match="retain-results"):
        provision(
            source,
            denied_output,
            runtime_uid=os.getuid(),
            runtime_gid=os.getgid(),
            allow_extract=True,
        )

    with pytest.raises(ProvisionError, match="retain-results"):
        provision(
            source,
            tmp_path / "write-denied.env",
            runtime_uid=os.getuid(),
            runtime_gid=os.getgid(),
            write_actions=("create_contact",),
        )

    output = tmp_path / "enabled.env"
    provision(
        source,
        output,
        runtime_uid=os.getuid(),
        runtime_gid=os.getgid(),
        allow_extract=True,
        allow_warehouse=True,
        retain_results=True,
    )

    values = read_env(output)
    assert values["XERO_MCP_OAUTH_ALLOW_EXTRACT"] == "true"
    assert values["XERO_MCP_OAUTH_ALLOW_WAREHOUSE"] == "true"
    assert values["XERO_MCP_OAUTH_ZERO_DATA_RETENTION"] == "false"

    write_output = tmp_path / "write-enabled.env"
    provision(
        source,
        write_output,
        runtime_uid=os.getuid(),
        runtime_gid=os.getgid(),
        retain_results=True,
        write_actions=("create_contact", "update_invoice"),
    )
    write_values = read_env(write_output)
    assert write_values["XERO_MCP_ENABLE_WRITES"] == "true"
    assert write_values["XERO_MCP_OAUTH_WRITE_ACTIONS"] == "create_contact,update_invoice"
    assert "accounting.contacts" in write_values["XERO_MCP_OAUTH_SCOPES"]
    assert "accounting.invoices" in write_values["XERO_MCP_OAUTH_SCOPES"]


@pytest.mark.parametrize("mode", [0o644, 0o640, 0o604])
def test_non_owner_only_source_is_rejected(tmp_path: Path, mode: int) -> None:
    source = source_file(tmp_path)
    source.chmod(mode)
    output = tmp_path / "server.env"

    with pytest.raises(ProvisionError, match="source permissions"):
        run_provision(source, output)

    assert not output.exists()


def test_source_symlink_is_rejected(tmp_path: Path) -> None:
    target = source_file(tmp_path)
    source = tmp_path / "source-link"
    source.symlink_to(target)
    output = tmp_path / "server.env"

    with pytest.raises(ProvisionError, match="symlink"):
        run_provision(source, output)

    assert not output.exists()


@pytest.mark.parametrize("existing_kind", ["file", "symlink"])
def test_existing_output_is_never_replaced(tmp_path: Path, existing_kind: str) -> None:
    source = source_file(tmp_path)
    output = tmp_path / "server.env"
    victim = tmp_path / "victim"
    victim.write_text("keep", encoding="ascii")
    if existing_kind == "symlink":
        output.symlink_to(victim)
    else:
        output.write_text("keep", encoding="ascii")

    with pytest.raises(FileExistsError):
        run_provision(source, output)

    assert output.read_text() == "keep"
    assert victim.read_text() == "keep"


def test_publish_race_preserves_other_writer_and_cleans_stage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = source_file(tmp_path)
    output = tmp_path / "server.env"
    original_link = os.link

    def race_link(src: object, dst: object) -> None:
        output.write_text("other-writer", encoding="ascii")
        original_link(src, dst)

    monkeypatch.setattr(os, "link", race_link)
    with pytest.raises(FileExistsError):
        run_provision(source, output)

    assert output.read_text() == "other-writer"
    assert sorted(path.name for path in tmp_path.iterdir()) == [".env.bff", "server.env"]


def test_publish_error_cleans_stage(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    source = source_file(tmp_path)
    output = tmp_path / "server.env"

    def fail_link(*args: object, **kwargs: object) -> None:
        raise OSError("simulated publish failure")

    monkeypatch.setattr(os, "link", fail_link)
    with pytest.raises(OSError, match="simulated publish failure"):
        run_provision(source, output)

    assert sorted(path.name for path in tmp_path.iterdir()) == [".env.bff"]


def test_rejects_same_source_and_output(tmp_path: Path) -> None:
    source = source_file(tmp_path)

    with pytest.raises(ProvisionError, match="distinct"):
        run_provision(source, source)

    assert source.read_text().startswith("export XERO_PKCE_CLIENT_ID=")


def test_rejects_writable_output_directory(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    directory = tmp_path / "writable"
    directory.mkdir()
    directory.chmod(0o777)
    output = directory / "server.env"

    with pytest.raises(ProvisionError, match="directory"):
        run_provision(source, output)

    assert not output.exists()


@pytest.mark.parametrize("uid,gid", [(-1, 1), (1, -1)])
def test_invalid_ownership_is_rejected(tmp_path: Path, uid: int, gid: int) -> None:
    source = source_file(tmp_path)
    output = tmp_path / "server.env"

    with pytest.raises(ProvisionError, match="non-negative"):
        provision(source, output, runtime_uid=uid, runtime_gid=gid)

    assert not output.exists()


def test_cli_defaults_and_overrides_ownership(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[tuple[tuple[object, ...], dict[str, object]]] = []

    def capture(*args: object, **kwargs: object) -> None:
        calls.append((args, kwargs))

    monkeypatch.setattr(utility, "provision", capture)
    assert utility.main(["--output-env", "server.env"]) == 0
    assert (
        utility.main(
            [
                "--source-env",
                "private.env",
                "--output-env",
                "other.env",
                "--uid",
                "42",
                "--gid",
                "43",
                "--retain-results",
                "--allow-write",
                "create_contact",
                "--allow-write",
                "update_invoice",
            ]
        )
        == 0
    )
    assert calls == [
        (
            (Path(".env.bff"), Path("server.env")),
            {
                "runtime_uid": 10001,
                "runtime_gid": 10001,
                "allow_extract": False,
                "allow_warehouse": False,
                "retain_results": False,
                "write_actions": (),
            },
        ),
        (
            (Path("private.env"), Path("other.env")),
            {
                "runtime_uid": 42,
                "runtime_gid": 43,
                "allow_extract": False,
                "allow_warehouse": False,
                "retain_results": True,
                "write_actions": ("create_contact", "update_invoice"),
            },
        ),
    ]


def test_cli_error_does_not_print_secret(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = source_file(tmp_path, "XERO_PKCE_CLIENT_SECRET=secret-sentinel\n")
    output = tmp_path / "server.env"

    result = utility.main(["--source-env", str(source), "--output-env", str(output)])

    captured = capsys.readouterr()
    assert result == 1
    assert "secret-sentinel" not in captured.out + captured.err
    assert not output.exists()
