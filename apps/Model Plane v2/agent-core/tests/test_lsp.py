"""Tests for Phase C4: LSP Integration."""

from __future__ import annotations

import pytest

from app.lsp.config import (
    DEFAULT_LSP_CONFIGS,
    LSPServerConfig,
    get_config_for_extension,
)
from app.lsp.instance import Diagnostic, LSPServerInstance, LSPState
from app.lsp.manager import LSPManager


# ── Config ─────────────────────────────────────────────────────

class TestLSPConfig:
    def test_default_configs_exist(self):
        assert len(DEFAULT_LSP_CONFIGS) >= 5

    def test_get_config_for_py(self):
        cfg = get_config_for_extension(".py")
        assert cfg is not None
        assert cfg.language == "python"

    def test_get_config_for_ts(self):
        cfg = get_config_for_extension(".ts")
        assert cfg is not None
        assert cfg.language == "typescript"

    def test_get_config_for_unknown(self):
        assert get_config_for_extension(".xyz") is None

    def test_disabled_config_skipped(self):
        configs = [
            LSPServerConfig(language="test", extensions=[".test"], enabled=False),
        ]
        assert get_config_for_extension(".test", configs) is None


# ── Instance ───────────────────────────────────────────────────

class TestLSPInstance:
    def test_initial_state(self):
        cfg = LSPServerConfig(language="python", command="pyright-langserver")
        inst = LSPServerInstance(cfg)
        assert inst.state == LSPState.STOPPED
        assert inst.language == "python"
        assert inst.restart_count == 0

    def test_diagnostics(self):
        cfg = LSPServerConfig(language="python", command="pyright-langserver")
        inst = LSPServerInstance(cfg)
        diag = Diagnostic(
            file="test.py",
            line=10,
            column=5,
            severity="error",
            message="Type error",
            source="pyright",
        )
        inst.set_diagnostics("test.py", [diag])
        result = inst.get_diagnostics("test.py")
        assert len(result) == 1
        assert result[0].message == "Type error"

    def test_clear_diagnostics(self):
        cfg = LSPServerConfig(language="python", command="pyright-langserver")
        inst = LSPServerInstance(cfg)
        inst.set_diagnostics("test.py", [
            Diagnostic(file="test.py", line=1, column=1, severity="warning", message="x"),
        ])
        inst.clear_diagnostics("test.py")
        assert inst.get_diagnostics("test.py") == []

    def test_all_diagnostics(self):
        cfg = LSPServerConfig(language="python", command="pyright-langserver")
        inst = LSPServerInstance(cfg)
        inst.set_diagnostics("a.py", [
            Diagnostic(file="a.py", line=1, column=1, severity="error", message="err"),
        ])
        inst.set_diagnostics("b.py", [])
        all_d = inst.all_diagnostics
        assert "a.py" in all_d
        assert "b.py" in all_d

    @pytest.mark.asyncio
    async def test_stop_from_stopped(self):
        cfg = LSPServerConfig(language="python", command="pyright-langserver")
        inst = LSPServerInstance(cfg)
        await inst.stop()  # Should not raise
        assert inst.state == LSPState.STOPPED

    @pytest.mark.asyncio
    async def test_start_missing_binary(self):
        cfg = LSPServerConfig(
            language="test", command="nonexistent-binary-xxxxx"
        )
        inst = LSPServerInstance(cfg)
        ok = await inst.start()
        assert ok is False
        assert inst.state == LSPState.ERROR

    @pytest.mark.asyncio
    async def test_restart_limit(self):
        cfg = LSPServerConfig(
            language="test",
            command="nonexistent-binary-xxxxx",
            max_restarts=2,
        )
        inst = LSPServerInstance(cfg)
        # Exhaust restarts
        for _ in range(3):
            await inst.restart()
        assert inst.state == LSPState.ERROR
        assert inst.restart_count >= 2


# ── Manager ────────────────────────────────────────────────────

class TestLSPManager:
    def _make_config(self) -> list[LSPServerConfig]:
        return [
            LSPServerConfig(
                language="test",
                extensions=[".tst"],
                command="echo",
                args=["hello"],
            ),
        ]

    def test_no_active_initially(self):
        mgr = LSPManager(configs=self._make_config())
        assert mgr.active_languages == []

    def test_get_instance_none(self):
        mgr = LSPManager(configs=self._make_config())
        assert mgr.get_instance("test") is None

    @pytest.mark.asyncio
    async def test_get_diagnostics_no_server(self):
        mgr = LSPManager(configs=self._make_config())
        diags = await mgr.get_diagnostics("file.tst")
        assert diags == []

    @pytest.mark.asyncio
    async def test_stop_all_empty(self):
        mgr = LSPManager(configs=self._make_config())
        await mgr.stop_all()  # No-op, should not raise

    @pytest.mark.asyncio
    async def test_get_or_start_unknown_ext(self):
        mgr = LSPManager(configs=self._make_config())
        inst = await mgr.get_or_start("file.xyz")
        assert inst is None
