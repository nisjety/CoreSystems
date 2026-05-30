"""cost-core v2 settings."""
from __future__ import annotations

from functools import lru_cache
from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    service_name: str = "cost-core-v2"
    port: int = 8006
    log_level: str = "INFO"

    postgres_host: str = "reasoning-v2-postgres"
    postgres_port: int = 5432
    postgres_db: str = "cost_core_v2_db"
    postgres_user: str = "reasoning_user"
    postgres_password: str = "reasoning_secure_password_2026"

    nats_url: str = "nats://nats:4222"
    nats_stream: str = "VELION_COST"
    nats_subjects: str = "velion.cost.>"

    run_migrations_on_startup: bool = True

    @property
    def postgres_dsn(self) -> str:
        return (
            f"postgresql://{self.postgres_user}:{self.postgres_password}"
            f"@{self.postgres_host}:{self.postgres_port}/{self.postgres_db}"
        )


@lru_cache
def get_settings() -> Settings:
    return Settings()
