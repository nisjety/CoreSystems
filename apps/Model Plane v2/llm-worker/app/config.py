"""llm-worker configuration."""

from __future__ import annotations

from functools import lru_cache

from pydantic import field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    """All settings from environment variables."""

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    # Server
    host: str = "0.0.0.0"
    port: int = 8005
    service_name: str = "llm-worker"
    service_version: str = "0.1.0"
    environment: str = "development"
    debug: bool = False

    # Redis (rate limiting only)
    redis_url: str

    # NATS
    nats_url: str = "nats://localhost:4232"
    nats_token: str = ""
    nats_local_url: str = "nats://localhost:4232"

    # Object storage
    object_storage_endpoint: str = "http://localhost:9000"
    object_storage_access_key: str
    object_storage_secret_key: str
    object_storage_bucket: str = "velion-completions"
    object_storage_region: str = "us-east-1"

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

    # Rate limiting
    rate_limit_rpm: int = 60
    rate_limit_tpm: int = 100_000

    # Result store
    result_store_threshold_bytes: int = 4096

    # Internal auth
    internal_api_key: str

    @field_validator("internal_api_key")
    @classmethod
    def validate_internal_api_key(cls, value: str) -> str:
        if not value or value.startswith("REPLACE_"):
            raise ValueError(
                "INTERNAL_API_KEY must be set to a secure non-placeholder value"
            )
        if len(value) < 32:
            raise ValueError("INTERNAL_API_KEY must be at least 32 characters")
        return value

    @field_validator("redis_url")
    @classmethod
    def validate_redis_url(cls, value: str) -> str:
        if "REPLACE_" in value:
            raise ValueError("REDIS_URL must not contain placeholder credentials")
        return value

@lru_cache(maxsize=1)
def get_settings() -> Settings:
    return Settings()
