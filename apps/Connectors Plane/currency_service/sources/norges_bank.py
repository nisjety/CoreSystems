"""
Norges Bank currency rate source.

Fetches exchange rates from Norges Bank's SDMX API (EXR dataset).
"""

import csv
import logging
from datetime import datetime, timedelta
from io import StringIO
from typing import Optional
from urllib.parse import urlencode

import requests

from config import config

logger = logging.getLogger(__name__)


class NorgesBankSource:
    """
    Norges Bank exchange rate data source.
    
    API: https://data.norges-bank.no/api/data/EXR
    Format: SDMX CSV
    Base currency: Always returns rates to NOK
    """
    
    BASE_URL = config.NORGES_BANK_BASE_URL
    SUPPORTED_CURRENCIES = config.NORGES_BANK_CURRENCIES
    
    def __init__(self):
        self.session = requests.Session()
        self.session.headers.update({
            "User-Agent": "Aquatiq-Currency-Service/1.0"
        })
    
    def get_latest_rates(
        self,
        currencies: Optional[list[str]] = None
    ) -> list[dict]:
        """
        Get latest exchange rates for specified currencies.
        
        Args:
            currencies: List of currency codes (default: all supported)
            
        Returns:
            List of rate dictionaries with keys:
            - date (str): Rate date (YYYY-MM-DD)
            - base (str): Base currency code
            - quote (str): Quote currency (always NOK)
            - value (float): Exchange rate
            - src (str): Source identifier ("NB")
        """
        if currencies is None:
            currencies = self.SUPPORTED_CURRENCIES
        
        # Get data for last 10 days to ensure we have latest rate
        end_date = datetime.now().date()
        start_date = end_date - timedelta(days=10)
        
        rates = self.get_historical_rates(
            currencies=currencies,
            start_date=start_date,
            end_date=end_date
        )

        # Keep latest point per currency for the "latest" API.
        latest_rates = {}
        for rate in rates:
            curr = rate["base"]
            if curr not in latest_rates or rate["date"] > latest_rates[curr]["date"]:
                latest_rates[curr] = rate

        return list(latest_rates.values())
    
    def get_historical_rates(
        self,
        currencies: Optional[list[str]] = None,
        start_date: Optional[datetime.date] = None,
        end_date: Optional[datetime.date] = None
    ) -> list[dict]:
        """
        Get historical exchange rates for specified period.
        
        Args:
            currencies: List of currency codes
            start_date: Start date (default: 30 days ago)
            end_date: End date (default: today)
            
        Returns:
            List of rate dictionaries
        """
        if currencies is None:
            currencies = self.SUPPORTED_CURRENCIES
        
        if end_date is None:
            end_date = datetime.now().date()
        if start_date is None:
            start_date = end_date - timedelta(days=30)
        
        # Build URL with query parameters
        params = {
            "format": "csv",
            "startPeriod": start_date.isoformat(),
            "endPeriod": end_date.isoformat(),
        }
        
        url = f"{self.BASE_URL}/?{urlencode(params)}"
        
        logger.info(
            f"Fetching Norges Bank rates for {len(currencies)} currencies "
            f"from {start_date} to {end_date}"
        )
        
        try:
            response = self.session.get(url, timeout=30)
            response.raise_for_status()
            
            csv_data = response.text
            # Historical endpoint should return full time series, not latest-only.
            rates = self._parse_csv(csv_data, currencies, latest_only=False)
            
            logger.info(f"Fetched {len(rates)} rates from Norges Bank")
            return rates
            
        except requests.RequestException as e:
            logger.error(f"Failed to fetch from Norges Bank: {e}")
            return []
    
    def _parse_csv(
        self,
        csv_data: str,
        target_currencies: list[str],
        latest_only: bool = True,
    ) -> list[dict]:
        """
        Parse Norges Bank CSV response.
        
        CSV format (semicolon-separated):
        Columns:
        - [2]: BASE_CURR (base currency, e.g., USD)
        - [4]: QUOTE_CURR (quote currency, always NOK)
        - [10]: UNIT_MULT (unit multiplier: 0=1, 2=100)
        - [14]: TIME_PERIOD (date: YYYY-MM-DD)
        - [15]: OBS_VALUE (rate value)
        """
        reader = csv.reader(StringIO(csv_data), delimiter=';')
        
        # Skip header row
        next(reader, None)
        
        rates = []
        latest_rates = {}
        
        for row in reader:
            if len(row) < 16:
                continue
            
            try:
                base_currency = row[2]
                quote_currency = row[4]
                unit_mult = int(row[10]) if row[10] else 0
                date_str = row[14]
                value_str = row[15]

                # Keep only daily points (YYYY-MM-DD).
                # Norges Bank payload can also include monthly/yearly aggregates.
                if len(date_str) != 10:
                    continue
                
                # Filter for target currencies and NOK quote
                if quote_currency != "NOK" or base_currency not in target_currencies:
                    continue
                
                # Parse value
                raw_value = float(value_str)
                
                # Adjust for unit multiplier (JPY uses 100-unit)
                if unit_mult == 2:
                    adjusted_value = raw_value / 100
                else:
                    adjusted_value = raw_value
                
                parsed_rate = {
                    "date": date_str,
                    "base": base_currency,
                    "quote": "NOK",
                    "value": adjusted_value,
                    "src": "NB"
                }

                if latest_only:
                    key = base_currency
                    if key not in latest_rates or date_str > latest_rates[key]["date"]:
                        latest_rates[key] = parsed_rate
                else:
                    rates.append(parsed_rate)
                    
            except (ValueError, IndexError) as e:
                logger.warning(f"Failed to parse CSV row: {e}")
                continue
        
        if latest_only:
            rates = list(latest_rates.values())
            logger.debug(f"Parsed {len(rates)} unique currency latest rates from CSV")
        else:
            logger.debug(f"Parsed {len(rates)} historical currency rates from CSV")

        return rates
