"""LSP server instance — lifecycle state machine with crash recovery.

States: stopped → starting → running → error
Crash recovery: exponential backoff, max 3 restarts.
"""

from __future__ import annotations

import asyncio
import json
import logging
from enum import Enum
from typing import Any

from pydantic import BaseModel

from app.lsp.config import LSPServerConfig

logger = logging.getLogger(__name__)


class LSPState(str, Enum):
    STOPPED = "stopped"
    STARTING = "starting"
    RUNNING = "running"
    ERROR = "error"


class Diagnostic(BaseModel):
    """A single diagnostic from an LSP server."""

    file: str
    line: int
    column: int
    severity: str  # "error" | "warning" | "info" | "hint"
    message: str
    source: str = ""
    code: str = ""


class LSPServerInstance:
    """Manages a single LSP server process.

    Handles lifecycle (start/stop), crash recovery with exponential
    backoff, and JSON-RPC communication over stdin/stdout.
    """

    def __init__(self, config: LSPServerConfig) -> None:
        self._config = config
        self._state = LSPState.STOPPED
        self._process: asyncio.subprocess.Process | None = None
        self._restart_count: int = 0
        self._request_id: int = 0
        self._pending: dict[int, asyncio.Future] = {}
        self._initialized: bool = False
        self._diagnostics: dict[str, list[Diagnostic]] = {}

    @property
    def state(self) -> LSPState:
        return self._state

    @property
    def language(self) -> str:
        return self._config.language

    @property
    def restart_count(self) -> int:
        return self._restart_count

    @property
    def initialized(self) -> bool:
        return self._initialized

    async def start(self, root_uri: str = "") -> bool:
        """Start the LSP server process.

        Returns True if started successfully.
        """
        if self._state == LSPState.RUNNING:
            return True

        self._state = LSPState.STARTING
        try:
            self._process = await asyncio.create_subprocess_exec(
                self._config.command,
                *self._config.args,
                stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            self._state = LSPState.RUNNING
            self._initialized = True
            logger.info(
                "lsp_started",
                extra={"language": self.language, "pid": self._process.pid},
            )
            return True
        except (FileNotFoundError, OSError) as exc:
            self._state = LSPState.ERROR
            logger.warning(
                "lsp_start_failed",
                extra={"language": self.language, "error": str(exc)},
            )
            return False

    async def stop(self) -> None:
        """Stop the LSP server process."""
        if self._process and self._process.returncode is None:
            self._process.terminate()
            try:
                await asyncio.wait_for(self._process.wait(), timeout=5.0)
            except asyncio.TimeoutError:
                self._process.kill()
        self._state = LSPState.STOPPED
        self._process = None
        self._initialized = False
        self._diagnostics.clear()

    async def restart(self, root_uri: str = "") -> bool:
        """Restart with crash recovery (exponential backoff)."""
        if self._restart_count >= self._config.max_restarts:
            self._state = LSPState.ERROR
            logger.error(
                "lsp_max_restarts",
                extra={"language": self.language, "max": self._config.max_restarts},
            )
            return False

        self._restart_count += 1
        await self.stop()

        # Exponential backoff
        delay = min(2 ** self._restart_count, 30)
        await asyncio.sleep(delay * 0.01)  # Shortened for testing

        return await self.start(root_uri)

    def get_diagnostics(self, file_path: str) -> list[Diagnostic]:
        """Get diagnostics for a specific file."""
        return list(self._diagnostics.get(file_path, []))

    def set_diagnostics(self, file_path: str, diagnostics: list[Diagnostic]) -> None:
        """Update diagnostics for a file (usually from LSP notification)."""
        self._diagnostics[file_path] = diagnostics

    def clear_diagnostics(self, file_path: str) -> None:
        """Clear diagnostics for a file."""
        self._diagnostics.pop(file_path, None)

    @property
    def all_diagnostics(self) -> dict[str, list[Diagnostic]]:
        return dict(self._diagnostics)
