"""Currency converter integration with currency_service."""

import logging
from typing import Dict, Optional, Any
from decimal import Decimal

import requests
from requests.exceptions import RequestException

logger = logging.getLogger(__name__)


class CurrencyConverter:
    """Client for currency conversion using the currency_service."""
    
    def __init__(self, currency_service_url: str = "http://currency-service:5004"):
        """Initialize currency converter.
        
        Args:
            currency_service_url: URL to the currency service
        """
        self.service_url = currency_service_url.rstrip("/")
    
    def get_exchange_rate(
        self,
        from_currency: str,
        to_currency: str,
        date: Optional[str] = None
    ) -> Optional[Decimal]:
        """Get exchange rate between two currencies.
        
        Args:
            from_currency: Source currency code (e.g., 'USD')
            to_currency: Target currency code (e.g., 'NOK')
            date: Optional date in YYYY-MM-DD format (defaults to latest)
        
        Returns:
            Exchange rate or None if not available
        """
        try:
            params = {
                "from": from_currency,
                "to": to_currency
            }
            if date:
                params["date"] = date
            
            response = requests.get(
                f"{self.service_url}/api/rates",
                params=params,
                timeout=10
            )
            response.raise_for_status()
            
            data = response.json()
            rate = data.get("rate")
            
            if rate:
                logger.debug(f"Exchange rate {from_currency}/{to_currency}: {rate}")
                return Decimal(str(rate))
            
            return None
        
        except RequestException as e:
            logger.error(f"Failed to get exchange rate: {e}")
            return None
    
    def convert_amount(
        self,
        amount: float,
        from_currency: str,
        to_currency: str,
        date: Optional[str] = None
    ) -> Optional[Dict[str, Any]]:
        """Convert amount from one currency to another.
        
        Args:
            amount: Amount to convert
            from_currency: Source currency code
            to_currency: Target currency code
            date: Optional date for historical rate
        
        Returns:
            Dict with converted amount and rate, or None
        """
        if from_currency == to_currency:
            return {
                "amount": amount,
                "from_currency": from_currency,
                "to_currency": to_currency,
                "rate": 1.0,
                "converted_amount": amount
            }
        
        rate = self.get_exchange_rate(from_currency, to_currency, date)
        
        if not rate:
            logger.warning(f"Could not convert {from_currency} to {to_currency}")
            return None
        
        converted_amount = Decimal(str(amount)) * rate
        
        return {
            "amount": amount,
            "from_currency": from_currency,
            "to_currency": to_currency,
            "rate": float(rate),
            "converted_amount": float(converted_amount)
        }
    
    def convert_superoffice_data(
        self,
        data: Dict[str, Any],
        target_currency: str,
        amount_fields: list[str] = None
    ) -> Dict[str, Any]:
        """Convert SuperOffice data amounts to target currency.
        
        Args:
            data: SuperOffice data (sale, invoice, etc.)
            target_currency: Target currency code
            amount_fields: Fields to convert (defaults to common fields)
        
        Returns:
            Data with converted amounts
        """
        if not amount_fields:
            amount_fields = ["Amount", "TotalAmount", "NetAmount", "GrossAmount"]
        
        # Get source currency from data
        source_currency = data.get("Currency") or data.get("CurrencyCode")
        
        if not source_currency:
            logger.warning("No source currency found in data")
            return data
        
        if source_currency == target_currency:
            return data
        
        # Convert each amount field
        converted_data = data.copy()
        conversion_info = {}
        
        for field in amount_fields:
            if field in data and data[field] is not None:
                result = self.convert_amount(
                    data[field],
                    source_currency,
                    target_currency
                )
                
                if result:
                    converted_data[f"{field}Converted"] = result["converted_amount"]
                    conversion_info[field] = {
                        "original": data[field],
                        "converted": result["converted_amount"],
                        "rate": result["rate"]
                    }
        
        # Add conversion metadata
        converted_data["_conversion"] = {
            "original_currency": source_currency,
            "target_currency": target_currency,
            "fields": conversion_info
        }
        
        return converted_data
    
    def get_supported_currencies(self) -> list[str]:
        """Get list of supported currencies from currency service.
        
        Returns:
            List of currency codes
        """
        try:
            response = requests.get(
                f"{self.service_url}/api/currencies",
                timeout=10
            )
            response.raise_for_status()
            
            data = response.json()
            return data.get("currencies", [])
        
        except RequestException as e:
            logger.error(f"Failed to get supported currencies: {e}")
            return []
