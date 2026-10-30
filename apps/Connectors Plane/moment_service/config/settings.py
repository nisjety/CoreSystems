"""
Moment Service Configuration

Centralizes all Moment-specific configuration and environment variables.
"""

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from dotenv import load_dotenv

# Load variables from a local .env file (if present) into the environment
# before anything below reads os.getenv(...). Docker Compose already injects
# these directly via `environment:`, so this is a no-op there; it's what
# makes `python app.py` / `python test_moment_api.py` work directly against a
# local .env without the caller having to `source`/export it first.
load_dotenv()


@dataclass
class MomentConfig:
    """Configuration for Moment API service."""
    
    # API credentials
    api_key: str = field(default_factory=lambda: os.getenv("MOMENT_API_KEY", ""))
    company_code: str = field(default_factory=lambda: os.getenv("MOMENT_COMPANY_CODE", ""))
    
    # API endpoints
    base_url: str = field(default_factory=lambda: os.getenv(
        "MOMENT_BASE_URL",
        "https://app.moment.team/api/1.0"
    ))
    
    # Environment settings
    environment: str = field(default_factory=lambda: os.getenv("MOMENT_ENVIRONMENT", "prod"))  # prod, sandbox, all
    
    # Database
    database_path: Path = field(default_factory=lambda: Path(
        os.getenv("DATABASE_PATH", "./moment_sync.db")
    ))
    
    # Redis configuration (Docker internal communication)
    redis_url: str = field(default_factory=lambda: os.getenv(
        "REDIS_URL",
        "redis://:redis@redis:6379/0"
    ))
    
    # PostgreSQL configuration (Docker internal communication)
    postgres_url: str = field(default_factory=lambda: os.getenv(
        "POSTGRES_URL",
        os.getenv("DATABASE_URL", "postgresql://aquatiq:postgres@postgres:5432/aquatiq_dev")
    ))
    
    # Rate limiting
    rate_limit_requests_per_second: int = field(default_factory=lambda: int(
        os.getenv("MOMENT_RATE_LIMIT_REQUESTS_PER_SECOND", "30")
    ))
    rate_limit_requests_per_minute: int = field(default_factory=lambda: int(
        os.getenv("MOMENT_RATE_LIMIT_REQUESTS_PER_MINUTE", "120")
    ))
    
    # Sync settings
    sync_interval_minutes: int = field(default_factory=lambda: int(
        os.getenv("MOMENT_SYNC_INTERVAL_MINUTES", "15")
    ))
    
    # GCP settings (if using BigQuery)
    gcp_project_id: Optional[str] = field(default_factory=lambda: os.getenv("GCP_PROJECT_ID"))
    gcp_dataset_id: str = field(default_factory=lambda: os.getenv("GCP_DATASET_ID", "moment_data"))
    
    # Flask settings
    flask_secret_key: str = field(default_factory=lambda: os.getenv(
        "FLASK_SECRET_KEY",
        os.urandom(24).hex()
    ))
    flask_port: int = field(default_factory=lambda: int(os.getenv("MOMENT_PORT", "5006")))
    
    def __post_init__(self):
        """Validate required configuration."""
        if not self.api_key:
            raise ValueError("MOMENT_API_KEY is required")
        if not self.company_code:
            raise ValueError("MOMENT_COMPANY_CODE is required")
    
    @property
    def companies_url(self) -> str:
        """Get the companies API URL."""
        return f"{self.base_url}/companies/{self.company_code}"


# Global configuration instance
config = MomentConfig()
