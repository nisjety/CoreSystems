#!/usr/bin/env python3
"""
OAuth Login Helper - Get id_token for System User Setup

This script helps you perform OAuth login and capture the id_token,
which contains the system_user_token needed for System User authentication.

Usage:
    python get_oauth_token.py
"""

import os
import sys
import webbrowser
from urllib.parse import urlparse, parse_qs
from http.server import HTTPServer, BaseHTTPRequestHandler
import threading
import time

from dotenv import load_dotenv
load_dotenv()

# Add parent directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from auth.oauth_handler import SuperOfficeOAuthHandler
from config.settings import Settings
import jwt


# Store the callback data
callback_data = {}


class CallbackHandler(BaseHTTPRequestHandler):
    """Handle OAuth callback."""
    
    def do_GET(self):
        """Handle GET request from OAuth callback."""
        global callback_data
        
        # Parse the callback URL
        parsed = urlparse(self.path)
        params = parse_qs(parsed.query)
        
        if 'code' in params and 'state' in params:
            callback_data['code'] = params['code'][0]
            callback_data['state'] = params['state'][0]
            
            # Send success response
            self.send_response(200)
            self.send_header('Content-type', 'text/html; charset=utf-8')
            self.end_headers()
            
            html_response = """
                <html>
                <head><title>OAuth Success</title></head>
                <body style="font-family: Arial; text-align: center; padding: 50px;">
                    <h1 style="color: green;">Authentication Successful!</h1>
                    <p>You can close this window and return to the terminal.</p>
                    <p>The id_token has been captured.</p>
                </body>
                </html>
            """
            self.wfile.write(html_response.encode('utf-8'))
        else:
            # Error in callback
            self.send_response(400)
            self.send_header('Content-type', 'text/html; charset=utf-8')
            self.end_headers()
            
            html_response = """
                <html>
                <head><title>OAuth Error</title></head>
                <body style="font-family: Arial; text-align: center; padding: 50px;">
                    <h1 style="color: red;">Authentication Failed</h1>
                    <p>No code or state parameter received.</p>
                    <p>Please try again.</p>
                </body>
                </html>
            """
            self.wfile.write(html_response.encode('utf-8'))
    
    def log_message(self, format, *args):
        """Suppress server log messages."""
        pass


def start_callback_server(port=5001):
    """Start a temporary HTTP server to handle OAuth callback."""
    server = HTTPServer(('localhost', port), CallbackHandler)
    thread = threading.Thread(target=server.serve_forever)
    thread.daemon = True
    thread.start()
    return server


def print_header(text: str):
    """Print formatted header."""
    print(f"\n{'=' * 70}")
    print(f"  {text}")
    print(f"{'=' * 70}\n")


def print_success(text: str):
    """Print success message."""
    print(f"✅ {text}")


def print_error(text: str):
    """Print error message."""
    print(f"❌ {text}")


def print_info(text: str):
    """Print info message."""
    print(f"ℹ️  {text}")


def extract_system_user_claims(id_token_jwt: str):
    """Extract and display system user claims from id_token."""
    try:
        # Decode without verification (we just need to read claims)
        decoded = jwt.decode(id_token_jwt, options={"verify_signature": False})
        
        # Extract relevant claims
        system_user_token = decoded.get("http://schemes.superoffice.net/identity/system_user_token")
        context_identifier = decoded.get("http://schemes.superoffice.net/identity/ctx")
        
        print_header("System User Credentials Extracted")
        
        if system_user_token:
            print_success(f"System User Token: {system_user_token}")
        else:
            print_error("system_user_token not found in id_token!")
        
        if context_identifier:
            print_success(f"Context Identifier: {context_identifier}")
        else:
            print_error("context_identifier not found in id_token!")
        
        # Show other useful claims
        print("\n" + "-" * 70)
        print("Other Claims:")
        print("-" * 70)
        print(f"  Issuer: {decoded.get('iss', 'N/A')}")
        print(f"  Subject: {decoded.get('sub', 'N/A')}")
        print(f"  Email: {decoded.get('email', 'N/A')}")
        print(f"  Name: {decoded.get('name', 'N/A')}")
        
        if system_user_token and context_identifier:
            print_header("Add These to .env.production")
            print(f"SOCRM_SYSTEM_USER_TOKEN={system_user_token}")
            print(f"SOCRM_CONTEXT_IDENTIFIER={context_identifier}")
            print()
            return True
        
        return False
    
    except Exception as e:
        print_error(f"Failed to decode id_token: {e}")
        return False


def main():
    """Main function to get OAuth token."""
    print_header("SuperOffice OAuth - Get id_token for System User")
    
    # Load settings
    try:
        settings = Settings()
        print_success("Configuration loaded")
        print_info(f"Environment: {settings.environment}")
        print_info(f"Client ID: {settings.client_id}")
        print_info(f"Redirect URI: {settings.redirect_uri}")
    except Exception as e:
        print_error(f"Failed to load configuration: {e}")
        print_info("Make sure .env or .env.production is properly configured")
        return 1
    
    # Check redirect URI
    if 'localhost:5001' not in settings.redirect_uri and '127.0.0.1:5001' not in settings.redirect_uri:
        print("\n" + "⚠️  " * 35)
        print_error("Redirect URI must include localhost:5001 or 127.0.0.1:5001")
        print_info(f"Current redirect URI: {settings.redirect_uri}")
        print_info("Update SOCRM_REDIRECT_URI in .env to include http://localhost:5001/callback")
        print_info("Also add this URL to your SuperOffice App configuration on devnet.superoffice.com")
        print("⚠️  " * 35 + "\n")
        return 1
    
    # Initialize OAuth handler
    oauth_handler = SuperOfficeOAuthHandler(settings)
    
    # Start callback server
    print_info("Starting local callback server on port 5001...")
    server = start_callback_server(5001)
    print_success("Callback server started")
    
    # Generate authorization URL
    organization_id = "test_org"  # Temporary ID for this flow
    auth_url = oauth_handler.get_authorization_url(organization_id)
    
    print_header("Step 1: Authorize with SuperOffice")
    print_info("Opening browser for OAuth login...")
    print_info(f"URL: {auth_url}")
    print()
    print("If the browser doesn't open, copy this URL:")
    print("-" * 70)
    print(auth_url)
    print("-" * 70)
    print()
    
    # Open browser
    try:
        webbrowser.open(auth_url)
    except:
        print_error("Could not open browser automatically")
    
    print_info("Waiting for OAuth callback... (press Ctrl+C to cancel)")
    
    # Wait for callback (max 2 minutes)
    timeout = 120
    start_time = time.time()
    
    while not callback_data and (time.time() - start_time) < timeout:
        time.sleep(1)
    
    if not callback_data:
        print_error("Timeout waiting for OAuth callback")
        server.shutdown()
        return 1
    
    print_success("OAuth callback received!")
    
    # Exchange code for tokens
    print_header("Step 2: Exchange Code for Tokens")
    
    try:
        tokens = oauth_handler.handle_callback(
            callback_data['code'],
            callback_data['state']
        )
        
        print_success("Token exchange successful!")
        
        # Check for id_token
        if tokens.get('id_token'):
            print_success("id_token received!")
            print()
            
            # Extract and display system user credentials
            if extract_system_user_claims(tokens['id_token']):
                print_header("✅ Success! Next Steps")
                print("1. Copy the environment variables above")
                print("2. Add them to .env.production")
                print("3. Run: python SYSTEM_USER_EXAMPLES.py")
                print("4. Deploy: docker-compose up -d socrm_service")
                print()
            else:
                print_error("Could not extract System User credentials from id_token")
                print_info("This might mean System User flow is not enabled for your app")
                print_info("Contact SuperOffice support to enable System User for your application")
        else:
            print_error("No id_token in response!")
            print_info("This means your app might not have OpenID Connect enabled")
            print_info("Check your app configuration at devnet.superoffice.com")
            print()
            print("Available tokens:")
            for key in tokens.keys():
                if key != 'organization_id' and key != 'created_at':
                    print(f"  - {key}")
    
    except Exception as e:
        print_error(f"Token exchange failed: {e}")
        server.shutdown()
        return 1
    
    # Shutdown server
    server.shutdown()
    print()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\n\n⚠️  Cancelled by user")
        sys.exit(1)
