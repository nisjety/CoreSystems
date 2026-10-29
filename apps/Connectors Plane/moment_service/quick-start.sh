#!/bin/bash

echo "=========================================="
echo "Moment Service - Quick Start"
echo "=========================================="
echo ""

# Check if .env exists
if [ ! -f .env ]; then
    echo "❌ .env file not found!"
    echo "Creating .env from .env.example..."
    cp .env.example .env
    echo ""
    echo "⚠️  Please edit .env and set:"
    echo "   - MOMENT_API_KEY"
    echo "   - MOMENT_COMPANY_CODE"
    echo ""
    echo "Then run this script again."
    exit 1
fi

# Check if API key is set
if grep -q "your_api_key_here" .env; then
    echo "⚠️  Please configure your .env file:"
    echo "   - Set MOMENT_API_KEY"
    echo "   - Set MOMENT_COMPANY_CODE"
    echo ""
    exit 1
fi

echo "✅ Configuration found"
echo ""

# Check if running in Docker or local
read -p "Run with Docker? (y/n) [default: y]: " use_docker
use_docker=${use_docker:-y}

if [ "$use_docker" = "y" ] || [ "$use_docker" = "Y" ]; then
    echo ""
    echo "🐳 Starting with Docker Compose..."
    docker-compose up -d
    echo ""
    echo "✅ Services started!"
    echo ""
    echo "📊 Service URLs:"
    echo "   - Moment Service: http://localhost:5006"
    echo "   - Health Check:   http://localhost:5006/health"
    echo "   - API Status:     http://localhost:5006/api/status"
    echo ""
    echo "📝 View logs:"
    echo "   docker-compose logs -f moment_service"
    echo ""
    echo "🛑 Stop services:"
    echo "   docker-compose down"
else
    echo ""
    echo "🐍 Starting local Python server..."
    echo ""
    
    # Check if virtual environment exists
    if [ ! -d "venv" ]; then
        echo "Creating virtual environment..."
        python3 -m venv venv
    fi
    
    # Activate virtual environment
    source venv/bin/activate
    
    # Install dependencies
    echo "Installing dependencies..."
    pip install -q -r requirements.txt
    
    echo ""
    echo "✅ Starting Moment Service..."
    echo ""
    python app.py
fi
