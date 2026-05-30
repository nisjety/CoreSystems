"""LSPTool — LLM-invokable wrapper for the language server protocol manager.

Supports: diagnostics, hover, goto-definition, completion.
The LSPManager is lazy-started per language on first access.
"""

from __future__ import annotations

import logging
from typing import Any

from app.lsp.manager import LSPManager
from app.tools.base import ToolResult

logger = logging.getLogger(__name__)

# Injected at session startup via set_lsp_manager().
_lsp_manager: LSPManager | None = None


def set_lsp_manager(mgr: LSPManager) -> None:
    global _lsp_manager
    _lsp_manager = mgr


def _get_manager() -> LSPManager:
    global _lsp_manager
    if _lsp_manager is None:
        _lsp_manager = LSPManager()
    return _lsp_manager


class LSPTool:
    """Query language server features: diagnostics, hover, definition, completion."""

    name = "lsp"
    description = (
        "Query the language server for a file. "
        "Supported operations: 'diagnostics' (errors/warnings), 'hover' (type info), "
        "'definition' (go-to-definition), 'completion' (code completions at a position)."
    )
    input_schema: dict[str, Any] = {
        "type": "object",
        "properties": {
            "operation": {
                "type": "string",
                "enum": ["diagnostics", "hover", "definition", "completion"],
                "description": "LSP operation to perform.",
            },
            "file_path": {
                "type": "string",
                "description": "Absolute path to the file.",
            },
            "line": {
                "type": "integer",
                "description": "0-based line number (for hover/definition/completion).",
                "default": 0,
            },
            "column": {
                "type": "integer",
                "description": "0-based column/character offset (for hover/definition/completion).",
                "default": 0,
            },
        },
        "required": ["operation", "file_path"],
    }
    search_hint = "lsp language server diagnostics hover definition completion"
    should_defer = False

    def is_read_only(self) -> bool:
        return True

    def is_concurrent_safe(self) -> bool:
        return True

    def is_destructive(self) -> bool:
        return False

    def prompt(self) -> str:
        return (
            "Query the language server for type info, errors, and go-to-definition. "
            "Use 'diagnostics' to get errors/warnings before editing."
        )

    def validate_input(self, input_data: dict[str, Any]) -> dict[str, Any]:
        op = input_data.get("operation")
        valid_ops = {"diagnostics", "hover", "definition", "completion"}
        if op not in valid_ops:
            raise ValueError(f"Invalid operation '{op}'. Must be one of: {valid_ops}")
        if not input_data.get("file_path", "").strip():
            raise ValueError("'file_path' is required")
        return input_data

    async def call(self, input_data: dict[str, Any]) -> ToolResult:
        op = input_data["operation"]
        file_path = input_data["file_path"]
        line = input_data.get("line", 0)
        col = input_data.get("column", 0)

        mgr = _get_manager()
        instance = await mgr.get_or_start(file_path)
        if instance is None:
            return ToolResult(
                error=f"No LSP server available for file: {file_path}",
                metadata={"supported_languages": mgr.active_languages},
            )

        try:
            if op == "diagnostics":
                diags = await instance.diagnostics(file_path)
                return ToolResult(
                    output=f"{len(diags)} diagnostic(s) for {file_path}",
                    metadata={
                        "file": file_path,
                        "diagnostics": [d.model_dump() for d in diags],
                    },
                )
            elif op == "hover":
                result = await instance.hover(file_path, line, col)
                return ToolResult(
                    output=result or "(no hover info)",
                    metadata={"file": file_path, "line": line, "column": col},
                )
            elif op == "definition":
                locations = await instance.definition(file_path, line, col)
                return ToolResult(
                    output=f"{len(locations)} definition(s) found",
                    metadata={"locations": locations},
                )
            elif op == "completion":
                items = await instance.completion(file_path, line, col)
                return ToolResult(
                    output=f"{len(items)} completion(s)",
                    metadata={"completions": items[:20]},  # cap to 20
                )
            else:
                return ToolResult(error=f"Unhandled operation: {op}")
        except Exception as exc:
            logger.exception("lsp_tool_failed", extra={"op": op, "file": file_path})
            return ToolResult(error=f"LSP error ({op}): {exc}")
