"""Runtime configuration — each consumer supplies its own settings."""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class RuntimeConfig:
    """Immutable configuration for the reasoning runtime.

    Each service (ai-core, agent-core, llm-worker) creates an instance
    from its own pydantic Settings and passes it to ``configure()``.
    """

    # Provider API keys
    openai_api_key: str = ""
    anthropic_api_key: str = ""
    google_api_key: str = ""
    cohere_api_key: str = ""
    mistral_api_key: str = ""
    ollama_base_url: str = "http://localhost:11434"

    # Azure OpenAI
    azure_openai_endpoint: str = ""
    azure_openai_api_key: str = ""
    azure_openai_api_version: str = "2024-10-21"

    # Azure Speech (TTS/STT)
    azure_speech_key: str = ""
    azure_speech_region: str = ""

    # Azure Translator
    azure_translator_key: str = ""
    azure_translator_region: str = ""

    # Redis (rate limiting)
    redis_url: str = "redis://localhost:6379/5"
    rate_limit_rpm: int = 60
    rate_limit_tpm: int = 100_000

    # Object storage (MinIO / S3)
    object_storage_endpoint: str = "http://localhost:9000"
    object_storage_access_key: str = "minioadmin"
    object_storage_secret_key: str = "minioadmin"
    object_storage_bucket: str = "velion-completions"
    object_storage_region: str = "us-east-1"

    # Result store
    result_store_threshold_bytes: int = 4096


# ── Singleton config ────────────────────────────────────────────

_config: RuntimeConfig = RuntimeConfig()


def configure(cfg: RuntimeConfig) -> None:
    """Set the active runtime configuration.  Call once during startup."""
    global _config
    _config = cfg


def get_config() -> RuntimeConfig:
    """Return the active runtime configuration."""
    return _config
