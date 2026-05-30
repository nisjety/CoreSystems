"""Configuration for execution-core."""

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
    service_name: str = "execution-core"
    service_version: str = "1.0.0"
    environment: str = "development"
    debug: bool = False

    # --- HTTP ---
    host: str = "0.0.0.0"
    port: int = 8003

    # --- PostgreSQL (reasoning-postgres, separate DB) ---
    postgres_host: str = "postgres"
    postgres_port: int = 5432
    postgres_db: str = "execution_core_db"
    postgres_user: str = "reasoning_user"
    postgres_password: str = ""
    postgres_pool_min: int = 2
    postgres_pool_max: int = 10

    # --- Redis (reasoning-redis, DB 3) ---
    redis_url: str = "redis://redis:6379/3"
    redis_max_connections: int = 50
    redis_socket_timeout: float = 5.0

    # --- NATS: velion-nats (cross-plane, JetStream) ---
    nats_url: str = "nats://velion-nats:4222"
    nats_token: str = ""

    # --- NATS: reasoning-nats (local Model Plane) ---
    nats_local_url: str = "nats://nats:4222"
    nats_local_token: str = ""

    # --- Object storage (workspace artifacts) ---
    object_storage_endpoint: str = "http://minio:9000"
    object_storage_access_key: str = ""
    object_storage_secret_key: str = ""
    object_storage_bucket: str = "velion-artifacts"
    object_storage_region: str = "us-east-1"

    # --- Runner lifecycle ---
    runner_heartbeat_interval: int = 30
    runner_lease_ttl: int = 300
    max_concurrent_runners: int = 10
    workspace_base_path: str = "/tmp/workspaces"

    # --- Internal auth ---
    internal_api_key: str = ""

    @computed_field  # type: ignore[prop-decorator]
    @property
    def dsn(self) -> str:
        return (
            f"postgresql://{self.postgres_user}:{self.postgres_password}"
            f"@{self.postgres_host}:{self.postgres_port}/{self.postgres_db}"
        )


settings = Settings()
