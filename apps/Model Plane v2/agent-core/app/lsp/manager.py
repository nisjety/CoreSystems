"""LSP server manager — routes files to the right LSP instance.

Manages N LSP server instances, one per language. Lazy-starts
servers on first access by file extension.
"""

from __future__ import annotations

import logging
from pathlib import PurePosixPath
from typing import Any

from app.lsp.config import (
    DEFAULT_LSP_CONFIGS,
    LSPServerConfig,
    get_config_for_extension,
)
from app.lsp.instance import Diagnostic, LSPServerInstance, LSPState

logger = logging.getLogger(__name__)


class LSPManager:
    """Manages multiple LSP server instances.

    Features:
    - Lazy-start: servers are started on first access
    - Extension routing: maps file extensions to language servers
    - Crash recovery: restarts failed servers
    - Aggregated diagnostics across all servers
    """

    def __init__(
        self,
        configs: list[LSPServerConfig] | None = None,
        root_uri: str = "",
    ) -> None:
        self._configs = configs or DEFAULT_LSP_CONFIGS
        self._instances: dict[str, LSPServerInstance] = {}
        self._root_uri = root_uri

    @property
    def active_languages(self) -> list[str]:
        """List languages with running servers."""
        return [
            lang
            for lang, inst in self._instances.items()
            if inst.state == LSPState.RUNNING
        ]

    def get_instance(self, language: str) -> LSPServerInstance | None:
        """Get the server instance for a language."""
        return self._instances.get(language)

    async def get_or_start(self, file_path: str) -> LSPServerInstance | None:
        """Get or lazy-start the appropriate LSP server for a file.

        Returns None if no LSP config matches the file extension.
        """
        ext = PurePosixPath(file_path).suffix
        config = get_config_for_extension(ext, self._configs)
        if config is None:
            return None

        lang = config.language
        if lang in self._instances:
            inst = self._instances[lang]
            if inst.state == LSPState.RUNNING:
                return inst
            if inst.state == LSPState.ERROR:
                ok = await inst.restart(self._root_uri)
                return inst if ok else None

        inst = LSPServerInstance(config)
        ok = await inst.start(self._root_uri)
        if ok:
            self._instances[lang] = inst
            return inst
        return None

    async def get_diagnostics(self, file_path: str) -> list[Diagnostic]:
        """Get diagnostics for a file from the appropriate server."""
        ext = PurePosixPath(file_path).suffix
        config = get_config_for_extension(ext, self._configs)
        if config is None:
            return []
        inst = self._instances.get(config.language)
        if inst is None:
            return []
        return inst.get_diagnostics(file_path)

    async def get_all_diagnostics(self) -> dict[str, list[Diagnostic]]:
        """Get diagnostics from all running servers."""
        result: dict[str, list[Diagnostic]] = {}
        for inst in self._instances.values():
            if inst.state == LSPState.RUNNING:
                result.update(inst.all_diagnostics)
        return result

    async def stop_all(self) -> None:
        """Stop all running LSP servers."""
        for inst in self._instances.values():
            await inst.stop()
        self._instances.clear()

    async def stop(self, language: str) -> None:
        """Stop a specific language server."""
        inst = self._instances.pop(language, None)
        if inst:
            await inst.stop()
