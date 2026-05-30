from __future__ import annotations

import importlib
import sys
from pathlib import Path
from types import ModuleType

import grpc_tools
from grpc_tools import protoc


PROTO_FILES = ("documents.proto", "knowledge.proto", "auth.proto", "org_access.proto")
PROTO_GEN_DIR = Path(__file__).resolve().parent / "proto_gen"


def _find_proto_dir() -> Path:
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
    expected_outputs = (
        PROTO_GEN_DIR / "documents_pb2.py",
        PROTO_GEN_DIR / "documents_pb2_grpc.py",
        PROTO_GEN_DIR / "knowledge_pb2.py",
        PROTO_GEN_DIR / "knowledge_pb2_grpc.py",
    )
    if all(path.exists() for path in expected_outputs):
        _ensure_proto_path()
        return

    proto_dir = _find_proto_dir()
    grpc_include = Path(grpc_tools.__file__).resolve().parent / "_proto"

    PROTO_GEN_DIR.mkdir(parents=True, exist_ok=True)

    result = protoc.main(
        [
            "grpc_tools.protoc",
            f"-I{proto_dir}",
            f"-I{grpc_include}",
            f"--python_out={PROTO_GEN_DIR}",
            f"--grpc_python_out={PROTO_GEN_DIR}",
            *(str(proto_dir / proto_name) for proto_name in PROTO_FILES),
        ]
    )
    if result != 0:
        raise RuntimeError(f"Failed to generate gRPC stubs (exit code {result})")

    _ensure_proto_path()
    importlib.invalidate_caches()


def load_documents_proto_modules() -> tuple[ModuleType, ModuleType]:
    ensure_proto_generated()
    return (
        importlib.import_module("documents_pb2"),
        importlib.import_module("documents_pb2_grpc"),
    )