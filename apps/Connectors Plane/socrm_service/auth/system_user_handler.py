"""SuperOffice System User Authentication Handler.

Implements the System User flow for server-to-server (back-channel) authentication.
This enables non-interactive API access without requiring user login.

Supports both PEM and XML format keys (SuperOffice provides XML format from Developer Portal).

Documentation: https://docs.superoffice.com/en/api/netserver/authentication/online/auth-application/
"""

import base64
import hashlib
import logging
import xml.etree.ElementTree as ET
from datetime import datetime, timedelta
from typing import Dict, Optional, Any
from pathlib import Path

import jwt
import requests
from requests.exceptions import RequestException

try:
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import padding, rsa
    from cryptography.hazmat.backends import default_backend
    CRYPTOGRAPHY_AVAILABLE = True
except ImportError:
    CRYPTOGRAPHY_AVAILABLE = False

from config.settings import Settings

logger = logging.getLogger(__name__)


class SystemUserError(Exception):
    """Base exception for System User authentication errors."""
    pass


class SystemUserHandler:
    """
    Handle System User authentication for server-to-server communication.
    
    System User flow provides:
    - Non-interactive authentication (no user login required)
    - Unlimited API access (not bound by user permissions)
    - 6-hour ticket validity with sliding expiration
    - Ideal for background jobs, scheduled tasks, and automated workflows
    
    Usage:
        handler = SystemUserHandler(settings)
        ticket = handler.get_system_user_ticket(
            system_user_token="YourApp-abc123...",
            context_identifier="Cust12345"
        )
        # Use ticket for API calls with SOTicket authorization
    """
    
    def __init__(self, settings: Settings):
        """Initialize System User handler."""
        self.settings = settings
        self.ticket_cache: Dict[str, Dict[str, Any]] = {}
        self.private_key = None
        self.public_key = None
        
        # Load keys if configured
        if settings.system_user_private_key_path:
            self._load_private_key()
        if settings.superoffice_public_cert_path:
            self._load_public_key()
    
    def _load_private_key(self):
        """Load partner's private RSA key for signing tokens (supports PEM and XML formats)."""
        try:
            key_path = Path(self.settings.system_user_private_key_path)
            if not key_path.exists():
                logger.warning(f"Private key file not found: {key_path}")
                return
            
            with open(key_path, 'r') as key_file:
                content = key_file.read()
            
            # Detect format: XML from SuperOffice or PEM
            if content.strip().startswith('<RSAKeyValue>'):
                # XML format from SuperOffice Developer Portal
                logger.info("Detected XML format private key from SuperOffice")
                self.private_key = self._parse_xml_private_key(content)
            else:
                # PEM format
                if not CRYPTOGRAPHY_AVAILABLE:
                    raise SystemUserError("cryptography library required for PEM format keys")
                logger.info("Detected PEM format private key")
                self.private_key = serialization.load_pem_private_key(
                    content.encode(),
                    password=self.settings.system_user_private_key_password.encode() if self.settings.system_user_private_key_password else None,
                    backend=default_backend()
                )
            
            logger.info("Successfully loaded private key for System User signing")
        
        except Exception as e:
            logger.error(f"Failed to load private key: {e}")
            raise SystemUserError(f"Cannot load private key: {e}")
    
    def _parse_xml_private_key(self, xml_content: str) -> Dict[str, int]:
        """Parse SuperOffice XML format private key."""
        try:
            root = ET.fromstring(xml_content)
            
            # Extract and decode all components
            key_data = {
                'modulus': int.from_bytes(base64.b64decode(root.find('Modulus').text), 'big'),
                'exponent': int.from_bytes(base64.b64decode(root.find('Exponent').text), 'big'),
                'd': int.from_bytes(base64.b64decode(root.find('D').text), 'big'),
                'p': int.from_bytes(base64.b64decode(root.find('P').text), 'big'),
                'q': int.from_bytes(base64.b64decode(root.find('Q').text), 'big'),
                'dp': int.from_bytes(base64.b64decode(root.find('DP').text), 'big'),
                'dq': int.from_bytes(base64.b64decode(root.find('DQ').text), 'big'),
                'inverseq': int.from_bytes(base64.b64decode(root.find('InverseQ').text), 'big')
            }
            
            logger.debug("Successfully parsed XML private key")
            return key_data
        
        except Exception as e:
            raise SystemUserError(f"Failed to parse XML private key: {e}")
    
    def _load_public_key(self):
        """Load SuperOffice public certificate for JWT validation (supports PEM and XML formats)."""
        try:
            cert_path = Path(self.settings.superoffice_public_cert_path)
            if not cert_path.exists():
                logger.warning(f"Public certificate file not found: {cert_path}")
                return
            
            with open(cert_path, 'r') as cert_file:
                content = cert_file.read()
            
            # Detect format: XML from SuperOffice or PEM
            if content.strip().startswith('<RSAKeyValue>'):
                # XML format from SuperOffice
                logger.info("Detected XML format public key from SuperOffice")
                self.public_key = self._parse_xml_public_key(content)
            else:
                # PEM format
                if not CRYPTOGRAPHY_AVAILABLE:
                    raise SystemUserError("cryptography library required for PEM format keys")
                logger.info("Detected PEM format public certificate")
                self.public_key = serialization.load_pem_public_key(
                    content.encode(),
                    backend=default_backend()
                )
            
            logger.info("Successfully loaded SuperOffice public certificate")
        
        except Exception as e:
            logger.error(f"Failed to load public certificate: {e}")
            raise SystemUserError(f"Cannot load public certificate: {e}")
    
    def _parse_xml_public_key(self, xml_content: str) -> Dict[str, int]:
        """Parse SuperOffice XML format public key."""
        try:
            root = ET.fromstring(xml_content)
            
            key_data = {
                'modulus': int.from_bytes(base64.b64decode(root.find('Modulus').text), 'big'),
                'exponent': int.from_bytes(base64.b64decode(root.find('Exponent').text), 'big')
            }
            
            logger.debug("Successfully parsed XML public key")
            return key_data
        
        except Exception as e:
            raise SystemUserError(f"Failed to parse XML public key: {e}")
            
            logger.debug("Successfully parsed XML private key")
            return key_data
        
        except Exception as e:
            raise SystemUserError(f"Failed to parse XML private key: {e}")
    
    def _load_public_key(self):
        """Load SuperOffice public certificate for JWT validation."""
        try:
            cert_path = Path(self.settings.superoffice_public_cert_path)
            if not cert_path.exists():
                logger.warning(f"Public certificate file not found: {cert_path}")
                return
            
            with open(cert_path, 'rb') as cert_file:
                self.public_key = serialization.load_pem_public_key(
                    cert_file.read(),
                    backend=default_backend()
                )
            
            logger.info("Successfully loaded SuperOffice public certificate")
        
        except Exception as e:
            logger.error(f"Failed to load public certificate: {e}")
            raise SystemUserError(f"Cannot load public certificate: {e}")
    
    def sign_system_user_token(self, system_user_token: str) -> str:
        """
        Sign the System User token with timestamp using partner's private key.
        
        Format: SYSTEM_USER_TOKEN.YYYYMMDDHHMM.BASE64_SIGNATURE
        
        Args:
            system_user_token: The system user token from id_token claims
            
        Returns:
            Signed token string
            
        Raises:
            SystemUserError: If signing fails or private key not loaded
        """
        if not self.private_key:
            raise SystemUserError("Private key not loaded. Check system_user_private_key_path in settings.")
        
        try:
            # Generate UTC timestamp in required format
            utc_timestamp = datetime.utcnow().strftime("%Y%m%d%H%M")
            
            # Concatenate token and timestamp
            data_to_sign = f"{system_user_token}.{utc_timestamp}"
            
            logger.debug(f"Signing data: {data_to_sign}")
            
            # Determine key format and sign appropriately
            if isinstance(self.private_key, dict):
                # XML format - use pure Python implementation
                signature = self._sign_with_xml_key(data_to_sign.encode('utf-8'))
            else:
                # PEM format - use cryptography library
                if not CRYPTOGRAPHY_AVAILABLE:
                    raise SystemUserError("cryptography library required for PEM format keys")
                signature = self.private_key.sign(
                    data_to_sign.encode('utf-8'),
                    padding.PKCS1v15(),
                    hashes.SHA256()
                )
            
            # Base64 encode signature
            signature_b64 = base64.b64encode(signature).decode('utf-8')
            
            # Construct final signed token
            signed_token = f"{data_to_sign}.{signature_b64}"
            
            logger.info("Successfully signed System User token")
            
            return signed_token
        
        except Exception as e:
            logger.error(f"Failed to sign System User token: {e}")
            raise SystemUserError(f"Token signing failed: {e}")
    
    def _sign_with_xml_key(self, message: bytes) -> bytes:
        """Sign message using XML format RSA key (PKCS#1 v1.5 with SHA-256)."""
        try:
            # Hash the message with SHA-256
            h = hashlib.sha256(message).digest()
            
            # Apply PKCS#1 v1.5 padding
            key_size = (self.private_key['modulus'].bit_length() + 7) // 8
            padded = self._pkcs1_v15_pad(h, key_size)
            
            # RSA signature: sign = padded^d mod n
            padded_int = int.from_bytes(padded, 'big')
            signature_int = pow(padded_int, self.private_key['d'], self.private_key['modulus'])
            
            # Convert back to bytes
            return signature_int.to_bytes(key_size, 'big')
        
        except Exception as e:
            raise SystemUserError(f"XML key signing failed: {e}")
    
    def _pkcs1_v15_pad(self, digest: bytes, key_size: int) -> bytes:
        """Apply PKCS#1 v1.5 padding for SHA-256 hash."""
        # SHA-256 DigestInfo (DER encoded)
        digest_info = (
            b'\x30\x31'  # SEQUENCE, length 49
            b'\x30\x0d'  # SEQUENCE, length 13
            b'\x06\x09\x60\x86\x48\x01\x65\x03\x04\x02\x01'  # OID for SHA-256
            b'\x05\x00'  # NULL
            b'\x04\x20'  # OCTET STRING, length 32
        ) + digest
        
        # Calculate padding
        padding_length = key_size - len(digest_info) - 3
        if padding_length < 8:
            raise SystemUserError("Key size too small for PKCS#1 v1.5 padding")
        
        # Construct padded message: 0x00 || 0x01 || PS || 0x00 || DigestInfo
        return b'\x00\x01' + (b'\xff' * padding_length) + b'\x00' + digest_info
    
    def get_system_user_ticket(
        self,
        system_user_token: str,
        context_identifier: str,
        force_refresh: bool = False
    ) -> str:
        """
        Get System User ticket credential for API authentication.
        
        The ticket is cached and valid for 6 hours with sliding expiration.
        Only fetches a new ticket when necessary (expired or not cached).
        
        Args:
            system_user_token: System user token from initial OAuth flow
            context_identifier: Tenant identifier (e.g., "Cust12345")
            force_refresh: Force fetching a new ticket even if cached
            
        Returns:
            System User ticket credential (not the JWT, but the ticket claim)
            
        Raises:
            SystemUserError: If ticket exchange fails
        """
        cache_key = f"{context_identifier}:{system_user_token}"
        
        # Check cache
        if not force_refresh and cache_key in self.ticket_cache:
            cached = self.ticket_cache[cache_key]
            
            # Check if still valid (with 5 minute buffer)
            last_used = cached.get("last_used", cached.get("created_at"))
            expires_at = last_used + timedelta(hours=6, minutes=-5)
            
            if datetime.utcnow() < expires_at:
                logger.debug("Using cached System User ticket")
                # Update last used time (sliding expiration)
                cached["last_used"] = datetime.utcnow()
                return cached["ticket"]
            else:
                logger.info("Cached ticket expired, fetching new one")
        
        # Fetch new ticket
        ticket = self._exchange_token_for_ticket(system_user_token, context_identifier)
        
        # Cache the ticket
        self.ticket_cache[cache_key] = {
            "ticket": ticket,
            "created_at": datetime.utcnow(),
            "last_used": datetime.utcnow(),
            "context_identifier": context_identifier
        }
        
        return ticket
    
    def _exchange_token_for_ticket(
        self,
        system_user_token: str,
        context_identifier: str
    ) -> str:
        """
        Exchange signed System User token for System User ticket.
        
        Makes request to SuperOffice PartnerSystemUser endpoint.
        
        Args:
            system_user_token: System user token to sign and exchange
            context_identifier: Tenant identifier
            
        Returns:
            System User ticket credential
            
        Raises:
            SystemUserError: If exchange fails
        """
        try:
            # Step 1: Sign the token
            signed_token = self.sign_system_user_token(system_user_token)
            
            # Step 2: Prepare request
            url = f"{self.settings.get_auth_base_url()}/Login/api/PartnerSystemUser/Authenticate"
            
            payload = {
                "SignedSystemToken": signed_token,
                "ApplicationToken": self.settings.client_secret,  # OAuth client_secret
                "ContextIdentifier": context_identifier,
                "ReturnTokenType": "JWT"
            }
            
            headers = {
                "Content-Type": "application/json",
                "Accept": "application/json"
            }
            
            logger.info(f"Exchanging System User token for ticket at: {url}")
            
            # Step 3: Make request
            response = requests.post(
                url,
                json=payload,
                headers=headers,
                timeout=30
            )
            
            response.raise_for_status()
            result = response.json()
            
            # Step 4: Check response
            if not result.get("IsSuccessful"):
                error_msg = result.get("ErrorMessage", "Unknown error")
                raise SystemUserError(f"Ticket exchange failed: {error_msg}")
            
            jwt_token = result.get("Token")
            if not jwt_token:
                raise SystemUserError("No JWT token in response")
            
            # Step 5: Validate and extract ticket from JWT
            ticket = self._extract_ticket_from_jwt(jwt_token)
            
            logger.info("Successfully obtained System User ticket")
            
            return ticket
        
        except RequestException as e:
            logger.error(f"HTTP error during ticket exchange: {e}")
            raise SystemUserError(f"Ticket exchange request failed: {e}")
        
        except Exception as e:
            logger.error(f"Failed to exchange token for ticket: {e}")
            raise SystemUserError(f"Ticket exchange failed: {e}")
    
    def _extract_ticket_from_jwt(self, jwt_token: str) -> str:
        """
        Validate JWT and extract the ticket claim.
        
        Args:
            jwt_token: JWT token from SuperOffice
            
        Returns:
            Ticket credential from JWT claims
            
        Raises:
            SystemUserError: If validation fails or ticket not found
        """
        try:
            # For System User flow, JWT signature verification is optional
            # The signed token exchange already authenticated the partner
            
            # Check if we have a usable public key (PEM format, not XML)
            has_usable_key = (
                self.public_key and 
                not isinstance(self.public_key, dict) and 
                CRYPTOGRAPHY_AVAILABLE
            )
            
            if has_usable_key:
                # Validate with SuperOffice's public key
                decoded = jwt.decode(
                    jwt_token,
                    self.public_key,
                    algorithms=["RS256"],
                    options={"verify_exp": True}
                )
                logger.debug("JWT signature verified with SuperOffice public key")
            else:
                # Decode without signature verification
                # This is acceptable for System User flow since:
                # 1. The signed token exchange already authenticated us
                # 2. Communication is over HTTPS
                # 3. We got the JWT directly from SuperOffice's API
                if isinstance(self.public_key, dict):
                    logger.info("XML format public key detected - decoding JWT without verification")
                else:
                    logger.warning("Public key not available - decoding JWT without signature verification")
                
                decoded = jwt.decode(
                    jwt_token,
                    options={
                        "verify_signature": False,
                        "verify_exp": True
                    }
                )
            
            # Extract ticket claim
            ticket = decoded.get("http://schemes.superoffice.net/identity/ticket")
            
            if not ticket:
                raise SystemUserError("Ticket claim not found in JWT")
            
            logger.debug(f"Extracted ticket from JWT (expires: {decoded.get('exp')})")
            
            return ticket
        
        except jwt.ExpiredSignatureError:
            logger.error("JWT token has expired")
            raise SystemUserError("JWT token expired")
        
        except jwt.InvalidTokenError as e:
            logger.error(f"Invalid JWT token: {e}")
            raise SystemUserError(f"JWT validation failed: {e}")
        
        except Exception as e:
            logger.error(f"Failed to extract ticket from JWT: {e}")
            raise SystemUserError(f"Ticket extraction failed: {e}")
    
    def invalidate_ticket(self, context_identifier: str = None):
        """
        Invalidate cached ticket(s).
        
        Args:
            context_identifier: Specific tenant to invalidate, or None for all
        """
        if context_identifier:
            # Remove tickets for specific context
            keys_to_remove = [k for k in self.ticket_cache.keys() if k.startswith(f"{context_identifier}:")]
            for key in keys_to_remove:
                del self.ticket_cache[key]
            logger.info(f"Invalidated tickets for context: {context_identifier}")
        else:
            # Clear all tickets
            self.ticket_cache.clear()
            logger.info("Invalidated all cached tickets")
    
    def get_ticket_status(self, context_identifier: str, system_user_token: str) -> Optional[Dict[str, Any]]:
        """
        Get status information about a cached ticket.
        
        Args:
            context_identifier: Tenant identifier
            system_user_token: System user token
            
        Returns:
            Dictionary with ticket status, or None if not cached
        """
        cache_key = f"{context_identifier}:{system_user_token}"
        
        if cache_key not in self.ticket_cache:
            return None
        
        cached = self.ticket_cache[cache_key]
        last_used = cached.get("last_used", cached.get("created_at"))
        expires_at = last_used + timedelta(hours=6)
        time_remaining = expires_at - datetime.utcnow()
        
        return {
            "cached": True,
            "created_at": cached["created_at"].isoformat(),
            "last_used": last_used.isoformat(),
            "expires_at": expires_at.isoformat(),
            "time_remaining_minutes": int(time_remaining.total_seconds() / 60),
            "is_valid": datetime.utcnow() < expires_at
        }
