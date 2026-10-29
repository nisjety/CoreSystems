"""Contifico Service Configuration."""
import os
from pathlib import Path
from typing import Optional


class Settings:
    """Configuration settings for Contifico service."""

    # Service Configuration
    PORT: int = int(os.getenv("CONTIFICO_PORT", "5006"))
    HOST: str = os.getenv("CONTIFICO_HOST", "0.0.0.0")
    FLASK_ENV: str = os.getenv("FLASK_ENV", "development")
    FLASK_DEBUG: bool = os.getenv("FLASK_DEBUG", "False").lower() == "true"
    LOG_LEVEL: str = os.getenv("LOG_LEVEL", "INFO")

    # Contifico API Configuration
    CONTIFICO_API_KEY: str = os.getenv("CONTIFICO_API_KEY", "")
    CONTIFICO_API_TOKEN: str = os.getenv("CONTIFICO_API_TOKEN", "")
    CONTIFICO_API_BASE_URL: str = os.getenv(
        "CONTIFICO_API_BASE_URL",
        "https://api.contifico.com/sistema/api/v1"
    )

    # Database Configuration
    DATABASE_URL: str = os.getenv("DATABASE_URL", "")
    POSTGRES_URL: str = os.getenv("POSTGRES_URL", "")

    # Redis Configuration (Docker internal communication)
    REDIS_URL: str = os.getenv("REDIS_URL", "redis://:redis@redis:6379")
    REDIS_CACHE_TTL: int = int(os.getenv("REDIS_CACHE_TTL", "3600"))

    # GCP Configuration
    GCP_PROJECT_ID: str = os.getenv("GCP_PROJECT_ID", "")
    GCP_REGION: str = os.getenv("GCP_REGION", "europe-west1")
    GCP_DATASET_ID: str = os.getenv("GCP_DATASET_ID", "dw_1_staging_contifico")
    GCP_BUCKET_NAME: str = os.getenv("GCP_BUCKET_NAME", "")
    GOOGLE_APPLICATION_CREDENTIALS: Optional[str] = os.getenv("GOOGLE_APPLICATION_CREDENTIALS")

    # Rate Limiting
    CONTIFICO_RATE_LIMIT_MINUTE: int = int(os.getenv("CONTIFICO_RATE_LIMIT_MINUTE", "60"))
    CONTIFICO_RATE_LIMIT_DAY: int = int(os.getenv("CONTIFICO_RATE_LIMIT_DAY", "5000"))

    # Retry Configuration
    CONTIFICO_MAX_RETRIES: int = int(os.getenv("CONTIFICO_MAX_RETRIES", "5"))
    CONTIFICO_RETRY_DELAY: float = float(os.getenv("CONTIFICO_RETRY_DELAY", "1.0"))
    CONTIFICO_MAX_DELAY: float = float(os.getenv("CONTIFICO_MAX_DELAY", "60.0"))

    # Sync Configuration
    SYNC_ENABLED: bool = os.getenv("SYNC_ENABLED", "true").lower() == "true"
    SYNC_INTERVAL: int = int(os.getenv("SYNC_INTERVAL", "900"))
    SYNC_BATCH_SIZE: int = int(os.getenv("SYNC_BATCH_SIZE", "100"))

    # Security
    FLASK_SECRET_KEY: str = os.getenv("FLASK_SECRET_KEY", "dev_secret_key")

    @classmethod
    def validate(cls) -> None:
        """Validate required settings."""
        required_vars = [
            "CONTIFICO_API_KEY",
            "CONTIFICO_API_TOKEN",
        ]
        
        missing = [var for var in required_vars if not getattr(cls, var)]
        
        if missing:
            raise ValueError(
                f"Missing required environment variables: {', '.join(missing)}"
            )


settings = Settings()
