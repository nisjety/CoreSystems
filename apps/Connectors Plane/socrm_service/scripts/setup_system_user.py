#!/usr/bin/env python3
"""
Quick OAuth Setup - Get System User Token

This guide helps you obtain the system_user_token from OAuth login.

Two options:
1. Local OAuth (if you have localhost configured in SuperOffice app)
2. Manual extraction (if you have an id_token JWT)
"""

import sys
import os

# Add parent directory to path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

print("""
╔══════════════════════════════════════════════════════════════════════╗
║           Get System User Token - OAuth Setup                       ║
╚══════════════════════════════════════════════════════════════════════╝

You need to extract system_user_token from an OAuth id_token.
Choose your method:

┌──────────────────────────────────────────────────────────────────────┐
│ OPTION 1: Automated OAuth Flow (Recommended)                        │
└──────────────────────────────────────────────────────────────────────┘

Requirements:
  ✓ Add http://localhost:5001/callback to your SuperOffice app
  ✓ Run: python get_oauth_token.py

This will:
  1. Open SuperOffice OAuth in your browser
  2. Capture the id_token automatically
  3. Extract system_user_token and context_identifier
  4. Display the values to add to .env.production

To add localhost callback URL:
  1. Go to https://devnet.superoffice.com/
  2. Select your application
  3. Add http://localhost:5001/callback to Redirect URLs
  4. Save changes

Then run:
  $ python get_oauth_token.py


┌──────────────────────────────────────────────────────────────────────┐
│ OPTION 2: Manual Extraction (If you already have id_token)          │
└──────────────────────────────────────────────────────────────────────┘

If you already have an id_token JWT from previous OAuth login:

  $ python extract_system_user_token.py "YOUR_ID_TOKEN_JWT_HERE"

This will extract and display:
  - system_user_token
  - context_identifier


┌──────────────────────────────────────────────────────────────────────┐
│ OPTION 3: Production OAuth Flow                                     │
└──────────────────────────────────────────────────────────────────────┘

If you want to use the production callback URL:
  1. Ensure your app is accessible at: tools.aquatiq.com
  2. Trigger OAuth through your production app
  3. Capture the id_token from the response
  4. Use extract_system_user_token.py with the captured token


╔══════════════════════════════════════════════════════════════════════╗
║ Current Configuration                                                ║
╚══════════════════════════════════════════════════════════════════════╝
""")

from config.settings import Settings
from dotenv import load_dotenv

# Try to load settings
try:
    # Check which env file is being used
    if os.path.exists('.env'):
        load_dotenv('.env')
        print("📄 Using: .env")
    elif os.path.exists('.env.production'):
        load_dotenv('.env.production')
        print("📄 Using: .env.production")
    
    settings = Settings()
    print(f"🌍 Environment: {settings.environment}")
    print(f"🏢 Client ID: {settings.client_id}")
    print(f"🔗 Redirect URI: {settings.redirect_uri}")
    print()
    
    # Check if localhost:5001 is configured
    if 'localhost:5001' in settings.redirect_uri:
        print("✅ localhost:5001 is configured - you can use Option 1")
        print("   Run: python get_oauth_token.py")
    else:
        print("⚠️  localhost:5001 not configured")
        print("   To use Option 1:")
        print("   1. Add http://localhost:5001/callback to SuperOffice app")
        print("   2. Update SOCRM_REDIRECT_URI in .env to include localhost:5001")
        print()
        print("   Or use Option 2 or 3 instead")

except Exception as e:
    print(f"❌ Error loading configuration: {e}")
    print("   Make sure .env or .env.production exists and is properly configured")

print("""

╔══════════════════════════════════════════════════════════════════════╗
║ Next Steps                                                           ║
╚══════════════════════════════════════════════════════════════════════╝

Choose your option and follow the instructions above.

Once you have the tokens, add them to .env.production:

  SOCRM_SYSTEM_USER_TOKEN=your_token_here
  SOCRM_CONTEXT_IDENTIFIER=Cust12345

Then test with:
  $ python SYSTEM_USER_EXAMPLES.py

Need help? Check these files:
  📘 NEXT_STEPS.md - Detailed guide
  📗 SYSTEM_USER_SETUP_GUIDE.md - Complete setup instructions
  📕 SYSTEM_USER_READY.md - Status and architecture

""")
