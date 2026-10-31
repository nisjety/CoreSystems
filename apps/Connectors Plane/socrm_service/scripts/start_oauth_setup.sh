#!/bin/bash
# Quick Setup - System User Token Extraction
#
# This script sets up a virtual environment and runs the OAuth flow

set -e

echo "╔══════════════════════════════════════════════════════════════════════╗"
echo "║     System User Token Setup - Quick Start                           ║"
echo "╚══════════════════════════════════════════════════════════════════════╝"
echo ""

# Check if venv exists
if [ ! -d "venv" ]; then
    echo "📦 Creating Python virtual environment..."
    python3 -m venv venv
    echo "✅ Virtual environment created"
else
    echo "✅ Virtual environment already exists"
fi

# Activate venv
echo "🔧 Activating virtual environment..."
source venv/bin/activate

# Install dependencies
echo "📥 Installing dependencies..."
pip install -q python-dotenv pyjwt requests

echo "✅ Dependencies installed"
echo ""

echo "╔══════════════════════════════════════════════════════════════════════╗"
echo "║ Choose Your Option:                                                 ║"
echo "╚══════════════════════════════════════════════════════════════════════╝"
echo ""
echo "1️⃣  AUTOMATED: Run automated OAuth flow (localhost:5001)"
echo "   Requirements:"
echo "   - Add http://localhost:5001/callback to SuperOffice app"
echo "   - Run: python get_oauth_token.py"
echo ""
echo "2️⃣  MANUAL: Extract from existing id_token JWT"
echo "   If you have an id_token:"
echo "   - Run: python extract_system_user_token.py \"YOUR_ID_TOKEN\""
echo ""
echo "3️⃣  PRODUCTION: Use your production OAuth flow"
echo "   - Trigger OAuth at tools.aquatiq.com"
echo "   - Capture id_token from response"
echo "   - Extract using option 2"
echo ""

# Run the setup wizard
echo "Running setup wizard..."
echo ""
python setup_system_user.py

echo ""
echo "════════════════════════════════════════════════════════════════════════"
echo ""
echo "💡 To run OAuth flow now: python get_oauth_token.py"
echo "💡 To extract from id_token: python extract_system_user_token.py \"JWT\""
echo ""
echo "Don't forget to deactivate venv when done: deactivate"
echo ""
