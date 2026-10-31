#!/usr/bin/env python3
"""Simple OAuth Token Extractor - Direct Implementation"""

import sys
import os
import webbrowser
from urllib.parse import urlparse, parse_qs
from http.server import HTTPServer, BaseHTTPRequestHandler
import threading
import time
import jwt

# Store callback data
callback_data = {}


class OAuthCallbackHandler(BaseHTTPRequestHandler):
    """Handle OAuth callback."""
    
    def do_GET(self):
        """Handle callback."""
        global callback_data
        parsed = urlparse(self.path)
        params = parse_qs(parsed.query)
        
        if 'code' in params and 'state' in params:
            callback_data['code'] = params['code'][0]
            callback_data['state'] = params['state'][0]
            self.send_response(200)
            self.send_header('Content-type', 'text/html; charset=utf-8')
            self.end_headers()
            html = "<html><body style='text-align:center;padding:50px;'><h1>Success!</h1><p>You can close this window.</p></body></html>"
            self.wfile.write(html.encode('utf-8'))
        else:
            self.send_response(400)
            self.send_header('Content-type', 'text/html; charset=utf-8')
            self.end_headers()
            html = "<html><body style='text-align:center;padding:50px;'><h1>Error!</h1><p>Invalid callback.</p></body></html>"
            self.wfile.write(html.encode('utf-8'))
    
    def log_message(self, format, *args):
        pass


def main():
    """Main function."""
    print("\n" + "=" * 70)
    print("  SuperOffice OAuth - System User Token Extraction")
    print("=" * 70 + "\n")
    
    # Load environment variables directly
    from dotenv import load_dotenv
    
    if os.path.exists('.env'):
        load_dotenv('.env')
        print("Using .env")
    elif os.path.exists('.env.production'):
        load_dotenv('.env.production')  
        print("Using .env.production")
    else:
        print("ERROR: No .env or .env.production found!")
        return 1
    
    # Get config
    client_id = os.getenv('SOCRM_CLIENT_ID')
    redirect_uri = os.getenv('SOCRM_REDIRECT_URI', 'http://localhost:5001/callback')
    tenant = os.getenv('SOCRM_TENANT', 'sod')
    
    if not client_id:
        print("ERROR: SOCRM_CLIENT_ID not configured!")
        return 1
    
    print(f"Client ID: {client_id}")
    print(f"Tenant: {tenant}")
    print(f"Redirect URI: {redirect_uri}\n")
    
    # Start callback server
    print("Starting callback server on port 5001...")
    server = HTTPServer(('localhost', 5001), OAuthCallbackHandler)
    thread = threading.Thread(target=server.serve_forever)
    thread.daemon = True
    thread.start()
    print("✓ Callback server started\n")
    
    # Build auth URL
    tenant_map = {
        'sod': 'https://sod.superoffice.com',
        'stage': 'https://stage.superoffice.com',
        'online': 'https://online.superoffice.com'
    }
    base_url = tenant_map.get(tenant, tenant_map['sod'])
    
    auth_url = (
        f"{base_url}/login/common/oauth/authorize?"
        f"client_id={client_id}&"
        f"redirect_uri={redirect_uri}&"
        f"response_type=code&"
        f"scope=openid%20email%20profile&"
        f"state=oauth_state"
    )
    
    print("Opening browser for OAuth login...")
    print(f"URL: {auth_url}\n")
    
    try:
        webbrowser.open(auth_url)
    except:
        print("Could not open browser. Copy this URL manually:")
        print(auth_url)
    
    print("Waiting for OAuth callback...")
    
    # Wait for callback
    timeout = 180
    start = time.time()
    while not callback_data and (time.time() - start) < timeout:
        time.sleep(1)
    
    if not callback_data:
        print("ERROR: Timeout waiting for callback")
        server.shutdown()
        return 1
    
    print("✓ OAuth callback received!\n")
    
    # Exchange code for tokens
    print("Exchanging code for tokens...")
    
    try:
        import requests
        
        token_url = f"{base_url}/login/common/oauth/tokens"
        client_secret = os.getenv('SOCRM_CLIENT_SECRET')
        
        resp = requests.post(token_url, data={
            'grant_type': 'authorization_code',
            'client_id': client_id,
            'client_secret': client_secret,
            'code': callback_data['code'],
            'redirect_uri': redirect_uri
        }, timeout=30)
        
        if resp.status_code != 200:
            print(f"ERROR: Token exchange failed: {resp.status_code}")
            print(resp.text)
            server.shutdown()
            return 1
        
        tokens = resp.json()
        print("✓ Token exchange successful!\n")
        
        # Extract id_token
        if 'id_token' not in tokens:
            print("ERROR: No id_token in response!")
            print("Your app might not have OpenID Connect enabled.")
            print("Check your app configuration at devnet.superoffice.com")
            server.shutdown()
            return 1
        
        print("=" * 70)
        print("  System User Credentials")
        print("=" * 70 + "\n")
        
        id_token = tokens['id_token']
        decoded = jwt.decode(id_token, options={"verify_signature": False})
        
        system_user_token = decoded.get('http://schemes.superoffice.net/identity/system_user_token')
        context_identifier = decoded.get('http://schemes.superoffice.net/identity/ctx')
        
        if system_user_token:
            print(f"System User Token: {system_user_token}\n")
        else:
            print("ERROR: system_user_token not found in id_token!\n")
        
        if context_identifier:
            print(f"Context Identifier: {context_identifier}\n")
        else:
            print("ERROR: context_identifier not found in id_token!\n")
        
        print("=" * 70)
        print("  Add These to .env.production")
        print("=" * 70 + "\n")
        print(f"SOCRM_SYSTEM_USER_TOKEN={system_user_token}")
        print(f"SOCRM_CONTEXT_IDENTIFIER={context_identifier}")
        print()
        
        if system_user_token and context_identifier:
            print("✓ Success! Copy the values above to .env.production")
            print("✓ Then run: python SYSTEM_USER_EXAMPLES.py")
            print()
            server.shutdown()
            return 0
        else:
            print("ERROR: Could not extract required claims!")
            server.shutdown()
            return 1
    
    except Exception as e:
        print(f"ERROR: {e}")
        import traceback
        traceback.print_exc()
        server.shutdown()
        return 1


if __name__ == "__main__":
    sys.exit(main())
