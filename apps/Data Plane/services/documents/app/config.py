from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str = "postgresql+asyncpg://dataplane:dataplane@localhost:5432/dataplane"
    redis_url: str = "redis://localhost:6379"
    service_port: int = 8001
    grpc_port: int = 50051
    
    # Cross-plane gRPC endpoints
    auth_core_grpc_url: str = Field(
        default="auth-core:50011",
        validation_alias=AliasChoices("AUTH_CORE_GRPC_URL"),
    )
    org_core_grpc_url: str = Field(
        default="org-core:9090",
        validation_alias=AliasChoices("ORG_CORE_GRPC_URL"),
    )
    
    # Service-to-service authentication
    internal_api_key: str = Field(
        default="",
        validation_alias=AliasChoices("INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"),
    )

    # Auth cache TTL (seconds)
    auth_cache_ttl: int = 60
    
    # Velion frontend-plane NATS bus (cross-plane event bus)
    nats_shared_url: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_URL", "NATS_SHARED_URL"),
    )
    nats_shared_token: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_TOKEN", "NATS_SHARED_TOKEN"),
    )

    # Velion-NATS (Model Plane v2 cross-plane bus)
    nats_velion_url: str = Field(
        default="nats://velion-nats:4222",
        validation_alias=AliasChoices("NATS_VELION_URL"),
    )
    nats_velion_token: str = Field(
        default="",
        validation_alias=AliasChoices("NATS_VELION_TOKEN"),
    )


settings = Settings()
