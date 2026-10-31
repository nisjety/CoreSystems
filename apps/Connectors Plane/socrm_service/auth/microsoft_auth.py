"""Microsoft Azure AD OAuth handler for SuperOffice integration."""

import json
import logging
import secrets
from datetime import datetime, timedelta
from typing import Dict, Optional, Any
from urllib.parse import urlencode

import redis
import requests
from requests.exceptions import RequestException

from config.settings import Settings

logger = logging.getLogger(__name__)


class MicrosoftAuthHandler:
    """Handle Microsoft Azure AD OAuth 2.0 authentication."""
    
    def __init__(self, settings: Settings):
        """Initialize Microsoft Auth handler."""
        self.settings = settings
        # Use Redis for state storage to support multiple workers
        try:
            self.redis_client = redis.from_url(
                settings.redis_url,
                decode_responses=True,
                socket_connect_timeout=5
            )
            self.redis_client.ping()
            logger.info("Connected to Redis for state storage")
        except Exception as e:
            logger.warning(f"Failed to connect to Redis: {e}. Falling back to in-memory storage.")
            self.redis_client = None
        
        self.state_store: Dict[str, Dict[str, str]] = {}
        self.token_store: Dict[str, Dict[str, Any]] = {}
    
    def generate_state(self, organization_id: str, integration_type: str = "superoffice") -> str:
        """Generate secure state parameter for OAuth.
        
        Args:
            organization_id: Organization identifier
            integration_type: Type of integration (superoffice, general, etc.)
        
        Returns:
            State token
        """
        state = secrets.token_urlsafe(32)
        state_data = {
            "organization_id": organization_id,
            "integration_type": integration_type,
            "created_at": datetime.utcnow().isoformat()
        }
        
        # Store in Redis with 10 minute expiration, fallback to memory
        if self.redis_client:
            try:
                self.redis_client.setex(
                    f"oauth_state:{state}",
                    600,  # 10 minutes
                    json.dumps(state_data)
                )
            except Exception as e:
                logger.error(f"Failed to store state in Redis: {e}")
                self.state_store[state] = state_data
        else:
            self.state_store[state] = state_data
        
        return state
    
    def verify_state(self, state: str) -> Optional[Dict[str, str]]:
        """Verify state parameter and return metadata."""
        # Try Redis first
        if self.redis_client:
            try:
                state_json = self.redis_client.get(f"oauth_state:{state}")
                if state_json:
                    self.redis_client.delete(f"oauth_state:{state}")
                    return json.loads(state_json)
            except Exception as e:
                logger.error(f"Failed to retrieve state from Redis: {e}")
        
        # Fallback to memory
        state_data = self.state_store.pop(state, None)
        if not state_data:
            logger.warning(f"Invalid or expired state: {state}")
        return state_data
    
    def get_authorization_url(
        self,
        organization_id: str,
        scopes: Optional[list[str]] = None
    ) -> str:
        """Get Microsoft authorization URL.
        
        Args:
            organization_id: Organization identifier
            scopes: OAuth scopes (defaults to standard scopes)
        
        Returns:
            Authorization URL
        """
        if not scopes:
            scopes = [
                "openid",
                "profile",
                "email",
                "User.Read",
                "offline_access"
            ]
        
        state = self.generate_state(organization_id)
        
        params = {
            "client_id": self.settings.microsoft_client_id,
            "response_type": "code",
            "redirect_uri": self.settings.microsoft_redirect_uri,
            "response_mode": "query",
            "scope": " ".join(scopes),
            "state": state
        }
        
        auth_url = f"{self.settings.microsoft_auth_url}?{urlencode(params)}"
        logger.info(f"Generated Microsoft auth URL for organization: {organization_id}")
        
        return auth_url
    
    def handle_callback(self, code: str, state: str) -> Dict[str, Any]:
        """Handle OAuth callback and exchange code for tokens.
        
        Args:
            code: Authorization code
            state: State parameter
        
        Returns:
            Token data with organization info
        """
        # Verify state
        state_data = self.verify_state(state)
        if not state_data:
            raise ValueError("Invalid or expired state parameter")
        
        organization_id = state_data["organization_id"]
        
        # Exchange code for tokens
        tokens = self._exchange_code_for_tokens(code)
        
        # Store tokens with user info
        tokens["organization_id"] = organization_id
        tokens["integration_type"] = state_data["integration_type"]
        tokens["created_at"] = datetime.utcnow().isoformat()
        
        # Store in Redis with 90 day expiration (refresh token lifetime), fallback to memory
        if self.redis_client:
            try:
                self.redis_client.setex(
                    f"microsoft_tokens:{organization_id}",
                    7776000,  # 90 days (typical Microsoft refresh token lifetime)
                    json.dumps(tokens)
                )
            except Exception as e:
                logger.error(f"Failed to store tokens in Redis: {e}")
                self.token_store[organization_id] = tokens
        else:
            self.token_store[organization_id] = tokens
        
        logger.info(f"Successfully exchanged code for Microsoft tokens: {organization_id}")
        
        return tokens
    
    def _exchange_code_for_tokens(self, code: str) -> Dict[str, Any]:
        """Exchange authorization code for access and refresh tokens."""
        data = {
            "client_id": self.settings.microsoft_client_id,
            "client_secret": self.settings.microsoft_client_secret,
            "code": code,
            "redirect_uri": self.settings.microsoft_redirect_uri,
            "grant_type": "authorization_code"
        }
        
        try:
            response = requests.post(
                self.settings.microsoft_token_url,
                data=data,
                timeout=30
            )
            response.raise_for_status()
            
            tokens = response.json()
            return {
                "access_token": tokens["access_token"],
                "refresh_token": tokens.get("refresh_token"),
                "expires_in": tokens.get("expires_in", 3600),
                "token_type": tokens.get("token_type", "Bearer"),
                "id_token": tokens.get("id_token")
            }
        
        except RequestException as e:
            logger.error(f"Failed to exchange code for Microsoft tokens: {e}")
            raise
    
    def get_stored_tokens(self, organization_id: str) -> Optional[Dict[str, Any]]:
        """Get stored tokens for organization from Redis or memory.
        
        Args:
            organization_id: Organization identifier
        
        Returns:
            Token data or None if not found
        """
        # Try Redis first
        if self.redis_client:
            try:
                tokens_json = self.redis_client.get(f"microsoft_tokens:{organization_id}")
                if tokens_json:
                    return json.loads(tokens_json)
            except Exception as e:
                logger.error(f"Failed to retrieve tokens from Redis: {e}")
        
        # Fallback to memory
        return self.token_store.get(organization_id)
    
    def get_access_token(self, organization_id: str) -> str:
        """Get valid Microsoft access token for organization."""
        tokens = self.get_stored_tokens(organization_id)
        
        if not tokens:
            raise ValueError(f"No tokens found for organization: {organization_id}")
        
        # Check if token is expired
        if self._is_token_expired(tokens):
            logger.info(f"Microsoft token expired for {organization_id}, refreshing...")
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
        tokens = self.get_stored_tokens(organization_id)
        
        if not tokens or not tokens.get("refresh_token"):
            raise ValueError(f"No refresh token for organization: {organization_id}")
        
        data = {
            "client_id": self.settings.microsoft_client_id,
            "client_secret": self.settings.microsoft_client_secret,
            "refresh_token": tokens["refresh_token"],
            "grant_type": "refresh_token"
        }
        
        try:
            response = requests.post(
                self.settings.microsoft_token_url,
                data=data,
                timeout=30
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
            
            # Store updated tokens in Redis with 90 day expiration
            if self.redis_client:
                try:
                    self.redis_client.setex(
                        f"microsoft_tokens:{organization_id}",
                        7776000,  # 90 days
                        json.dumps(tokens)
                    )
                except Exception as e:
                    logger.error(f"Failed to store refreshed tokens in Redis: {e}")
            
            self.token_store[organization_id] = tokens
            
            logger.info(f"Refreshed Microsoft token for organization: {organization_id}")
            
            return tokens
        
        except RequestException as e:
            logger.error(f"Failed to refresh Microsoft token: {e}")
            raise
    
    def get_superoffice_token_with_microsoft(
        self,
        microsoft_access_token: str
    ) -> Optional[str]:
        """Exchange Microsoft token for SuperOffice token.
        
        SuperOffice supports Azure AD SSO, so we can use the Microsoft token
        to authenticate with SuperOffice CRM.
        
        Args:
            microsoft_access_token: Valid Microsoft access token
        
        Returns:
            SuperOffice access token or None
        """
        # This is a placeholder for SuperOffice SSO integration
        # In practice, SuperOffice needs to be configured to accept Azure AD tokens
        # The exact implementation depends on SuperOffice tenant configuration
        
        logger.info("Attempting to exchange Microsoft token for SuperOffice access")
        
        # TODO: Implement SuperOffice SSO token exchange
        # This typically involves:
        # 1. SuperOffice tenant configured with Azure AD
        # 2. Using Microsoft token to authenticate via SuperOffice SSO endpoint
        # 3. Receiving SuperOffice-specific token for API calls
        
        return None
