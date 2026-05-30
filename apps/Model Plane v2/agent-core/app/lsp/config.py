"""LSP configuration — language → server binary mapping."""

from __future__ import annotations

from pydantic import BaseModel, Field


class LSPServerConfig(BaseModel):
    """Configuration for a single LSP server."""

    language: str
    extensions: list[str] = Field(default_factory=list)
    command: str = ""
    args: list[str] = Field(default_factory=list)
    initialization_options: dict = Field(default_factory=dict)
    max_restarts: int = 3
    enabled: bool = True


# Default LSP server configurations
DEFAULT_LSP_CONFIGS: list[LSPServerConfig] = [
    LSPServerConfig(
        language="python",
        extensions=[".py", ".pyi"],
        command="pyright-langserver",
        args=["--stdio"],
    ),
    LSPServerConfig(
        language="typescript",
        extensions=[".ts", ".tsx", ".js", ".jsx"],
        command="typescript-language-server",
        args=["--stdio"],
    ),
    LSPServerConfig(
        language="go",
        extensions=[".go"],
        command="gopls",
        args=["serve"],
    ),
    LSPServerConfig(
        language="rust",
        extensions=[".rs"],
        command="rust-analyzer",
    ),
    LSPServerConfig(
        language="c",
        extensions=[".c", ".h", ".cpp", ".hpp", ".cc"],
        command="clangd",
    ),
    LSPServerConfig(
        language="java",
        extensions=[".java"],
        command="jdtls",
        enabled=False,  # requires complex setup
    ),
]


def get_config_for_extension(
    ext: str,
    configs: list[LSPServerConfig] | None = None,
) -> LSPServerConfig | None:
    """Find the LSP config for a file extension."""
    for cfg in configs or DEFAULT_LSP_CONFIGS:
        if cfg.enabled and ext in cfg.extensions:
            return cfg
    return None
