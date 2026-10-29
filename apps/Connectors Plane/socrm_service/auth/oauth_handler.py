"""SuperOffice OAuth Handler."""

import hashlib
import logging
import secrets
from datetime import datetime, timedelta
from typing import Dict, Optional, Any
from urllib.parse import urlencode

import jwt
import requests
from requests.exceptions import RequestException

from config.settings import Settings

logger = logging.getLogger(__name__)


class SuperOfficeOAuthHandler:
    """Handle OAuth 2.0 authentication with SuperOffice CRM."""
    
    def __init__(self, settings: Settings):
        """Initialize OAuth handler."""
        self.settings = settings
        self.state_store: Dict[str, str] = {}  # In production, use Redis or database
        self.token_store: Dict[str, Dict[str, Any]] = {}  # In production, use database with encryption
    
    def generate_state(self, organization_id: str) -> str:
        """Generate a secure state parameter for OAuth."""
        state = secrets.token_urlsafe(32)
        self.state_store[state] = organization_id
        return state
    
    def verify_state(self, state: str) -> Optional[str]:
        """Verify state parameter and return organization_id."""
        organization_id = self.state_store.pop(state, None)
        if not organization_id:
            logger.warning(f"Invalid or expired state: {state}")
        return organization_id
    
    def get_authorization_url(self, organization_id: str) -> str:
        """Get SuperOffice authorization URL."""
        state = self.generate_state(organization_id)
        
        params = {
            "client_id": self.settings.client_id,
            "redirect_uri": self.settings.redirect_uri,
            "response_type": "code",
            "scope": self.settings.scopes,
            "state": state
        }
        
        auth_url = f"{self.settings.auth_url}?{urlencode(params)}"
        logger.info(f"Generated authorization URL for organization: {organization_id}")
        
        return auth_url
    
    def handle_callback(self, code: str, state: str) -> Dict[str, Any]:
        """Handle OAuth callback and exchange code for tokens."""
        # Verify state
        organization_id = self.verify_state(state)
        if not organization_id:
            raise ValueError("Invalid or expired state parameter")
        
        # Exchange code for tokens
        tokens = self._exchange_code_for_tokens(code)
        
        # Store tokens
        tokens["organization_id"] = organization_id
        tokens["created_at"] = datetime.utcnow().isoformat()
        self.token_store[organization_id] = tokens
        
        logger.info(f"Successfully exchanged code for tokens for organization: {organization_id}")
        
        return tokens
    
    def _exchange_code_for_tokens(self, code: str) -> Dict[str, Any]:
        """Exchange authorization code for access and refresh tokens."""
        data = {
            "grant_type": "authorization_code",
            "client_id": self.settings.client_id,
            "client_secret": self.settings.client_secret,
            "code": code,
            "redirect_uri": self.settings.redirect_uri
        }
        
        try:
            response = requests.post(
                self.settings.token_url,
                data=data,
                timeout=self.settings.request_timeout
            )
            response.raise_for_status()
            
            tokens = response.json()
            return {
                "access_token": tokens["access_token"],
                "refresh_token": tokens.get("refresh_token"),
                "expires_in": tokens.get("expires_in", 3600),
                "token_type": tokens.get("token_type", "Bearer")
            }
        
        except RequestException as e:
            logger.error(f"Failed to exchange code for tokens: {e}")
            raise
    
    def get_access_token(self, organization_id: str) -> str:
        """Get valid access token for organization."""
        tokens = self.token_store.get(organization_id)
        
        if not tokens:
            raise ValueError(f"No tokens found for organization: {organization_id}")
        
        # Check if token is expired
        if self._is_token_expired(tokens):
            logger.info(f"Access token expired for organization: {organization_id}, refreshing...")
            tokens = self._refresh_access_token(organization_id)
        
        return tokens["access_token"]
    
    def _is_token_expired(self, tokens: Dict[str, Any]) -> bool:
        """Check if access token is expired."""
        created_at = datetime.fromisoformat(tokens.get("created_at", datetime.utcnow().isoformat()))
        expires_in = tokens.get("expires_in", 3600)
        
        # Add 5 minute buffer
        expiry_time = created_at + timedelta(seconds=expires_in - 300)
        
        return datetime.utcnow() >= expiry_time
    
    def _refresh_access_token(self, organization_id: str) -> Dict[str, Any]:
        """Refresh access token using refresh token."""
        tokens = self.token_store.get(organization_id)
        
        if not tokens or not tokens.get("refresh_token"):
            raise ValueError(f"No refresh token available for organization: {organization_id}")
        
        data = {
            "grant_type": "refresh_token",
            "client_id": self.settings.client_id,
            "client_secret": self.settings.client_secret,
            "refresh_token": tokens["refresh_token"]
        }
        
        try:
            response = requests.post(
                self.settings.token_url,
                data=data,
                timeout=self.settings.request_timeout
            )
            response.raise_for_status()
            
            new_tokens = response.json()
            
            # Update stored tokens
            tokens.update({
                "access_token": new_tokens["access_token"],
                "refresh_token": new_tokens.get("refresh_token", tokens["refresh_token"]),
                "expires_in": new_tokens.get("expires_in", 3600),
                "created_at": datetime.utcnow().isoformat()
            })
            
            self.token_store[organization_id] = tokens
            
            logger.info(f"Successfully refreshed access token for organization: {organization_id}")
            
            return tokens
        
        except RequestException as e:
            logger.error(f"Failed to refresh access token: {e}")
            raise
    
    def revoke_token(self, organization_id: str) -> bool:
        """Revoke tokens for organization."""
        if organization_id in self.token_store:
            del self.token_store[organization_id]
            logger.info(f"Revoked tokens for organization: {organization_id}")
            return True
        return False
