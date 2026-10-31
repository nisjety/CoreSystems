"""
Currency converter.

Performs currency conversion using cached or live exchange rates.
"""

import logging
from datetime import datetime
from typing import Optional

logger = logging.getLogger(__name__)


class CurrencyConverter:
    """
    Currency converter with multi-source rate support.
    """
    
    def __init__(self, rate_cache):
        """
        Initialize converter.
        
        Args:
            rate_cache: RateCache instance
        """
        self.cache = rate_cache
    
    def convert(
        self,
        amount: float,
        from_currency: str,
        to_currency: str,
        rates: Optional[dict] = None
    ) -> dict:
        """
        Convert amount between currencies.
        
        Args:
            amount: Amount to convert
            from_currency: Source currency code
            to_currency: Target currency code
            rates: Optional rate dictionary {currency: rate_to_NOK}
            
        Returns:
            Dictionary with:
            - from_currency: Source currency
            - to_currency: Target currency
            - amount: Source amount
            - result: Converted amount
            - rate: Exchange rate used
            - via: Intermediate currency if applicable
        """
        # Handle same currency
        if from_currency == to_currency:
            return {
                "from_currency": from_currency,
                "to_currency": to_currency,
                "amount": amount,
                "result": amount,
                "rate": 1.0,
                "via": None
            }
        
        # If rates provided, use them
        if rates:
            return self._convert_with_rates(amount, from_currency, to_currency, rates)
        
        # Otherwise, error (rates should be provided by caller)
        raise ValueError("No rates provided for conversion")
    
    def _convert_with_rates(
        self,
        amount: float,
        from_currency: str,
        to_currency: str,
        rates: dict
    ) -> dict:
        """
        Convert using provided rate dictionary.
        
        Rates are assumed to be in format {currency: rate_to_NOK}.
        """
        # If both currencies have rates to NOK, convert via NOK
        if from_currency in rates and to_currency in rates:
            # from_currency -> NOK -> to_currency
            nok_amount = amount * rates[from_currency]
            result = nok_amount / rates[to_currency]
            rate = rates[from_currency] / rates[to_currency]
            
            return {
                "from_currency": from_currency,
                "to_currency": to_currency,
                "amount": amount,
                "result": result,
                "rate": rate,
                "via": "NOK"
            }
        
        # If only from_currency has rate
        elif from_currency in rates and to_currency == "NOK":
            result = amount * rates[from_currency]
            return {
                "from_currency": from_currency,
                "to_currency": to_currency,
                "amount": amount,
                "result": result,
                "rate": rates[from_currency],
                "via": None
            }
        
        # If only to_currency has rate
        elif from_currency == "NOK" and to_currency in rates:
            result = amount / rates[to_currency]
            return {
                "from_currency": from_currency,
                "to_currency": to_currency,
                "amount": amount,
                "result": result,
                "rate": 1 / rates[to_currency],
                "via": None
            }
        
        # No conversion possible
        missing = []
        if from_currency not in rates and from_currency != "NOK":
            missing.append(from_currency)
        if to_currency not in rates and to_currency != "NOK":
            missing.append(to_currency)
        
        raise ValueError(f"No rates available for: {', '.join(missing)}")
