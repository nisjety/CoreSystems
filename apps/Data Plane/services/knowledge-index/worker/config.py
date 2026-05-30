from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str = "postgresql+asyncpg://dataplane:dataplane@localhost:5432/dataplane"
    redis_url: str = "redis://localhost:6379"
    documents_grpc_host: str = "documents-service"
    documents_grpc_port: int = 50051

    # Chunking parameters (token-aware, Phase 2.2)
    chunk_size: int = 256      # max tokens per chunk (tiktoken cl100k_base)
    chunk_overlap: int = 32    # token overlap between consecutive chunks
    pending_min_idle_ms: int = Field(30000, alias="KNOWLEDGE_INDEX_PENDING_MIN_IDLE_MS")
    max_delivery_attempts: int = Field(5, alias="KNOWLEDGE_INDEX_MAX_DELIVERY_ATTEMPTS")
    admin_host: str = Field("0.0.0.0", alias="KNOWLEDGE_INDEX_ADMIN_HOST")
    admin_port: int = Field(9101, alias="KNOWLEDGE_INDEX_ADMIN_PORT")
    
    # Velion frontend-plane NATS bus (cross-plane event bus)
    nats_shared_url: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_URL", "NATS_SHARED_URL"),
    )
    nats_shared_token: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_TOKEN", "NATS_SHARED_TOKEN"),
    )

    # Shared service-to-service auth key (must match documents-service INTERNAL_API_KEY)
    internal_api_key: str = Field(default="", alias="INTERNAL_API_KEY")


settings = Settings()
