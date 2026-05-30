"""
Proto compilation and module loading for cross-plane gRPC communication.

Compiles proto files (auth.proto, org_access.proto) at startup time
and returns the generated pb2 / pb2_grpc modules.
"""
from __future__ import annotations

import importlib
import sys
from pathlib import Path
from types import ModuleType

import grpc_tools
from grpc_tools import protoc

PROTO_FILES = ("auth.proto", "org_access.proto")
PROTO_GEN_DIR = Path(__file__).resolve().parent / "proto_gen"


def _find_proto_dir() -> Path:
    """Walk up from this file to find the shared proto/ directory."""
    for parent in Path(__file__).resolve().parents:
        candidate = parent / "proto"
        if candidate.exists():
            return candidate
    raise RuntimeError("Could not locate the shared proto directory")


def _ensure_proto_path() -> None:
    proto_gen_path = str(PROTO_GEN_DIR)
    if proto_gen_path not in sys.path:
        sys.path.insert(0, proto_gen_path)


def ensure_proto_generated() -> None:
    """Compile proto files if not already generated."""
    expected = []
    for pf in PROTO_FILES:
        stem = pf.replace(".proto", "")
        expected.append(PROTO_GEN_DIR / f"{stem}_pb2.py")
        expected.append(PROTO_GEN_DIR / f"{stem}_pb2_grpc.py")

    if all(p.exists() for p in expected):
        _ensure_proto_path()
        return

    proto_dir = _find_proto_dir()
    grpc_include = Path(grpc_tools.__file__).resolve().parent / "_proto"

    PROTO_GEN_DIR.mkdir(parents=True, exist_ok=True)

    for proto_file in PROTO_FILES:
        result = protoc.main([
            "grpc_tools.protoc",
            f"-I{proto_dir}",
            f"-I{grpc_include}",
            f"--python_out={PROTO_GEN_DIR}",
            f"--grpc_python_out={PROTO_GEN_DIR}",
            str(proto_dir / proto_file),
        ])
        if result != 0:
            raise RuntimeError(f"protoc failed for {proto_file} (code {result})")

    _ensure_proto_path()


def _load_module(name: str) -> ModuleType:
    ensure_proto_generated()
    if name in sys.modules:
        return sys.modules[name]
    return importlib.import_module(name)


def load_auth_proto_modules() -> tuple[ModuleType, ModuleType]:
    """Return (auth_pb2, auth_pb2_grpc) modules."""
    return _load_module("auth_pb2"), _load_module("auth_pb2_grpc")


def load_org_access_proto_modules() -> tuple[ModuleType, ModuleType]:
    """Return (org_access_pb2, org_access_pb2_grpc) modules."""
    return _load_module("org_access_pb2"), _load_module("org_access_pb2_grpc")
