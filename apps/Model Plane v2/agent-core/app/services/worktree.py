"""Git worktree service — create, list, and delete git worktrees.

Provides isolated working trees for parallel agent branches,
each mapped to its own directory so concurrent agents don't collide
on the working tree state.

Usage:
    wt = await WorktreeService.create("feature/my-branch")
    # ... agent works inside wt.path ...
    await WorktreeService.delete(wt.name)
"""

from __future__ import annotations

import asyncio
import logging
import re
import shutil
from dataclasses import dataclass
from pathlib import Path

logger = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Domain
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Worktree:
    """A git worktree entry."""

    name: str          # short identifier (branch-safe string)
    branch: str        # full branch name
    path: Path         # absolute filesystem path
    is_new_branch: bool = False


_SAFE_NAME_RE = re.compile(r"[^a-zA-Z0-9_\-]")


def _sanitise_name(name: str) -> str:
    """Return a branch-safe version of *name*."""
    safe = _SAFE_NAME_RE.sub("-", name.strip()).strip("-")
    if not safe:
        raise ValueError(f"Cannot derive a safe worktree name from: {name!r}")
    return safe[:64]


# ---------------------------------------------------------------------------
# Service
# ---------------------------------------------------------------------------


class WorktreeService:
    """Manages git worktrees via subprocess.

    All methods are async — they shell out to git and remain non-blocking.
    """

    def __init__(self, repo_root: str | Path) -> None:
        self._root = Path(repo_root).resolve()

    # ── Helpers ──────────────────────────────────────────────────────────

    async def _git(self, *args: str) -> str:
        """Run a git command in the repo root, return stripped stdout."""
        cmd = ["git", *args]
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            cwd=str(self._root),
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(
                f"git {' '.join(args)} failed (rc={proc.returncode}): "
                f"{stderr.decode('utf-8', errors='replace').strip()}"
            )
        return stdout.decode("utf-8", errors="replace").strip()

    def _worktree_path(self, name: str) -> Path:
        """Return the filesystem path for a worktree with the given name."""
        return self._root.parent / f".worktrees" / name

    # ── CRUD ─────────────────────────────────────────────────────────────

    async def create(
        self,
        name: str,
        *,
        branch: str | None = None,
        base: str = "HEAD",
        new_branch: bool = True,
    ) -> Worktree:
        """Create a new git worktree.

        Args:
            name: Human name for this worktree; sanitised into a safe string.
            branch: Branch name to use; defaults to the sanitised *name*.
            base: Commit/branch to base the new branch on.
            new_branch: If True, create a new branch. If False, checkout existing.

        Returns:
            Worktree record.
        """
        safe = _sanitise_name(name)
        branch = branch or safe
        wt_path = self._worktree_path(safe)

        if wt_path.exists():
            raise FileExistsError(f"Worktree directory already exists: {wt_path}")

        wt_path.parent.mkdir(parents=True, exist_ok=True)

        if new_branch:
            await self._git(
                "worktree", "add", "-b", branch, str(wt_path), base
            )
        else:
            await self._git(
                "worktree", "add", str(wt_path), branch
            )

        logger.info(
            "worktree_created",
            extra={"name": safe, "branch": branch, "path": str(wt_path)},
        )
        return Worktree(
            name=safe,
            branch=branch,
            path=wt_path,
            is_new_branch=new_branch,
        )

    async def list(self) -> list[Worktree]:
        """Return all worktrees registered with this repository."""
        try:
            output = await self._git("worktree", "list", "--porcelain")
        except RuntimeError:
            return []

        worktrees: list[Worktree] = []
        current: dict[str, str] = {}

        for line in output.splitlines():
            if line.startswith("worktree "):
                if current:
                    worktrees.append(_parse_worktree(current))
                current = {"path": line[len("worktree "):].strip()}
            elif line.startswith("branch "):
                current["branch"] = line[len("branch "):].strip()
            elif line == "":
                if current:
                    worktrees.append(_parse_worktree(current))
                    current = {}

        if current:
            worktrees.append(_parse_worktree(current))

        return worktrees

    async def delete(self, name: str, *, force: bool = False) -> None:
        """Remove a worktree by name.

        Args:
            name: Worktree name (as returned by ``create``).
            force: Pass --force to git worktree remove.
        """
        safe = _sanitise_name(name)
        wt_path = self._worktree_path(safe)
        args = ["worktree", "remove", str(wt_path)]
        if force:
            args.append("--force")
        try:
            await self._git(*args)
        except RuntimeError as exc:
            logger.warning("worktree_remove_git_failed", extra={"error": str(exc)})
            # Best-effort filesystem cleanup
            if wt_path.exists():
                shutil.rmtree(wt_path, ignore_errors=True)
                logger.info("worktree_fs_cleaned", extra={"path": str(wt_path)})
        logger.info("worktree_deleted", extra={"name": safe})

    async def prune(self) -> None:
        """Prune stale worktree references."""
        await self._git("worktree", "prune")
        logger.info("worktree_pruned")


def _parse_worktree(record: dict[str, str]) -> Worktree:
    path = Path(record.get("path", ""))
    branch = record.get("branch", "refs/heads/unknown")
    # refs/heads/feature/x → feature/x
    branch_short = branch.removeprefix("refs/heads/")
    name = path.name
    return Worktree(name=name, branch=branch_short, path=path)
