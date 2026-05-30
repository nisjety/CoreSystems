from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str = "postgresql+asyncpg://dataplane:dataplane@localhost:5432/dataplane"
    redis_url: str = "redis://localhost:6379"

    # Azure OpenAI — Embeddings
    azure_openai_api_key: str = Field(..., alias="AZURE_OPENAI_API_KEY_EMBEDDING")
    azure_openai_endpoint: str = Field("https://core-ai-rg.cognitiveservices.azure.com", alias="AZURE_OPENAI_ENDPOINT_EMBEDDING")
    azure_openai_embedding_deployment: str = Field("text-embedding-3-large", alias="AZURE_OPENAI_DEPLOYMENT_EMBEDDING")
    azure_openai_api_version: str = Field("2024-12-01-preview", alias="AZURE_OPENAI_API_VERSION_EMBEDDING")

    # Qdrant
    qdrant_host: str = "qdrant"
    qdrant_port: int = 6333
    qdrant_collection: str = "dataplane_knowledge"

    # Batching
    batch_size: int = 32
    pending_min_idle_ms: int = Field(30000, alias="EMBEDDING_PENDING_MIN_IDLE_MS")
    max_delivery_attempts: int = Field(5, alias="EMBEDDING_MAX_DELIVERY_ATTEMPTS")
    admin_host: str = Field("0.0.0.0", alias="EMBEDDING_ADMIN_HOST")
    admin_port: int = Field(9102, alias="EMBEDDING_ADMIN_PORT")
    
    # Velion frontend-plane NATS bus (cross-plane event bus)
    nats_shared_url: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_URL", "NATS_SHARED_URL"),
    )
    nats_shared_token: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_TOKEN", "NATS_SHARED_TOKEN"),
    )


settings = Settings()
