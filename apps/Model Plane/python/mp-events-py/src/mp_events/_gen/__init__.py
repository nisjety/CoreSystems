"""Generated protobuf bindings for the Model Plane contracts.

Everything under this package is emitted by `buf generate` (see
`apps/Model Plane/proto/buf.gen.yaml`, plugin `buf.build/protocolbuffers/python`,
`out: ../python/mp-events-py/src/mp_events/_gen`). Do not hand-edit the
`*_pb2.py` files — regenerate them instead.

`protoc-gen-python` emits cross-file imports rooted at the *proto* path, not at
the Python package that happens to contain the output. So
`model_plane/v1/sessions_pb2.py` contains:

    from model_plane.v1 import events_pb2

which only resolves if this directory — the generator's output root — is itself
an import root. Because the output is nested inside `mp_events._gen`, importing
`mp_events._gen.model_plane.v1.sessions_pb2` otherwise dies with
`ModuleNotFoundError: No module named 'model_plane'`, and every proto that
imports another proto (sessions, orchestration, execution, eventlog, gateway,
proof_bundle) is unreachable.

protoc has no option to emit package-relative imports, so the output root is
registered on `sys.path` here instead. This lives in `__init__.py` — a
hand-maintained file that protoc never writes — so it survives regeneration and
requires no post-processing of generated code.
"""

from __future__ import annotations

import os
import sys

# The generator output root: `.../src/mp_events/_gen`.
_GEN_ROOT = os.path.dirname(os.path.abspath(__file__))

if _GEN_ROOT not in sys.path:
    # Appended, not prepended: these are generic top-level names
    # (`model_plane`, `dataplane`) and must never shadow a real distribution.
    sys.path.append(_GEN_ROOT)
