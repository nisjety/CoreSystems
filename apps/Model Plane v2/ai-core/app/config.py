"""ai-core configuration."""

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
    port: int = 8001
    service_name: str = "ai-core"
    service_version: str = "2.0.0"
    environment: str = "development"
    debug: bool = False

    # Redis
    redis_url: str

    # NATS
    nats_url: str = "nats://localhost:4232"
    nats_token: str = ""

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

    # Azure Speech
    azure_speech_key: str = ""
    azure_speech_region: str = ""

    # Azure Translator
    azure_translator_key: str = ""
    azure_translator_region: str = ""

    # Azure Document Intelligence (Tier 1 — structured documents)
    azure_document_intelligence_endpoint: str = ""
    azure_document_intelligence_key: str = ""

    # Mistral Document AI (Tier 2 — complex document reasoning, via Azure AI Foundry Serverless)
    mistral_document_ai_endpoint: str = ""   # Azure AI Foundry serverless endpoint for mistral-document-ai-2505
    mistral_document_ai_key: str = ""        # Azure AI Foundry API key

    # Video generation (Sora via Azure OpenAI)
    azure_openai_video_deployment: str = "sora-turbo"
    azure_openai_video_api_version: str = "2025-01-01-preview"

    # Realtime conversation (OpenAI Realtime API)
    openai_realtime_model: str = "gpt-4o-realtime-preview-2025-01-09"
    azure_openai_realtime_deployment: str = ""   # set to enable Azure realtime

    # Azure AI Language (text analytics: sentiment, NER, key phrases, PII, summarisation)
    azure_ai_language_endpoint: str = ""
    azure_ai_language_key: str = ""
    azure_ai_language_api_version: str = "2024-11-15-preview"

    # Azure AI Content Understanding (multimodal: audio, video, image, document)
    azure_content_understanding_endpoint: str = ""
    azure_content_understanding_key: str = ""
    azure_content_understanding_api_version: str = "2025-05-01-preview"

    # gRPC
    grpc_port: int = 50051

    # Rate limiting
    rate_limit_rpm: int = 120
    rate_limit_tpm: int = 200_000

    # Result store
    result_store_threshold_bytes: int = 4096

    # Content safety
    content_safety_enabled: bool = True
    azure_content_safety_endpoint: str = ""
    azure_content_safety_key: str = ""
    content_safety_block_threshold: int = 4  # Azure severity 0-6; 4 = medium
    content_safety_flag_threshold: int = 2   # 2 = low → flag for review

    # Intent classification
    intent_llm_enabled: bool = True
    intent_model: str = "gpt-4o-mini"

    # RAG reflection + synthesis
    rag_reflection_enabled: bool = True
    rag_synthesis_enabled: bool = True
    rag_max_reflection_iterations: int = 1

    # Rate limiting
    rate_limit_rpm: int = 120
    rate_limit_tpm: int = 200_000

    # ASR
    deepgram_api_key: str = ""
    default_asr_language: str = "en-US"

    # Agent-core delegation
    agent_core_url: str = "http://localhost:8102"
    agent_core_timeout: int = 120

    # gRPC hardening
    grpc_max_workers: int = 10
    grpc_max_message_length: int = 52_428_800  # 50 MB
    grpc_keepalive_time_ms: int = 30_000
    grpc_keepalive_timeout_ms: int = 5_000
    grpc_enable_reflection: bool = True

    # Feature flags
    enable_chat: bool = True
    enable_tts: bool = True
    enable_asr: bool = True
    enable_translation: bool = True
    enable_image_generation: bool = True
    enable_vision: bool = True
    enable_document_intelligence: bool = True
    enable_content_safety: bool = True
    enable_content_understanding: bool = False
    enable_video_generation: bool = False
    enable_realtime: bool = False
    enable_mistral_document_ai: bool = False

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
