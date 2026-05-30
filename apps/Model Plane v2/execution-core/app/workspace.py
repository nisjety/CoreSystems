"""Workspace bootstrap — creates and manages workspace-scoped directories.

Each runner task executes within a workspace directory:
  {WORKSPACE_BASE_PATH}/{workspace_id}/

The directory is created on first use and cleaned up when all tasks for
that workspace are complete (or on explicit cleanup request).
"""

from __future__ import annotations

import logging
import os
import shutil
from pathlib import Path

from app.config import settings

logger = logging.getLogger(__name__)


def workspace_path(workspace_id: str) -> Path:
    """Return the root path for a workspace."""
    # Prevent path traversal
    safe_id = workspace_id.replace("/", "_").replace("..", "_")
    return Path(settings.workspace_base_path) / safe_id


def ensure_workspace(workspace_id: str) -> Path:
    """Create the workspace directory if it doesn't exist. Returns the path."""
    wpath = workspace_path(workspace_id)
    wpath.mkdir(parents=True, exist_ok=True)
    logger.info("workspace_ensured", extra={"workspace_id": workspace_id, "path": str(wpath)})
    return wpath


def cleanup_workspace(workspace_id: str) -> bool:
    """Remove the workspace directory and all its contents.

    Returns True if the directory existed and was removed.
    """
    wpath = workspace_path(workspace_id)
    if wpath.exists():
        shutil.rmtree(wpath)
        logger.info("workspace_cleaned", extra={"workspace_id": workspace_id})
        return True
    return False


def workspace_disk_usage(workspace_id: str) -> int:
    """Return total disk usage in bytes for a workspace directory."""
    wpath = workspace_path(workspace_id)
    if not wpath.exists():
        return 0
    total = 0
    for dirpath, _dirnames, filenames in os.walk(wpath):
        for f in filenames:
            fp = Path(dirpath) / f
            try:
                total += fp.stat().st_size
            except OSError:
                pass
    return total


def list_workspace_files(workspace_id: str, max_files: int = 500) -> list[str]:
    """Return a list of relative file paths within a workspace."""
    wpath = workspace_path(workspace_id)
    if not wpath.exists():
        return []
    files: list[str] = []
    for dirpath, _dirnames, filenames in os.walk(wpath):
        for f in filenames:
            fp = Path(dirpath) / f
            files.append(str(fp.relative_to(wpath)))
            if len(files) >= max_files:
                return files
    return files
