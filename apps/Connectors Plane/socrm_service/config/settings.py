"""SuperOffice CRM Service Configuration Settings."""

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from dotenv import load_dotenv

# Load environment variables
load_dotenv()


@dataclass
class Settings:
    """Service configuration settings."""
    
    # Service settings
    host: str = os.getenv("SOCRM_HOST", "0.0.0.0")
    port: int = int(os.getenv("SOCRM_PORT", "5006"))
    debug: bool = os.getenv("FLASK_DEBUG", "false").lower() == "true"
    
    # Microsoft Azure AD OAuth settings (Primary Authentication)
    microsoft_client_id: str = os.getenv("MICROSOFT_CLIENT_ID", "")
    microsoft_client_secret: str = os.getenv("MICROSOFT_CLIENT_SECRET", "")
    microsoft_redirect_uri: str = os.getenv("MICROSOFT_REDIRECT_URI", "http://localhost:5006/api/auth/microsoft/callback")
    microsoft_tenant_id: str = os.getenv("MICROSOFT_TENANT_ID", "common")
    
    @property
    def microsoft_auth_url(self) -> str:
        """Get Microsoft authorization URL."""
        return f"https://login.microsoftonline.com/{self.microsoft_tenant_id}/oauth2/v2.0/authorize"
    
    @property
    def microsoft_token_url(self) -> str:
        """Get Microsoft token URL."""
        return f"https://login.microsoftonline.com/{self.microsoft_tenant_id}/oauth2/v2.0/token"
    
    # SuperOffice OAuth settings (Legacy/Alternative)
    client_id: str = os.getenv("SOCRM_CLIENT_ID", "")
    client_secret: str = os.getenv("SOCRM_CLIENT_SECRET", "")
    redirect_uri: str = os.getenv("SOCRM_REDIRECT_URI", "http://localhost:5006/api/auth/callback")
    tenant: str = os.getenv("SOCRM_TENANT", "sod")  # sod, stage, or online
    
    # SuperOffice API endpoints based on tenant
    @property
    def auth_url(self) -> str:
        """Get authorization URL based on tenant."""
        tenant_urls = {
            "sod": "https://sod.superoffice.com",
            "stage": "https://stage.superoffice.com",
            "online": "https://online.superoffice.com"
        }
        return f"{tenant_urls.get(self.tenant, tenant_urls['sod'])}/login/common/oauth/authorize"
    
    @property
    def token_url(self) -> str:
        """Get token URL based on tenant."""
        tenant_urls = {
            "sod": "https://sod.superoffice.com",
            "stage": "https://stage.superoffice.com",
            "online": "https://online.superoffice.com"
        }
        return f"{tenant_urls.get(self.tenant, tenant_urls['sod'])}/login/common/oauth/tokens"
    
    @property
    def api_base_url(self) -> str:
        """Get API base URL based on tenant."""
        tenant_urls = {
            "sod": "https://sod.superoffice.com",
            "stage": "https://stage.superoffice.com",
            "online": "https://online.superoffice.com"
        }
        return f"{tenant_urls.get(self.tenant, tenant_urls['sod'])}/api/v1"
    
    # Scopes for SuperOffice API
    scopes: str = "openid offline_access"
    
    # Database settings
    database_url: str = os.getenv(
        "DATABASE_URL",
        "postgresql://postgres:postgres@localhost:5432/aquatiq_db"
    )
    
    # Currency Service settings
    currency_service_url: str = os.getenv(
        "CURRENCY_SERVICE_URL",
        "http://currency-service:5004"
    )
    default_currency: str = os.getenv("DEFAULT_CURRENCY", "NOK")
    
    # Cache settings
    cache_dir: Path = Path(os.getenv("CACHE_DIR", "/app/cache"))
    cache_ttl: int = int(os.getenv("CACHE_TTL", "3600"))
    
    # Redis settings
    redis_url: str = os.getenv("REDIS_URL", "redis://localhost:6379/0")
    
    # Logging
    log_level: str = os.getenv("LOG_LEVEL", "INFO")
    
    # Rate limiting
    rate_limit_per_minute: int = int(os.getenv("RATE_LIMIT_PER_MINUTE", "60"))
    
    # Timeout settings
    request_timeout: int = int(os.getenv("REQUEST_TIMEOUT", "30"))
    
    def __post_init__(self):
        """Validate settings after initialization."""
        # Microsoft Auth is now primary
        if not self.microsoft_client_id:
            raise ValueError("MICROSOFT_CLIENT_ID is required")
        if not self.microsoft_client_secret:
            raise ValueError("MICROSOFT_CLIENT_SECRET is required")
        
        # SuperOffice credentials are optional (for direct integration)
        if self.client_id and not self.client_secret:
            raise ValueError("SOCRM_CLIENT_SECRET required if SOCRM_CLIENT_ID is set")
        
        # Create cache directory if it doesn't exist
        self.cache_dir.mkdir(parents=True, exist_ok=True)
    
    def get_api_url(self, endpoint: str) -> str:
        """Get full API URL for an endpoint."""
        return f"{self.api_base_url}/{endpoint.lstrip('/')}"
