"""Load the hyphen-named scripts of apps/Data Plane v2/scripts as modules, for their tests.

The scripts are command-line tools (eval-gate.py, eval-ingest-corpus.py, ...), so none of them
is importable by its name. Tests load them by path and call their functions directly, or call
main(argv) the way the command line does.
"""
from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
PLANE = SCRIPTS.parent
REPO = PLANE.parents[1]
FIXTURES = Path(__file__).resolve().parent / "fixtures"

_cache: dict[str, object] = {}


def load(filename: str):
    """Import scripts/<filename> once and return the module."""
    if filename in _cache:
        return _cache[filename]
    path = SCRIPTS / filename
    name = "dpv2_" + filename.replace("-", "_").removesuffix(".py")
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    _cache[filename] = module
    return module
