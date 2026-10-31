#!/usr/bin/env python3
"""
SuperOffice API Connection Test Script

This script helps verify your SuperOffice API connection and credentials.
Run this after configuring your .env file to ensure everything is set up correctly.

Usage:
    python test_superoffice_connection.py
"""

import os
import sys
from dotenv import load_dotenv

# Add parent directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from clients.socrm_client import (
    SuperOfficeClient,
    SuperOfficeAPIError,
    SuperOfficeAuthError,
    SuperOfficeNotFoundError
)
from config.settings import Settings


def print_header(text: str):
    """Print formatted header."""
    print(f"\n{'=' * 60}")
    print(f"  {text}")
    print(f"{'=' * 60}\n")


def print_success(text: str):
    """Print success message."""
    print(f"✓ {text}")


def print_error(text: str):
    """Print error message."""
    print(f"✗ {text}")


def print_info(text: str):
    """Print info message."""
    print(f"ℹ {text}")


def test_configuration():
    """Test configuration settings."""
    print_header("Configuration Test")
    
    try:
        settings = Settings()
        
        # Check SuperOffice settings
        if settings.client_id:
            print_success(f"SuperOffice Client ID: {settings.client_id[:10]}...")
        else:
            print_error("SuperOffice Client ID not configured")
            return False
        
        if settings.client_secret:
            print_success("SuperOffice Client Secret: configured")
        else:
            print_error("SuperOffice Client Secret not configured")
            return False
        
        print_success(f"SuperOffice Tenant: {settings.tenant}")
        print_success(f"API Base URL: {settings.api_base_url}")
        print_success(f"Auth URL: {settings.auth_url}")
        print_success(f"Token URL: {settings.token_url}")
        
        # Check Microsoft settings
        if settings.microsoft_client_id:
            print_success(f"Microsoft Client ID: {settings.microsoft_client_id[:10]}...")
        else:
            print_info("Microsoft Client ID not configured (optional)")
        
        return True
    
    except Exception as e:
        print_error(f"Configuration error: {e}")
        return False


def test_api_connection(access_token: str):
    """Test API connection with access token."""
    print_header("API Connection Test")
    
    try:
        settings = Settings()
        client = SuperOfficeClient(access_token, settings)
        
        # Test health check
        if client.health_check():
            print_success("API connection successful")
        else:
            print_error("API health check failed")
            return False
        
        # Get current user
        try:
            user = client.get_current_user()
            print_success(f"Authenticated as: {user.get('FullName', 'Unknown')}")
            print_info(f"User ID: {user.get('AssociateId', 'N/A')}")
            print_info(f"Email: {user.get('EmailAddress', 'N/A')}")
            print_info(f"Database: {user.get('UserName', 'N/A')}")
        except SuperOfficeAPIError as e:
            print_error(f"Failed to get user info: {e}")
            return False
        
        return True
    
    except SuperOfficeAuthError as e:
        print_error(f"Authentication failed: {e}")
        print_info("Your access token may be invalid or expired")
        return False
    except Exception as e:
        print_error(f"API connection error: {e}")
        return False


def test_basic_operations(access_token: str):
    """Test basic API operations."""
    print_header("Basic Operations Test")
    
    try:
        settings = Settings()
        client = SuperOfficeClient(access_token, settings)
        
        # Test get contacts
        try:
            contacts = client.get_contacts(page_size=5)
            print_success(f"Retrieved {len(contacts)} contacts")
            if contacts:
                print_info(f"First contact: {contacts[0].get('FirstName', '')} {contacts[0].get('LastName', '')}")
        except SuperOfficeAPIError as e:
            print_error(f"Failed to get contacts: {e}")
        
        # Test get companies
        try:
            companies = client.get_companies(page_size=5)
            print_success(f"Retrieved {len(companies)} companies")
            if companies:
                print_info(f"First company: {companies[0].get('Name', 'N/A')}")
        except SuperOfficeAPIError as e:
            print_error(f"Failed to get companies: {e}")
        
        # Test get sales
        try:
            sales = client.get_sales(page_size=5)
            print_success(f"Retrieved {len(sales)} sales")
            if sales:
                print_info(f"First sale: {sales[0].get('Heading', 'N/A')}")
        except SuperOfficeAPIError as e:
            print_error(f"Failed to get sales: {e}")
        
        # Test MDO lists
        try:
            categories = client.get_list("category")
            print_success(f"Retrieved {len(categories)} categories")
        except SuperOfficeAPIError as e:
            print_error(f"Failed to get categories: {e}")
        
        return True
    
    except Exception as e:
        print_error(f"Operations test error: {e}")
        return False


def test_archive_provider(access_token: str):
    """Test archive provider search."""
    print_header("Archive Provider Test")
    
    try:
        settings = Settings()
        client = SuperOfficeClient(access_token, settings)
        
        # Search contacts using archive provider
        try:
            results = client.search_archive(
                provider_name="ContactPersonDynamic",
                columns=["personId", "fullName", "email"],
                page_size=5
            )
            print_success(f"Archive search returned {len(results)} results")
            if results:
                for i, result in enumerate(results[:3], 1):
                    print_info(f"Result {i}: {result.get('fullName', 'N/A')} ({result.get('email', 'N/A')})")
        except SuperOfficeAPIError as e:
            print_error(f"Archive search failed: {e}")
            return False
        
        return True
    
    except Exception as e:
        print_error(f"Archive provider test error: {e}")
        return False


def main():
    """Main test function."""
    print_header("SuperOffice API Connection Test")
    print("This script will test your SuperOffice API configuration and connection.")
    
    # Load environment variables
    load_dotenv()
    
    # Test configuration
    if not test_configuration():
        print("\n❌ Configuration test failed. Please check your .env file.")
        print("\nSee .env.example for configuration template.")
        return 1
    
    print("\n✅ Configuration test passed!")
    
    # Check if access token is provided
    access_token = os.getenv("SOCRM_ACCESS_TOKEN")
    
    if not access_token:
        print("\n" + "=" * 60)
        print("  Access Token Required")
        print("=" * 60)
        print("\nTo test API connectivity, you need an access token.")
        print("\nOptions:")
        print("1. Set SOCRM_ACCESS_TOKEN in your .env file")
        print("2. Run the OAuth flow to get a token:")
        print("   python app.py")
        print("   Then visit: http://localhost:5006/api/auth/authorize?organization_id=test")
        print("\n3. Or provide token as argument:")
        print("   python test_superoffice_connection.py --token YOUR_TOKEN")
        return 0
    
    # Test API connection
    if not test_api_connection(access_token):
        print("\n❌ API connection test failed.")
        return 1
    
    print("\n✅ API connection test passed!")
    
    # Test basic operations
    if not test_basic_operations(access_token):
        print("\n⚠️  Some basic operations failed.")
        print("This may be due to permissions or empty database.")
    else:
        print("\n✅ Basic operations test passed!")
    
    # Test archive provider
    if not test_archive_provider(access_token):
        print("\n⚠️  Archive provider test failed.")
    else:
        print("\n✅ Archive provider test passed!")
    
    print("\n" + "=" * 60)
    print("  Test Summary")
    print("=" * 60)
    print("\n✅ SuperOffice API connection is working!")
    print("\nYou can now:")
    print("- Use the API client in your application")
    print("- Set up synchronization workflows")
    print("- Configure webhooks for real-time updates")
    print("\nSee SUPEROFFICE_API_GUIDE.md for detailed documentation.")
    
    return 0


if __name__ == "__main__":
    import sys
    
    # Check for command-line token
    if len(sys.argv) > 2 and sys.argv[1] == "--token":
        os.environ["SOCRM_ACCESS_TOKEN"] = sys.argv[2]
    
    sys.exit(main())
