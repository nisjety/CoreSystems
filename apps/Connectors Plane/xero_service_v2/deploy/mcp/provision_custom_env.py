#!/usr/bin/env python3
"""Provision private MCP pilot credentials from a single Xero custom connection."""

from __future__ import annotations

import argparse
import os
import re
import secrets
import stat
import sys
import tempfile
import uuid
from pathlib import Path


SOURCE_KEYS = (
    "XERO_ORG_1_CLIENT_ID",
    "XERO_ORG_1_CLIENT_SECRET",
    "XERO_ORG_1_TENANT_ID",
)
ASSIGNMENT = re.compile(r"\s*(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=(.*)\Z")
SAFE_ENV_VALUE = re.compile(r"[A-Za-z0-9._~+:/=@%-]+\Z", re.ASCII)
UNQUOTED_VALUE = re.compile(r"([^\s#'\"\\]+)(?:\s+#.*)?\s*\Z")
MAX_SOURCE_BYTES = 1_048_576
PUBLIC_HOST = re.compile(
    r"(?=.{1,253}\Z)[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?"
    r"(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+\Z",
    re.ASCII,
)


class ProvisionError(ValueError):
    """The source or requested output configuration is unsafe."""


def _parse_value(raw: str, key: str) -> str:
    value = raw.strip()
    if value.startswith(("'", '"')):
        quote = value[0]
        closing = value.find(quote, 1)
        if closing < 0:
            raise ProvisionError(f"{key} is malformed")
        suffix = value[closing + 1 :].strip()
        if suffix and not suffix.startswith("#"):
            raise ProvisionError(f"{key} is malformed")
        parsed = value[1:closing]
    else:
        match = UNQUOTED_VALUE.fullmatch(value)
        if match is None:
            raise ProvisionError(f"{key} is malformed")
        parsed = match.group(1)
    if SAFE_ENV_VALUE.fullmatch(parsed) is None:
        raise ProvisionError(f"{key} contains unsupported characters")
    return parsed


def _read_source(source: Path) -> str:
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(source, flags)
    try:
        metadata = os.fstat(fd)
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > MAX_SOURCE_BYTES:
            raise ProvisionError("source must be a regular file of at most 1 MiB")
        if metadata.st_mode & 0o077 or metadata.st_uid not in (0, os.geteuid()):
            raise ProvisionError("source permissions must restrict access to its owner")
        with os.fdopen(fd, "r", encoding="utf-8", closefd=False) as stream:
            content = stream.read(MAX_SOURCE_BYTES + 1)
        if len(content.encode("utf-8")) > MAX_SOURCE_BYTES:
            raise ProvisionError("source exceeds 1 MiB")
        return content
    except UnicodeError as exc:
        raise ProvisionError("source must be UTF-8") from exc
    finally:
        os.close(fd)


def _credentials(source: Path) -> dict[str, str]:
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
    tenant = values["XERO_ORG_1_TENANT_ID"]
    try:
        if str(uuid.UUID(tenant)) != tenant:
            raise ValueError
    except ValueError as exc:
        raise ProvisionError("XERO_ORG_1_TENANT_ID must be a canonical UUID") from exc
    return values


def _runtime_env(values: dict[str, str], token: str, allowed_host: str) -> bytes:
    tenant = values["XERO_ORG_1_TENANT_ID"]
    settings = {
        "XERO_AUTH_MODE": "custom",
        **values,
        "XERO_MCP_TENANTS": tenant,
        "XERO_MCP_TOKEN": token,
        "XERO_MCP_TLS_PROXY": "true",
        "XERO_MCP_TOOL_MODE": "compact",
        "XERO_MCP_ZERO_DATA_RETENTION": "true",
        "XERO_MCP_ALLOW_EXTRACT": "false",
        "XERO_MCP_ALLOW_SENSITIVE": "false",
        "XERO_MCP_ALLOW_WAREHOUSE": "false",
        "XERO_MCP_ALLOWED_HOSTS": f"{allowed_host},localhost,127.0.0.1",
        "XERO_MCP_STORAGE_ROOT": "/app/out/mcp",
        "XERO_MCP_BIND": "0.0.0.0:5003",
    }
    return "".join(f"{key}={value}\n" for key, value in settings.items()).encode("ascii")


def _stage(path: Path, content: bytes, uid: int, gid: int) -> Path:
    fd, name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(name)
    try:
        os.fchown(fd, uid, gid)
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "wb", closefd=False) as stream:
            stream.write(content)
            stream.flush()
            os.fsync(fd)
        return temporary
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise
    finally:
        os.close(fd)


def provision(
    source: Path,
    runtime_env: Path,
    client_token: Path,
    *,
    runtime_uid: int = 10001,
    runtime_gid: int = 10001,
    token_uid: int = 0,
    token_gid: int = 0,
    allowed_host: str,
) -> None:
    """Write a container-readable env and a separate root-readable client token."""
    source, runtime_env, client_token = map(Path, (source, runtime_env, client_token))
    if min(runtime_uid, runtime_gid, token_uid, token_gid) < 0:
        raise ProvisionError("UID and GID values must be non-negative")
    if PUBLIC_HOST.fullmatch(allowed_host) is None:
        raise ProvisionError("public hostname must be a valid lower-case DNS name")
    if len({path.resolve() for path in (source, runtime_env, client_token)}) != 3:
        raise ProvisionError("source and outputs must be distinct paths")
    for output in (runtime_env, client_token):
        if os.path.lexists(output):
            raise FileExistsError("output already exists")
        if not output.parent.is_dir():
            raise ProvisionError("output parent directory does not exist")
        if output.parent.stat().st_mode & 0o022:
            raise ProvisionError("output parent directory must not be group or world writable")

    values = _credentials(source)
    token = secrets.token_urlsafe(48)
    runtime_temp: Path | None = None
    token_temp: Path | None = None
    runtime_published = False
    try:
        runtime_temp = _stage(
            runtime_env, _runtime_env(values, token, allowed_host), runtime_uid, runtime_gid
        )
        token_temp = _stage(client_token, f"{token}\n".encode("ascii"), token_uid, token_gid)
        os.link(runtime_temp, runtime_env)
        runtime_published = True
        os.link(token_temp, client_token)
    except BaseException:
        if runtime_published and runtime_temp is not None:
            published = runtime_env.stat(follow_symlinks=False)
            staged = runtime_temp.stat(follow_symlinks=False)
            if (published.st_dev, published.st_ino) == (staged.st_dev, staged.st_ino):
                runtime_env.unlink()
        raise
    finally:
        if runtime_temp is not None:
            runtime_temp.unlink(missing_ok=True)
        if token_temp is not None:
            token_temp.unlink(missing_ok=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-env", required=True, type=Path)
    parser.add_argument("--output-env", required=True, type=Path)
    parser.add_argument("--token-file", required=True, type=Path)
    parser.add_argument("--uid", type=int, default=10001, help="container runtime file UID")
    parser.add_argument("--gid", type=int, default=10001, help="container runtime file GID")
    parser.add_argument("--allowed-host", required=True, help="approved public MCP hostname")
    args = parser.parse_args(argv)
    try:
        provision(
            args.source_env,
            args.output_env,
            args.token_file,
            runtime_uid=args.uid,
            runtime_gid=args.gid,
            allowed_host=args.allowed_host,
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
