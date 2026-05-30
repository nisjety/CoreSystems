from pydantic import AliasChoices, Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file=".env", extra="ignore")

    service_name: str = "data-analyzer"
    service_port: int = 9103

    internal_api_key: str = Field(
        default="",
        validation_alias=AliasChoices("INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"),
    )

    nats_shared_url: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_URL", "NATS_SHARED_URL"),
    )
    nats_shared_token: str = Field(
        default="",
        validation_alias=AliasChoices("VELION_NATS_TOKEN", "NATS_SHARED_TOKEN"),
    )

    minio_endpoint: str = Field(
        default="minio:9000",
        validation_alias=AliasChoices("MINIO_ENDPOINT"),
    )
    minio_access_key: str = Field(
        default="minioadmin",
        validation_alias=AliasChoices("MINIO_ACCESS_KEY"),
    )
    minio_secret_key: str = Field(
        default="minioadmin",
        validation_alias=AliasChoices("MINIO_SECRET_KEY"),
    )
    minio_bucket: str = Field(
        default="documents",
        validation_alias=AliasChoices("MINIO_BUCKET"),
    )


settings = Settings()
