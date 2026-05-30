"""Configuration for capability-core."""

from __future__ import annotations

from pydantic import computed_field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    """Environment-driven settings with sensible Docker defaults."""

    model_config = SettingsConfigDict(
        env_file=".env",
        env_file_encoding="utf-8",
        case_sensitive=False,
        extra="ignore",
    )

    # --- Service identity ---
    service_name: str = "capability-core"
    service_version: str = "1.0.0"
    environment: str = "development"
    debug: bool = False

    # --- HTTP ---
    host: str = "0.0.0.0"
    port: int = 8004

    # --- PostgreSQL (reasoning-postgres, separate DB) ---
    postgres_host: str = "postgres"
    postgres_port: int = 5432
    postgres_db: str = "capability_core_db"
    postgres_user: str = "reasoning_user"
    postgres_password: str = "reasoning_secure_password_2026"
    postgres_pool_min: int = 2
    postgres_pool_max: int = 10

    # --- Redis (reasoning-redis, DB 4) ---
    redis_url: str = "redis://redis:6379/4"
    redis_max_connections: int = 50
    redis_socket_timeout: float = 5.0

    # --- NATS: velion-nats (cross-plane, JetStream) ---
    nats_url: str = "nats://velion-nats:4222"
    nats_token: str = ""

    # --- NATS: reasoning-nats (local Model Plane) ---
    nats_local_url: str = "nats://nats:4222"
    nats_local_token: str = ""

    # --- Internal auth ---
    internal_api_key: str = ""

    # --- Azure Content Safety ---
    azure_content_safety_endpoint: str = ""
    azure_content_safety_key: str = ""

    # --- Budget defaults (NOK) ---
    default_daily_budget_nok: float = 25.0
    default_monthly_budget_nok: float = 500.0

    # --- Tool search ---
    tool_search_cache_ttl: int = 300
    provider_health_cache_ttl: int = 30

    @computed_field  # type: ignore[prop-decorator]
    @property
    def dsn(self) -> str:
        return (
            f"postgresql://{self.postgres_user}:{self.postgres_password}"
            f"@{self.postgres_host}:{self.postgres_port}/{self.postgres_db}"
        )


settings = Settings()


def get_settings() -> Settings:
    """Return the singleton settings instance."""
    return settings
