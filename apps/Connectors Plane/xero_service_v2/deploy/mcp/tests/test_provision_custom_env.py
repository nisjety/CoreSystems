import os
import stat
from pathlib import Path

import pytest

from deploy.mcp import provision_custom_env as utility
from deploy.mcp.provision_custom_env import ProvisionError, provision


TENANT_ID = "804fa45e-c7de-4c0b-bd30-786b17b53bb3"
CLIENT_ID = "pilot-client-id"
CLIENT_SECRET = "fake-client-secret-for-tests"


def source_file(tmp_path: Path, content: str | None = None) -> Path:
    source = tmp_path / "source.fixture"
    source.write_text(
        content
        if content is not None
        else (
            f'export XERO_ORG_1_CLIENT_ID="{CLIENT_ID}"\n'
            f"XERO_ORG_1_CLIENT_SECRET='{CLIENT_SECRET}'\n"
            f"XERO_ORG_1_TENANT_ID={TENANT_ID} # pilot\n"
            "XERO_ORG_2_CLIENT_SECRET=not-for-this-pilot\n"
            "XERO_MCP_TOKEN=attacker-token\n"
        ),
        encoding="utf-8",
    )
    source.chmod(0o600)
    return source


def paths(tmp_path: Path) -> tuple[Path, Path]:
    return tmp_path / "runtime.env", tmp_path / "client.token"


def run_provision(source: Path, runtime: Path, token_file: Path) -> None:
    provision(
        source,
        runtime,
        token_file,
        runtime_uid=os.getuid(),
        runtime_gid=os.getgid(),
        token_uid=os.getuid(),
        token_gid=os.getgid(),
        allowed_host="tools.aquatiq.com",
    )


def read_runtime(path: Path) -> dict[str, str]:
    return dict(line.split("=", 1) for line in path.read_text(encoding="utf-8").splitlines())


def test_provisions_one_tenant_read_only_pilot_with_private_files(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)

    run_provision(source, runtime, token_file)

    env = read_runtime(runtime)
    assert env == {
        "XERO_AUTH_MODE": "custom",
        "XERO_ORG_1_CLIENT_ID": CLIENT_ID,
        "XERO_ORG_1_CLIENT_SECRET": CLIENT_SECRET,
        "XERO_ORG_1_TENANT_ID": TENANT_ID,
        "XERO_MCP_TENANTS": TENANT_ID,
        "XERO_MCP_TOKEN": env["XERO_MCP_TOKEN"],
        "XERO_MCP_TLS_PROXY": "true",
        "XERO_MCP_TOOL_MODE": "compact",
        "XERO_MCP_ZERO_DATA_RETENTION": "true",
        "XERO_MCP_ALLOW_EXTRACT": "false",
        "XERO_MCP_ALLOW_SENSITIVE": "false",
        "XERO_MCP_ALLOW_WAREHOUSE": "false",
        "XERO_MCP_ALLOWED_HOSTS": "tools.aquatiq.com,localhost,127.0.0.1",
        "XERO_MCP_STORAGE_ROOT": "/app/out/mcp",
        "XERO_MCP_BIND": "0.0.0.0:5003",
    }
    assert "not-for-this-pilot" not in runtime.read_text(encoding="utf-8")
    assert env["XERO_MCP_TOKEN"] != "attacker-token"
    token = token_file.read_text(encoding="ascii").strip()
    assert len(token) >= 32
    assert token == env["XERO_MCP_TOKEN"]
    assert stat.S_IMODE(runtime.stat().st_mode) == 0o600
    assert stat.S_IMODE(token_file.stat().st_mode) == 0o600
    assert (runtime.stat().st_uid, runtime.stat().st_gid) == (os.getuid(), os.getgid())
    assert (token_file.stat().st_uid, token_file.stat().st_gid) == (os.getuid(), os.getgid())


def test_two_provisions_generate_different_tokens(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    runtime_a, token_a = paths(tmp_path)
    runtime_b, token_b = tmp_path / "runtime-b.env", tmp_path / "client-b.token"

    run_provision(source, runtime_a, token_a)
    run_provision(source, runtime_b, token_b)

    assert token_a.read_bytes() != token_b.read_bytes()


def test_selected_hostname_is_written_to_runtime_allowlist(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)

    provision(
        source,
        runtime,
        token_file,
        runtime_uid=os.getuid(),
        runtime_gid=os.getgid(),
        token_uid=os.getuid(),
        token_gid=os.getgid(),
        allowed_host="mcp.example.com",
    )

    assert read_runtime(runtime)["XERO_MCP_ALLOWED_HOSTS"] == (
        "mcp.example.com,localhost,127.0.0.1"
    )


@pytest.mark.parametrize("existing", ["runtime", "token"])
def test_existing_output_is_preserved_without_creating_other_output(
    tmp_path: Path, existing: str
) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)
    existing_path = runtime if existing == "runtime" else token_file
    existing_path.write_text("keep-this", encoding="ascii")

    with pytest.raises(FileExistsError):
        run_provision(source, runtime, token_file)

    assert existing_path.read_text(encoding="ascii") == "keep-this"
    assert not (token_file if existing == "runtime" else runtime).exists()


@pytest.mark.parametrize(
    "content",
    [
        f"XERO_ORG_1_CLIENT_ID={CLIENT_ID}\nXERO_ORG_1_TENANT_ID={TENANT_ID}\n",
        f"XERO_ORG_1_CLIENT_ID=\nXERO_ORG_1_CLIENT_SECRET={CLIENT_SECRET}\nXERO_ORG_1_TENANT_ID={TENANT_ID}\n",
        f"XERO_ORG_1_CLIENT_ID={CLIENT_ID}\nXERO_ORG_1_CLIENT_SECRET='unterminated\nXERO_ORG_1_TENANT_ID={TENANT_ID}\n",
        f"XERO_ORG_1_CLIENT_ID={CLIENT_ID}\nXERO_ORG_1_CLIENT_SECRET={CLIENT_SECRET}\nXERO_ORG_1_TENANT_ID=not-a-uuid\n",
        f"XERO_ORG_1_CLIENT_ID={CLIENT_ID}\nXERO_ORG_1_CLIENT_ID=other\nXERO_ORG_1_CLIENT_SECRET={CLIENT_SECRET}\nXERO_ORG_1_TENANT_ID={TENANT_ID}\n",
        f"XERO_ORG_1_CLIENT_ID={CLIENT_ID}\nXERO_ORG_1_CLIENT_SECRET='bad value'\nXERO_ORG_1_TENANT_ID={TENANT_ID}\n",
    ],
)
def test_invalid_source_fails_without_outputs(tmp_path: Path, content: str) -> None:
    source = source_file(tmp_path, content)
    runtime, token_file = paths(tmp_path)

    with pytest.raises(ProvisionError):
        run_provision(source, runtime, token_file)

    assert not runtime.exists()
    assert not token_file.exists()


def test_output_symlink_is_not_followed(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)
    victim = tmp_path / "victim"
    victim.write_text("keep-this", encoding="ascii")
    runtime.symlink_to(victim)

    with pytest.raises(FileExistsError):
        run_provision(source, runtime, token_file)

    assert victim.read_text(encoding="ascii") == "keep-this"
    assert not token_file.exists()


def test_insecure_source_permissions_are_rejected(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    source.chmod(0o644)
    runtime, token_file = paths(tmp_path)

    with pytest.raises(ProvisionError, match="source permissions"):
        run_provision(source, runtime, token_file)

    assert not runtime.exists()
    assert not token_file.exists()


@pytest.mark.parametrize(
    "hostname", ["bad,host", "example.com\\nINJECT=1", "localhost", "-bad.example"]
)
def test_invalid_public_host_is_rejected(tmp_path: Path, hostname: str) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)

    with pytest.raises(ProvisionError, match="hostname"):
        provision(
            source,
            runtime,
            token_file,
            runtime_uid=os.getuid(),
            runtime_gid=os.getgid(),
            token_uid=os.getuid(),
            token_gid=os.getgid(),
            allowed_host=hostname,
        )

    assert not runtime.exists()
    assert not token_file.exists()


def test_second_publish_failure_removes_only_new_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)
    original_link = os.link
    calls = 0

    def fail_second_link(*args: object, **kwargs: object) -> None:
        nonlocal calls
        calls += 1
        if calls == 2:
            raise OSError("simulated publish failure")
        original_link(*args, **kwargs)

    monkeypatch.setattr(os, "link", fail_second_link)

    with pytest.raises(OSError, match="simulated publish failure"):
        run_provision(source, runtime, token_file)

    assert not runtime.exists()
    assert not token_file.exists()
    assert sorted(path.name for path in tmp_path.iterdir()) == [source.name]


def test_cli_uses_requested_flags_and_container_owner_defaults(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)
    call: dict[str, object] = {}

    def capture(*args: object, **kwargs: object) -> None:
        call["args"] = args
        call["kwargs"] = kwargs

    monkeypatch.setattr(utility, "provision", capture)
    result = utility.main(
        [
            "--source-env",
            str(source),
            "--output-env",
            str(runtime),
            "--token-file",
            str(token_file),
            "--allowed-host",
            "tools.aquatiq.com",
        ]
    )

    assert result == 0
    assert call == {
        "args": (source, runtime, token_file),
        "kwargs": {"runtime_uid": 10001, "runtime_gid": 10001, "allowed_host": "tools.aquatiq.com"},
    }

    result = utility.main(
        [
            "--source-env",
            str(source),
            "--output-env",
            str(runtime),
            "--token-file",
            str(token_file),
            "--uid",
            str(os.getuid()),
            "--gid",
            str(os.getgid()),
            "--allowed-host",
            "mcp.example.com",
        ]
    )

    assert result == 0
    assert call["kwargs"] == {
        "runtime_uid": os.getuid(),
        "runtime_gid": os.getgid(),
        "allowed_host": "mcp.example.com",
    }


def test_default_token_ownership_is_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    source = source_file(tmp_path)
    runtime, token_file = paths(tmp_path)
    requested_owners: list[tuple[int, int]] = []
    original_fchown = os.fchown

    def capture_owner(fd: int, uid: int, gid: int) -> None:
        requested_owners.append((uid, gid))
        if (uid, gid) == (os.getuid(), os.getgid()):
            original_fchown(fd, uid, gid)

    monkeypatch.setattr(os, "fchown", capture_owner)
    provision(
        source,
        runtime,
        token_file,
        runtime_uid=os.getuid(),
        runtime_gid=os.getgid(),
        allowed_host="tools.aquatiq.com",
    )

    assert requested_owners == [(os.getuid(), os.getgid()), (0, 0)]


def test_cli_error_does_not_print_source_secret(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    source = source_file(tmp_path, "XERO_ORG_1_CLIENT_SECRET=source-secret-sentinel\n")
    runtime, token_file = paths(tmp_path)

    result = utility.main(
        [
            "--source-env",
            str(source),
            "--output-env",
            str(runtime),
            "--token-file",
            str(token_file),
            "--allowed-host",
            "tools.aquatiq.com",
        ]
    )

    captured = capsys.readouterr()
    assert result == 1
    assert "source-secret-sentinel" not in captured.out + captured.err
    assert not runtime.exists()
    assert not token_file.exists()


def test_rejects_world_writable_output_directory(tmp_path: Path) -> None:
    source = source_file(tmp_path)
    output_dir = tmp_path / "output"
    output_dir.mkdir(mode=0o777)
    output_dir.chmod(0o777)
    runtime, token_file = output_dir / "runtime.env", output_dir / "client.token"

    with pytest.raises(ProvisionError, match="directory must not be"):
        run_provision(source, runtime, token_file)

    assert not runtime.exists()
    assert not token_file.exists()
