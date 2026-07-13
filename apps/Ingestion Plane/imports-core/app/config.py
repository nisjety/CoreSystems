from functools import lru_cache
from typing import List

from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    import_service_name: str = Field(default="import-service", alias="IMPORT_SERVICE_NAME")
    import_service_port: int = Field(default=3025, alias="IMPORT_SERVICE_PORT")
    log_level: str = Field(default="INFO", alias="LOG_LEVEL")

    database_url: str = Field(alias="DATABASE_URL")

    document_service_url: str = Field(alias="DOCUMENT_SERVICE_URL")
    document_service_import_path: str = Field(
        default="/v1/documents/", alias="DOCUMENT_SERVICE_IMPORT_PATH"
    )
    org_service_url: str = Field(alias="ORG_SERVICE_URL")
    org_service_quota_path: str = Field(
        default="/api/v1/org/quota/check", alias="ORG_SERVICE_QUOTA_PATH"
    )

    nats_url: str = Field(default="nats://localhost:4222", alias="NATS_URL")
    nats_token: str | None = Field(default=None, alias="NATS_TOKEN")

    nats_shared_url: str = Field(
        default="nats://velion-nats:4222",
        validation_alias=AliasChoices("VELION_NATS_URL", "NATS_SHARED_URL"),
    )
    nats_shared_token: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_TOKEN", "NATS_SHARED_TOKEN"),
    )

    # Internal API key for service-to-service authentication.
    # Required in production — fail fast if missing.
    internal_api_key: str = Field(
        alias="INTERNAL_API_KEY",
    )

    # Auth core URL for optional Bearer token verification via control plane
    auth_core_url: str = Field(
        default="http://auth-core:3011",
        alias="AUTH_CORE_URL",
    )
    auth_core_jwks_url: str = Field(
        default="http://auth-core:3011/api/convex-auth/jwks",
        alias="AUTH_CORE_JWKS_URL",
    )
    plane_token_issuer: str = Field(
        default="http://auth-core:3011/api/convex-auth",
        alias="PLANE_TOKEN_ISSUER",
    )
    ingestion_auth_audience: str = Field(default="ingestion", alias="INGESTION_AUTH_AUDIENCE")
    ingestion_service_id: str = Field(default="imports-core", alias="INGESTION_SERVICE_ID")
    ingestion_service_api_key: str = Field(default="", alias="INGESTION_SERVICE_API_KEY")
    allow_legacy_tenant_key: bool = Field(default=False, alias="ALLOW_LEGACY_TENANT_KEY")
    allow_insecure_dev_defaults: bool = Field(default=False, alias="ALLOW_INSECURE_DEV_DEFAULTS")
    isolated_e2e: bool = Field(default=False, alias="ISOLATED_E2E")

    # integration-corev2 base URL for the GitHub/Slack knowledge content-sync
    # worker (connections + actions surface). Empty disables the worker (the
    # trigger endpoint returns 503 "not configured"). Auth reuses internal_api_key.
    integration_core_url: str = Field(default="", alias="INTEGRATION_CORE_URL")

    temporal_enabled: bool = Field(default=False, alias="TEMPORAL_ENABLED")
    temporal_host_port: str = Field(default="localhost:7233", alias="TEMPORAL_HOST_PORT")
    temporal_namespace: str = Field(default="default", alias="TEMPORAL_NAMESPACE")
    temporal_task_queue: str = Field(
        default="import-service-task-queue", alias="TEMPORAL_TASK_QUEUE"
    )

    max_upload_files: int = Field(default=100, alias="MAX_UPLOAD_FILES")
    max_file_size_mb: int = Field(default=50, alias="MAX_FILE_SIZE_MB")
    allowed_file_types_raw: str = Field(
        default="pdf,docx,txt,md,csv,json,html,htm", alias="ALLOWED_FILE_TYPES"
    )
    document_processor_order_raw: str = Field(
        default="local", alias="DOCUMENT_PROCESSOR_ORDER"
    )
    tika_enabled: bool = Field(default=False, alias="TIKA_ENABLED")
    tika_url: str = Field(default="http://tika:9998", alias="TIKA_URL")
    tika_timeout_s: float = Field(default=20.0, alias="TIKA_TIMEOUT_S")
    gotenberg_url: str = Field(
        default="http://gotenberg:3000", alias="GOTENBERG_URL"
    )
    stirling_pdf_url: str = Field(
        default="http://stirling-pdf:8080", alias="STIRLING_PDF_URL"
    )

    @property
    def allowed_file_types(self) -> List[str]:
        return [item.strip().lower() for item in self.allowed_file_types_raw.split(",") if item.strip()]

    @property
    def document_processor_order(self) -> List[str]:
        return [
            item.strip().lower()
            for item in self.document_processor_order_raw.split(",")
            if item.strip()
        ]


@lru_cache
def get_settings() -> Settings:
    return Settings()
