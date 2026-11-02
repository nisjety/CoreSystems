"""
Abstract API currency rate source.

Fetches exchange rates from Abstract API (commercial service).
Used primarily for currencies not available from central banks (e.g., CLP).
"""

import logging
from datetime import datetime
from typing import Optional

import requests

from config import config

logger = logging.getLogger(__name__)


class AbstractAPISource:
    """
    Abstract API exchange rate data source.
    
    API: https://app.abstractapi.com/api/exchange-rates
    Requires: API key
    Coverage: 170+ currencies including CLP
    """
    
    BASE_URL = config.ABSTRACT_API_URL
    SUPPORTED_CURRENCIES = config.ABSTRACT_CURRENCIES
    
    def __init__(self, api_key: Optional[str] = None):
        self.api_key = api_key or config.ABSTRACT_API_KEY
        if not self.api_key:
            logger.warning("Abstract API key not configured")
        
        self.session = requests.Session()
    
    def get_latest_rates(
        self,
        base: str = "NOK",
        targets: Optional[list[str]] = None
    ) -> list[dict]:
        """
        Get latest exchange rates from Abstract API.
        
        Fetches all rates with NOK as base, then extracts only the target currencies.
        Abstract API doesn't support target parameter for all currencies.
        
        Args:
            base: Base currency (NOK)
            targets: Target currencies (default: CLP)
            
        Returns:
            List of rate dictionaries
        """
        if targets is None:
            targets = self.SUPPORTED_CURRENCIES
        
        if not self.api_key:
            logger.error("Cannot fetch from Abstract API: no API key")
            return []
        
        rates = []
        
        try:
            # Fetch all rates with NOK as base (no target parameter)
            params = {
                "api_key": self.api_key,
                "base": base  # NOK
            }
            
            logger.info(f"Fetching all rates from Abstract API with base={base}")
            
            response = self.session.get(
                self.BASE_URL,
                params=params,
                timeout=15
            )
            response.raise_for_status()
            
            data = response.json()
            
            # Abstract API returns: {"base": "NOK", "last_updated": 123, "exchange_rates": {...}}
            if "exchange_rates" in data:
                exchange_rates = data["exchange_rates"]
                
                # Extract only the currencies we need
                for target in targets:
                    if target in exchange_rates:
                        rate_value = exchange_rates[target]
                        
                        rate = {
                            "date": datetime.now().date().isoformat(),
                            "base": base,  # NOK
                            "quote": target,  # CLP
                            "value": rate_value,  # Direct NOK to CLP rate
                            "src": "ABSTRACT"
                        }
                        
                        rates.append(rate)
                        logger.info(f"Fetched {base}/{target} = {rate_value}")
                    else:
                        logger.warning(f"Currency {target} not available in Abstract API response")
            else:
                logger.error(f"Invalid response format: {data}")
                
        except requests.RequestException as e:
            logger.error(f"Failed to fetch from Abstract API: {e}")
        
        return rates
    
    def get_historical_rate(
        self,
        base: str,
        target: str,
        date: datetime.date
    ) -> Optional[dict]:
        """
        Get historical exchange rate for specific date.
        
        Args:
            base: Base currency
            target: Target currency
            date: Historical date
            
        Returns:
            Rate dictionary or None
        """
        if not self.api_key:
            logger.error("Cannot fetch from Abstract API: no API key")
            return None
        
        try:
            params = {
                "api_key": self.api_key,
                "base": base,
                "target": target,
                "date": date.isoformat()
            }
            
            response = self.session.get(
                f"{self.BASE_URL}/historical",
                params=params,
                timeout=15
            )
            response.raise_for_status()
            
            data = response.json()
            
            if "exchange_rate" in data:
                return {
                    "date": date.isoformat(),
                    "base": base,
                    "quote": target,
                    "value": data["exchange_rate"],
                    "src": "ABSTRACT"
                }
            
            return None
            
        except requests.RequestException as e:
            logger.error(f"Failed to fetch historical rate: {e}")
            return None
