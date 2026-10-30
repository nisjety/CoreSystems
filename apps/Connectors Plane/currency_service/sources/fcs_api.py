"""
FCS Forex API currency rate source.

Fetches exchange rates from FCS API (https://fcsapi.com).
Used for currencies not available from central banks (e.g., CLP).
"""

import logging
from datetime import datetime
from typing import Optional

import requests

from config import config

logger = logging.getLogger(__name__)


class FCSAPISource:
    """
    FCS Forex API exchange rate data source.
    
    API: https://fcsapi.com/api-v3/forex
    Requires: API key
    Coverage: 2000+ forex currency pairs including CLP
    Free tier: 500 API calls per month
    """
    
    BASE_URL = config.FCS_API_URL
    SUPPORTED_CURRENCIES = config.FCS_CURRENCIES
    
    def __init__(self, api_key: Optional[str] = None):
        self.api_key = api_key or config.FCS_API_KEY
        if not self.api_key:
            logger.warning("FCS API key not configured")
        
        self.session = requests.Session()
    
    def get_latest_rates(
        self,
        base: str = "NOK",
        targets: Optional[list[str]] = None
    ) -> list[dict]:
        """
        Get latest exchange rates from FCS API.
        
        FCS API returns rates with format: EUR/USD, GBP/USD, etc.
        We need NOK as the quote currency, so we fetch pairs like CLP/NOK.
        
        Args:
            base: Base currency (NOK)
            targets: Target currencies (default: CLP)
            
        Returns:
            List of rate dictionaries with format:
            {
                "date": "2025-12-10",
                "base": "CLP",
                "quote": "NOK",
                "value": 0.0123,
                "src": "FCS"
            }
        """
        if targets is None:
            targets = self.SUPPORTED_CURRENCIES
        
        if not self.api_key:
            logger.error("Cannot fetch from FCS API: no API key")
            return []
        
        rates = []
        
        # FCS API uses format: BASE/QUOTE (e.g., CLP/NOK)
        # We want rates where NOK is the quote currency
        symbols = [f"{target}/{base}" for target in targets]
        symbol_param = ",".join(symbols)
        
        try:
            params = {
                "access_key": self.api_key,
                "symbol": symbol_param
            }
            
            logger.info(f"Fetching {len(symbols)} rates from FCS API: {symbol_param}")
            
            response = self.session.get(
                self.BASE_URL,
                params=params,
                timeout=15
            )
            response.raise_for_status()
            
            data = response.json()
            
            # Check if API returned success
            if not data.get("status"):
                error_msg = data.get("msg", "Unknown error")
                logger.error(f"FCS API error: {error_msg}")
                return []
            
            # FCS API returns: {"status": true, "response": [{...}], "info": {...}}
            if "response" in data and isinstance(data["response"], list):
                for item in data["response"]:
                    # Extract symbol (e.g., "CLP/NOK")
                    symbol = item.get("s", "")
                    if "/" not in symbol:
                        continue
                    
                    parts = symbol.split("/")
                    if len(parts) != 2:
                        continue
                    
                    item_base = parts[0]  # CLP
                    item_quote = parts[1]  # NOK
                    
                    # Get the close price (current price)
                    rate_value = item.get("c")
                    
                    if rate_value and item_quote == base:
                        rate = {
                            "date": datetime.now().date().isoformat(),
                            "base": item_base,  # CLP
                            "quote": item_quote,  # NOK
                            "value": float(rate_value),
                            "src": "FCS"
                        }
                        
                        rates.append(rate)
                        logger.info(f"Fetched {item_base}/{item_quote} = {rate_value}")
                    else:
                        logger.warning(f"Invalid rate data for {symbol}")
            else:
                logger.error(f"Unexpected response format: {data}")
            
            logger.info(f"Successfully fetched {len(rates)} rates from FCS API")
            
        except requests.RequestException as e:
            logger.error(f"Failed to fetch from FCS API: {e}")
        
        return rates
    
    def get_historical_rate(
        self,
        base: str,
        quote: str,
        date: str
    ) -> Optional[dict]:
        """
        Get historical exchange rate for a specific date.
        
        Note: FCS API historical data requires specific endpoint and may have
        different credit usage. Not implemented for now.
        
        Args:
            base: Base currency
            quote: Quote currency
            date: Date in YYYY-MM-DD format
            
        Returns:
            Rate dictionary or None if not found
        """
        logger.warning("Historical rates not implemented for FCS API")
        return None
