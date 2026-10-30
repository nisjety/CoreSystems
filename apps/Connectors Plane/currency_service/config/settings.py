"""
Currency Service Configuration

Centralizes all currency service configuration and environment variables.
"""

import os
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class CurrencyConfig:
    """Currency service configuration."""
    
    # Service settings
    SERVICE_NAME: str = "currency_service"
    SERVICE_PORT: int = int(os.getenv("CURRENCY_PORT", "5004"))
    HOST: str = os.getenv("CURRENCY_HOST", "0.0.0.0")
    DEBUG: bool = os.getenv("FLASK_DEBUG", "False").lower() == "true"
    
    # API Keys
    ABSTRACT_API_KEY: str = os.getenv("ABSTRACT_API_KEY", "")
    FCS_API_KEY: str = os.getenv("FCS_API_KEY", "")
    
    # Data sources configuration
    NORGES_BANK_BASE_URL: str = "https://data.norges-bank.no/api/data/EXR"
    ECB_BASE_URL: str = "https://data-api.ecb.europa.eu/service/data/EXR"
    ABSTRACT_API_URL: str = "https://api.abstractapi.com/v1/exchange-rates"
    FCS_API_URL: str = "https://fcsapi.com/api-v3/forex/latest"
    
    # Supported currencies
    # Norges Bank provides: USD, EUR, GBP, SEK, DKK, CHF, CAD, ISK, AUD, NZD, IDR, JPY
    # Updates: Monday-Friday at 16:00 CET
    # NB: CHF is quoted per 100 ("Hundreds", unit_mult=2) — handled by _parse_csv.
    NORGES_BANK_CURRENCIES: list = field(default_factory=lambda: [
        "USD", "EUR", "GBP", "SEK", "DKK", "CHF",
        "CAD", "ISK", "AUD", "NZD", "IDR", "JPY"
    ])
    
    # FCS API provides currencies not available from Norges Bank
    FCS_CURRENCIES: list = field(default_factory=lambda: [
        "CLP",  # Chilean Peso
        "BRL"   # Brazilian Real
    ])
    
    # Deprecated - keeping for backward compatibility
    ABSTRACT_CURRENCIES: list = field(default_factory=lambda: [])
    ECB_CURRENCIES: list = field(default_factory=lambda: [])
    
    # Cache settings
    CACHE_DIR: str = os.getenv("CURRENCY_CACHE_DIR", 
                               str(Path(__file__).parent.parent / "cache" / "rates"))
    CACHE_TTL_SECONDS: int = int(os.getenv("CACHE_TTL", "3600"))  # 1 hour
    
    # Database settings (shared)
    DB_PATH: str = os.getenv("DB_PATH", 
                            str(Path(__file__).parent.parent.parent.parent / "data" / "tokens.db"))
    
    # Rate limiting
    RATE_LIMIT_REQUESTS: int = int(os.getenv("CURRENCY_RATE_LIMIT", "100"))
    
    # Retry configuration
    MAX_RETRIES: int = int(os.getenv("CURRENCY_MAX_RETRIES", "3"))
    BASE_DELAY: float = float(os.getenv("CURRENCY_RETRY_DELAY", "1.0"))
    MAX_DELAY: float = float(os.getenv("CURRENCY_MAX_DELAY", "30.0"))
    
    # Logging
    LOG_LEVEL: str = os.getenv("LOG_LEVEL", "INFO")
    
    @classmethod
    def validate(cls) -> bool:
        """Validate required configuration."""
        config = cls()
        
        # Abstract API key is optional but warn if missing
        if not config.ABSTRACT_API_KEY:
            import logging
            logging.getLogger(__name__).warning(
                "ABSTRACT_API_KEY not configured - CLP rates unavailable"
            )
        
        return True
    
    # Property accessors
    @property
    def port(self) -> int:
        return self.SERVICE_PORT
    
    @property
    def host(self) -> str:
        return self.HOST
    
    @property
    def database_path(self) -> str:
        return self.DB_PATH
    
    @property
    def cache_dir(self) -> str:
        return self.CACHE_DIR
    
    @property
    def cache_ttl(self) -> int:
        return self.CACHE_TTL_SECONDS


# Singleton instance
config = CurrencyConfig()
