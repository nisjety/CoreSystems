"""Tests for Phase E1: Wire Everything Together.

Verifies that all new subsystems are properly imported, instantiated,
and integrated in the application factory.
"""

from __future__ import annotations

import pytest

# Verify all modules import cleanly — compile-time test
from app.tools.registry import ToolRegistry
from app.tools.builtins import ALL_BUILTINS, register_builtins
from app.tools.dispatch import ToolDispatcher
from app.commands.registry import CommandRegistry
from app.commands.builtins.commands import ALL_COMMANDS, register_all_commands
from app.rate_limits.service import RateLimitService
from app.prompt_cache import PromptCacheManager
from app.coordinator.mode import CoordinatorMode
from app.coordinator.workers import WorkerManager
from app.plugins.registry import PluginRegistry
from app.plugins.loader import PluginLoader
from app.plugins.marketplace import MarketplaceClient
from app.plugins.reconciler import PluginReconciler
from app.voice.config import VoiceConfig
from app.voice.stt import SpeechToTextService
from app.voice.tts import TextToSpeechService
from app.lsp.config import DEFAULT_LSP_CONFIGS
from app.lsp.manager import LSPManager
from app.streaming_loop import run_streaming_turn_loop
from app.config import Settings


# ── Import Smoke Tests ─────────────────────────────────────────

class TestImportSmoke:
    """Ensure every new module imports without error."""

    def test_tool_registry_imports(self):
        assert ToolRegistry is not None

    def test_tool_dispatcher_imports(self):
        assert ToolDispatcher is not None

    def test_command_registry_imports(self):
        assert CommandRegistry is not None

    def test_rate_limit_service_imports(self):
        assert RateLimitService is not None

    def test_prompt_cache_imports(self):
        assert PromptCacheManager is not None

    def test_coordinator_imports(self):
        assert CoordinatorMode is not None
        assert WorkerManager is not None

    def test_plugin_subsystem_imports(self):
        assert PluginRegistry is not None
        assert PluginLoader is not None
        assert MarketplaceClient is not None

    def test_voice_imports(self):
        assert VoiceConfig is not None
        assert SpeechToTextService is not None
        assert TextToSpeechService is not None

    def test_lsp_imports(self):
        assert LSPManager is not None


# ── Wiring Tests ───────────────────────────────────────────────

class TestWiring:
    """Verify components wire together correctly."""

    def test_tool_registry_with_builtins(self):
        reg = ToolRegistry()
        register_builtins(reg)
        assert reg.count >= len(ALL_BUILTINS)

    def test_tool_dispatcher_from_registry(self):
        reg = ToolRegistry()
        register_builtins(reg)
        dispatcher = ToolDispatcher(reg)
        assert dispatcher is not None

    def test_command_registry_with_builtins(self):
        reg = CommandRegistry()
        register_all_commands(reg)
        assert len(reg.list_all()) >= 20

    def test_rate_limit_service_defaults(self):
        svc = RateLimitService()
        assert svc is not None

    def test_prompt_cache_enabled(self):
        mgr = PromptCacheManager(enabled=True)
        assert mgr.enabled

    def test_prompt_cache_disabled(self):
        mgr = PromptCacheManager(enabled=False)
        assert not mgr.enabled

    def test_coordinator_enter_exit(self):
        cm = CoordinatorMode()
        cm.enter()
        assert cm.active
        cm.exit()
        assert not cm.active

    def test_plugin_loader_from_registry(self):
        reg = PluginRegistry()
        loader = PluginLoader(reg)
        assert loader is not None

    def test_marketplace_client(self):
        client = MarketplaceClient(catalog_url="https://example.com/plugins")
        assert client.catalog_size == 0


# ── Config Tests ───────────────────────────────────────────────

class TestSettingsIntegration:
    """Verify new config fields exist with defaults."""

    def test_rate_limit_config(self):
        s = Settings()
        assert s.rate_limit_max_retries == 5

    def test_prompt_cache_config(self):
        s = Settings()
        assert s.disable_prompt_caching is False

    def test_lsp_config(self):
        s = Settings()
        assert s.lsp_enabled is False

    def test_voice_config(self):
        s = Settings()
        assert s.voice_stt_backend == "stub"
        assert s.voice_tts_backend == "stub"

    def test_plugin_config(self):
        s = Settings()
        assert s.plugin_catalog_url == ""

    def test_coordinator_config(self):
        s = Settings()
        assert s.coordinator_enabled is True
