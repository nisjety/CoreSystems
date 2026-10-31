#!/usr/bin/env python3
"""
Extract System User Token from SuperOffice OAuth id_token.

This script helps you extract the System User Token and Context Identifier
from the id_token received during initial OAuth authorization.

Usage:
    python extract_system_user_token.py <id_token>

Example:
    python extract_system_user_token.py "eyJhbGciOiJSUzI1NiIsImtpZCI6..."
"""

import sys
import jwt
from typing import Dict, Optional


def extract_tokens(id_token: str) -> Dict[str, Optional[str]]:
    """
    Extract System User Token and Context Identifier from id_token.
    
    Args:
        id_token: JWT token from OAuth callback
        
    Returns:
        Dictionary with system_user_token and context_identifier
    """
    try:
        # Decode without signature verification
        # (We just need to read the claims, validation happens later)
        decoded = jwt.decode(
            id_token,
            options={"verify_signature": False}
        )
        
        # Extract SuperOffice-specific claims
        system_user_token = decoded.get(
            "http://schemes.superoffice.net/identity/system_user_token"
        )
        
        context_identifier = decoded.get(
            "http://schemes.superoffice.net/identity/ctx"
        )
        
        # Also extract some useful information
        webapi_url = decoded.get(
            "http://schemes.superoffice.net/identity/webapi_url"
        )
        
        netserver_url = decoded.get(
            "http://schemes.superoffice.net/identity/netserver_url"
        )
        
        associate_id = decoded.get(
            "http://schemes.superoffice.net/identity/associateid"
        )
        
        return {
            "system_user_token": system_user_token,
            "context_identifier": context_identifier,
            "webapi_url": webapi_url,
            "netserver_url": netserver_url,
            "associate_id": associate_id,
            "all_claims": decoded
        }
    
    except jwt.DecodeError as e:
        print(f"❌ Error: Invalid JWT token format: {e}")
        return {}
    except Exception as e:
        print(f"❌ Error: Failed to decode token: {e}")
        return {}


def print_results(tokens: Dict[str, Optional[str]]):
    """Print extracted tokens in a user-friendly format."""
    
    if not tokens:
        return
    
    print("\n" + "="*60)
    print("SuperOffice System User Token Extraction")
    print("="*60)
    
    # Essential values
    print("\n✅ REQUIRED FOR SYSTEM USER AUTHENTICATION:\n")
    
    if tokens.get("system_user_token"):
        print(f"System User Token:")
        print(f"  {tokens['system_user_token']}")
    else:
        print("⚠️  System User Token: NOT FOUND")
        print("   Your application may not be configured as 'Server to Server'")
    
    print()
    
    if tokens.get("context_identifier"):
        print(f"Context Identifier (Tenant ID):")
        print(f"  {tokens['context_identifier']}")
    else:
        print("⚠️  Context Identifier: NOT FOUND")
    
    # Additional useful information
    print("\n📋 ADDITIONAL INFORMATION:\n")
    
    if tokens.get("webapi_url"):
        print(f"Web API URL: {tokens['webapi_url']}")
    
    if tokens.get("netserver_url"):
        print(f"NetServer URL: {tokens['netserver_url']}")
    
    if tokens.get("associate_id"):
        print(f"Associate ID: {tokens['associate_id']}")
    
    # Generate .env entries
    if tokens.get("system_user_token") and tokens.get("context_identifier"):
        print("\n" + "="*60)
        print("ADD TO YOUR .env FILE:")
        print("="*60)
        print()
        print(f'SOCRM_SYSTEM_USER_TOKEN="{tokens["system_user_token"]}"')
        print(f'SOCRM_CONTEXT_IDENTIFIER="{tokens["context_identifier"]}"')
        print()
        print("="*60)
    
    # Show all claims for debugging
    if tokens.get("all_claims"):
        print("\n🔍 ALL JWT CLAIMS (for debugging):\n")
        for key, value in tokens["all_claims"].items():
            # Truncate long values
            value_str = str(value)
            if len(value_str) > 80:
                value_str = value_str[:77] + "..."
            print(f"  {key}: {value_str}")


def main():
    """Main entry point."""
    
    print("\n🔐 SuperOffice System User Token Extractor\n")
    
    # Check command line arguments
    if len(sys.argv) < 2:
        print("Usage: python extract_system_user_token.py <id_token>")
        print()
        print("Where <id_token> is the JWT token from the OAuth callback URL.")
        print()
        print("Example:")
        print('  python extract_system_user_token.py "eyJhbGciOiJSUzI1NiIsImtpZCI6..."')
        print()
        print("The id_token is found in the URL fragment after OAuth authorization:")
        print("  http://your-callback-url#id_token=eyJhbGciOiJS...")
        print()
        sys.exit(1)
    
    id_token = sys.argv[1]
    
    # Remove common prefixes if user copied the full URL fragment
    if "id_token=" in id_token:
        # Extract just the token part
        parts = id_token.split("id_token=")
        if len(parts) > 1:
            id_token = parts[1].split("&")[0]
    
    # Extract tokens
    tokens = extract_tokens(id_token)
    
    # Print results
    print_results(tokens)
    
    # Check if successful
    if tokens.get("system_user_token") and tokens.get("context_identifier"):
        print("\n✅ SUCCESS! You can now use System User authentication.")
        print("   Follow the setup guide in SYSTEM_USER_SETUP_GUIDE.md")
        sys.exit(0)
    else:
        print("\n❌ INCOMPLETE: Missing required tokens.")
        print("   Verify your application is configured as 'Server to Server'")
        print("   in the SuperOffice Developer Portal.")
        sys.exit(1)


if __name__ == "__main__":
    main()
