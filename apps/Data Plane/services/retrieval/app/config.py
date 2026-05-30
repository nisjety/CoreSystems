from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    database_url: str = "postgresql+asyncpg://dataplane:dataplane@localhost:5432/dataplane"
    redis_url: str = "redis://localhost:6379"
    service_port: int = 8004
    grpc_port: int = 50052

    # Azure OpenAI — Embeddings (with Field aliases to match .env var names)
    azure_openai_api_key: str = Field(..., alias="AZURE_OPENAI_API_KEY_EMBEDDING")
    azure_openai_endpoint: str = Field("https://core-ai-rg.cognitiveservices.azure.com", alias="AZURE_OPENAI_ENDPOINT_EMBEDDING")
    azure_openai_deployment: str = Field("text-embedding-3-large", alias="AZURE_OPENAI_DEPLOYMENT_EMBEDDING")
    azure_openai_api_version: str = Field("2024-12-01-preview", alias="AZURE_OPENAI_API_VERSION_EMBEDDING")

    # Cohere — Reranking only (Azure-hosted endpoint)
    cohere_api_key: str = Field(..., alias="COHERE_API_KEY", min_length=1)
    cohere_base_url: str = "https://core-ai-rg.services.ai.azure.com/providers/cohere/v2"
    cohere_rerank_model: str = "Cohere-rerank-v4.0-pro"

    # Qdrant
    qdrant_host: str = "qdrant"
    qdrant_port: int = 6333
    qdrant_collection: str = "dataplane_knowledge"

    # Retrieval defaults
    top_k: int = 20             # candidates before reranking
    top_n_after_rerank: int = 5  # final facts returned

    # Hybrid retrieval (Phase 2.3)
    hybrid_enabled: bool = True      # enable BM25 + dense fusion
    hybrid_bm25_top_k_factor: int = 2  # pull top_k * factor for BM25 scoring

    # Confidence gating (Phase 2.4)
    confidence_threshold: float = 0.3  # min rerank_score for high-confidence results
    
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

    # ── Agentic RAG ──────────────────────────────────────────────────────────
    # Multi-agent pipeline: Planning → Routing → Retrieval → Reranking →
    #                       Reflection → Synthesis (6 agents)
    # LLM calls delegated to ai-core v2 via HTTP.
    agentic_rag_enabled: bool = Field(
        default=False,
        validation_alias=AliasChoices("AGENTIC_RAG_ENABLED"),
    )
    # ai-core URL for LLM calls made by the RAG agents
    ai_core_url: str = Field(
        default="http://ai-core:8001",
        validation_alias=AliasChoices("AI_CORE_URL"),
    )
    # Model used by Planning, Routing, Reflection, and Synthesis agents
    agentic_rag_model: str = Field(
        default="gpt-4o",
        validation_alias=AliasChoices("AGENTIC_RAG_MODEL"),
    )
    # Max sub-queries the Planning agent may decompose a question into
    agentic_rag_max_subqueries: int = Field(
        default=4,
        validation_alias=AliasChoices("AGENTIC_RAG_MAX_SUBQUERIES"),
    )
    # Max reflection iterations before the Synthesis agent gives a final answer
    agentic_rag_max_reflection_iters: int = Field(
        default=2,
        validation_alias=AliasChoices("AGENTIC_RAG_MAX_REFLECTION_ITERS"),
    )
    # Confidence threshold below which Reflection agent triggers another retrieval pass
    agentic_rag_reflection_threshold: float = Field(
        default=0.5,
        validation_alias=AliasChoices("AGENTIC_RAG_REFLECTION_THRESHOLD"),
    )


settings = Settings()
