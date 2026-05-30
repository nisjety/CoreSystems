"""Tests for Phase G — Extended Thinking (thinking.py)."""

from __future__ import annotations

import pytest

from app.thinking import (
    ThinkingConfig,
    ThinkingMode,
    has_thinking_blocks,
    model_supports_adaptive,
    model_supports_thinking,
    resolve_thinking_config,
    strip_thinking_blocks,
)


class TestModelSupportsThinking:
    def test_known_capable_model(self) -> None:
        assert model_supports_thinking("claude-4-sonnet") is True

    def test_opus_capable(self) -> None:
        assert model_supports_thinking("claude-4-opus") is True

    def test_haiku_not_capable(self) -> None:
        assert model_supports_thinking("claude-3-haiku-20240307") is False

    def test_with_provider_prefix(self) -> None:
        assert model_supports_thinking("anthropic/claude-4-sonnet") is True

    def test_with_region_prefix(self) -> None:
        assert model_supports_thinking("us.claude-4-sonnet") is True

    def test_unknown_model_not_capable(self) -> None:
        assert model_supports_thinking("gpt-4o") is False


class TestModelSupportsAdaptive:
    def test_sonnet_adaptive(self) -> None:
        assert model_supports_adaptive("claude-4-sonnet") is True

    def test_opus_adaptive(self) -> None:
        assert model_supports_adaptive("claude-4-opus") is True

    def test_non_adaptive_capable(self) -> None:
        # Claude 3.5 supports thinking but not adaptive
        assert model_supports_adaptive("claude-3.5-sonnet") is False

    def test_incapable_model(self) -> None:
        assert model_supports_adaptive("gpt-4o") is False


class TestThinkingConfig:
    def test_disabled_to_api_param_returns_none(self) -> None:
        cfg = ThinkingConfig(mode=ThinkingMode.DISABLED)
        assert cfg.to_api_param() is None

    def test_enabled_to_api_param(self) -> None:
        cfg = ThinkingConfig(mode=ThinkingMode.ENABLED, budget_tokens=5000)
        result = cfg.to_api_param()
        assert result == {"type": "enabled", "budget_tokens": 5000}

    def test_adaptive_to_api_param_uses_default_when_zero(self) -> None:
        cfg = ThinkingConfig(mode=ThinkingMode.ADAPTIVE, budget_tokens=0)
        result = cfg.to_api_param()
        assert result == {"type": "enabled", "budget_tokens": 10_000}

    def test_adaptive_to_api_param_uses_budget(self) -> None:
        cfg = ThinkingConfig(mode=ThinkingMode.ADAPTIVE, budget_tokens=8000)
        result = cfg.to_api_param()
        assert result == {"type": "enabled", "budget_tokens": 8000}

    def test_is_active_disabled(self) -> None:
        assert ThinkingConfig(mode=ThinkingMode.DISABLED).is_active is False

    def test_is_active_enabled(self) -> None:
        assert ThinkingConfig(mode=ThinkingMode.ENABLED, budget_tokens=1000).is_active is True

    def test_frozen(self) -> None:
        cfg = ThinkingConfig(mode=ThinkingMode.DISABLED)
        with pytest.raises((AttributeError, TypeError)):
            cfg.mode = ThinkingMode.ENABLED  # type: ignore[misc]


class TestResolveThinkingConfig:
    def test_incapable_model_returns_disabled(self) -> None:
        cfg = resolve_thinking_config("gpt-4o")
        assert cfg.mode == ThinkingMode.DISABLED

    def test_adaptive_capable_defaults_to_adaptive(self) -> None:
        cfg = resolve_thinking_config("claude-4-sonnet")
        assert cfg.mode == ThinkingMode.ADAPTIVE

    def test_non_adaptive_capable_defaults_to_enabled(self) -> None:
        cfg = resolve_thinking_config("claude-3.5-sonnet")
        assert cfg.mode == ThinkingMode.ENABLED
        assert cfg.budget_tokens > 0

    def test_user_override_disabled(self) -> None:
        cfg = resolve_thinking_config("claude-4-sonnet", user_override=ThinkingMode.DISABLED)
        assert cfg.mode == ThinkingMode.DISABLED

    def test_user_override_adaptive_on_capable_model(self) -> None:
        cfg = resolve_thinking_config("claude-4-opus", user_override=ThinkingMode.ADAPTIVE)
        assert cfg.mode == ThinkingMode.ADAPTIVE

    def test_user_override_adaptive_on_non_adaptive_model_falls_through(self) -> None:
        # claude-3.5-sonnet supports thinking but not adaptive
        cfg = resolve_thinking_config("claude-3.5-sonnet", user_override=ThinkingMode.ADAPTIVE)
        # Should still work but fall through to ENABLED since model doesn't support adaptive
        assert cfg.mode == ThinkingMode.ENABLED

    def test_budget_override(self) -> None:
        cfg = resolve_thinking_config("claude-4-sonnet", budget_override=5000)
        assert cfg.budget_tokens == 5000

    def test_budget_capped_at_max(self) -> None:
        cfg = resolve_thinking_config("claude-4-sonnet", budget_override=999_999)
        assert cfg.budget_tokens <= 32_000

    def test_env_budget(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setenv("MAX_THINKING_TOKENS", "7500")
        cfg = resolve_thinking_config("claude-4-sonnet")
        assert cfg.budget_tokens == 7500


class TestStripThinkingBlocks:
    def test_strips_thinking_blocks(self) -> None:
        messages = [
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "let me think..."},
                    {"type": "text", "text": "The answer is 42"},
                ],
            }
        ]
        result = strip_thinking_blocks(messages)
        assert len(result) == 1
        content = result[0]["content"]
        assert len(content) == 1
        assert content[0]["type"] == "text"

    def test_removes_message_when_only_thinking(self) -> None:
        messages = [
            {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "deep thought..."},
                ],
            }
        ]
        result = strip_thinking_blocks(messages)
        assert len(result) == 0

    def test_preserves_string_content(self) -> None:
        messages = [{"role": "user", "content": "Hello"}]
        result = strip_thinking_blocks(messages)
        assert result == messages

    def test_preserves_non_thinking_blocks(self) -> None:
        messages = [
            {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "result"},
                    {"type": "tool_use", "id": "t1"},
                ],
            }
        ]
        result = strip_thinking_blocks(messages)
        assert len(result[0]["content"]) == 2


class TestHasThinkingBlocks:
    def test_detects_thinking(self) -> None:
        messages = [
            {"role": "assistant", "content": [{"type": "thinking", "thinking": "hmm"}]}
        ]
        assert has_thinking_blocks(messages) is True

    def test_no_thinking(self) -> None:
        messages = [{"role": "user", "content": "hello"}]
        assert has_thinking_blocks(messages) is False

    def test_empty_messages(self) -> None:
        assert has_thinking_blocks([]) is False
