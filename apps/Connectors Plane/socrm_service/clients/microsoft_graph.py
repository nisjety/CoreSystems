"""Microsoft Graph API client for user information."""

import logging
from typing import Dict, Optional, Any

import requests
from requests.exceptions import RequestException

logger = logging.getLogger(__name__)


class MicrosoftGraphClient:
    """Client for Microsoft Graph API to fetch user information."""
    
    GRAPH_API_URL = "https://graph.microsoft.com/v1.0"
    
    def __init__(self, access_token: str):
        """Initialize Microsoft Graph client.
        
        Args:
            access_token: Microsoft OAuth access token
        """
        self.access_token = access_token
        self.session = self._create_session()
    
    def _create_session(self) -> requests.Session:
        """Create requests session with headers."""
        session = requests.Session()
        session.headers.update({
            "Authorization": f"Bearer {self.access_token}",
            "Content-Type": "application/json"
        })
        return session
    
    def get_user_profile(self) -> Dict[str, Any]:
        """Get user profile information.
        
        Returns:
            User profile data including name, email, location, etc.
        """
        try:
            response = self.session.get(f"{self.GRAPH_API_URL}/me")
            response.raise_for_status()
            return response.json()
        except RequestException as e:
            logger.error(f"Failed to get user profile: {e}")
            raise
    
    def get_user_location(self) -> Optional[str]:
        """Get user's preferred location/country from profile.
        
        Returns:
            Country code (e.g., 'NO', 'SE', 'US') or None
        """
        try:
            profile = self.get_user_profile()
            
            # Try multiple fields that might contain location
            country = (
                profile.get("country") or
                profile.get("usageLocation") or
                profile.get("officeLocation", "").split(",")[-1].strip()
            )
            
            if country:
                logger.info(f"User location detected: {country}")
                return country
            
            logger.warning("Could not determine user location from profile")
            return None
        
        except Exception as e:
            logger.error(f"Failed to get user location: {e}")
            return None
    
    def get_user_preferred_language(self) -> Optional[str]:
        """Get user's preferred language.
        
        Returns:
            Language code (e.g., 'nb-NO', 'en-US') or None
        """
        try:
            profile = self.get_user_profile()
            return profile.get("preferredLanguage")
        except Exception as e:
            logger.error(f"Failed to get user language: {e}")
            return None
    
    def get_user_currency_preference(self) -> Optional[str]:
        """Infer currency preference from user location.
        
        Returns:
            Currency code (e.g., 'NOK', 'SEK', 'USD') or None
        """
        location = self.get_user_location()
        
        if not location:
            return None
        
        # Map countries to currencies
        country_to_currency = {
            "NO": "NOK",
            "Norway": "NOK",
            "SE": "SEK",
            "Sweden": "SEK",
            "DK": "DKK",
            "Denmark": "DKK",
            "GB": "GBP",
            "UK": "GBP",
            "United Kingdom": "GBP",
            "US": "USD",
            "USA": "USD",
            "United States": "USD",
            "EU": "EUR",
            "DE": "EUR",
            "Germany": "EUR",
            "FR": "EUR",
            "France": "EUR",
            "ES": "EUR",
            "Spain": "EUR",
            "IT": "EUR",
            "Italy": "EUR",
            "AU": "AUD",
            "Australia": "AUD",
            "NZ": "NZD",
            "New Zealand": "NZD",
            "CA": "CAD",
            "Canada": "CAD",
            "IS": "ISK",
            "Iceland": "ISK",
        }
        
        currency = country_to_currency.get(location.upper()) or country_to_currency.get(location)
        
        if currency:
            logger.info(f"Currency preference: {currency} (from location: {location})")
        else:
            logger.warning(f"No currency mapping for location: {location}")
        
        return currency
    
    def get_organization_info(self) -> Dict[str, Any]:
        """Get user's organization information.
        
        Returns:
            Organization data
        """
        try:
            response = self.session.get(f"{self.GRAPH_API_URL}/organization")
            response.raise_for_status()
            
            org_data = response.json()
            if org_data.get("value"):
                return org_data["value"][0]
            return {}
        
        except RequestException as e:
            logger.error(f"Failed to get organization info: {e}")
            return {}
