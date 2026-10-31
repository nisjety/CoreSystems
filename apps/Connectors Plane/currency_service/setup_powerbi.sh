#!/bin/bash

# Power BI SQL Server Setup Script for Currency Service
# This script helps set up the SQL Server database for Power BI integration

set -e

echo "====================================="
echo "Power BI SQL Server Setup"
echo "====================================="
echo ""

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${YELLOW}ℹ $1${NC}"
}

# Check if running with Docker or existing SQL Server
echo "Select SQL Server setup option:"
echo "1) Use Docker (Recommended for development)"
echo "2) Use existing SQL Server"
read -p "Enter choice [1-2]: " SETUP_CHOICE

if [ "$SETUP_CHOICE" == "1" ]; then
    echo ""
    print_info "Setting up SQL Server in Docker..."
    
    # Check if Docker is installed
    if ! command -v docker &> /dev/null; then
        print_error "Docker is not installed. Please install Docker first."
        exit 1
    fi
    
    print_success "Docker found"
    
    # Get SQL Server password
    read -sp "Enter SA password (min 8 chars, uppercase, lowercase, digits, and special chars): " SA_PASSWORD
    echo ""
    
    # Validate password strength
    if [ ${#SA_PASSWORD} -lt 8 ]; then
        print_error "Password must be at least 8 characters long"
        exit 1
    fi
    
    # Stop and remove existing container if it exists
    if docker ps -a | grep -q sql_powerbi; then
        print_info "Removing existing sql_powerbi container..."
        docker stop sql_powerbi 2>/dev/null || true
        docker rm sql_powerbi 2>/dev/null || true
    fi
    
    # Start SQL Server container
    print_info "Starting SQL Server container..."
    docker run -e "ACCEPT_EULA=Y" -e "SA_PASSWORD=${SA_PASSWORD}" \
        -p 1433:1433 --name sql_powerbi \
        --restart unless-stopped \
        -d mcr.microsoft.com/mssql/server:2022-latest
    
    print_success "SQL Server container started"
    
    # Wait for SQL Server to be ready
    print_info "Waiting for SQL Server to be ready (this may take 30-60 seconds)..."
    sleep 30
    
    SQL_SERVER="localhost"
    SQL_USERNAME="sa"
    SQL_PASSWORD="$SA_PASSWORD"
    
else
    echo ""
    print_info "Using existing SQL Server..."
    
    read -p "SQL Server host (e.g., localhost or your-server.database.windows.net): " SQL_SERVER
    read -p "SQL Server username: " SQL_USERNAME
    read -sp "SQL Server password: " SQL_PASSWORD
    echo ""
fi

SQL_DATABASE="CurrencyPowerBI"

# Test connection using Python
print_info "Testing SQL Server connection..."

python3 << EOF
import sys
try:
    import pyodbc
    print("pyodbc installed")
except ImportError:
    print("ERROR: pyodbc not installed. Run: pip install pyodbc")
    sys.exit(1)

try:
    # Try to connect
    conn_str = "DRIVER={ODBC Driver 17 for SQL Server};SERVER=${SQL_SERVER};UID=${SQL_USERNAME};PWD=${SQL_PASSWORD}"
    conn = pyodbc.connect(conn_str, timeout=10)
    print("Connection successful")
    conn.close()
except Exception as e:
    print(f"ERROR: Failed to connect to SQL Server: {e}")
    sys.exit(1)
EOF

if [ $? -eq 0 ]; then
    print_success "SQL Server connection successful"
else
    print_error "Failed to connect to SQL Server"
    exit 1
fi

# Create database
print_info "Creating database: $SQL_DATABASE..."

python3 << EOF
import pyodbc
try:
    conn_str = "DRIVER={ODBC Driver 17 for SQL Server};SERVER=${SQL_SERVER};UID=${SQL_USERNAME};PWD=${SQL_PASSWORD}"
    conn = pyodbc.connect(conn_str, autocommit=True)
    cursor = conn.cursor()
    
    # Check if database exists
    cursor.execute("SELECT name FROM sys.databases WHERE name = ?", SQL_DATABASE)
    if cursor.fetchone():
        print("Database already exists")
    else:
        cursor.execute(f"CREATE DATABASE [{SQL_DATABASE}]")
        print("Database created")
    
    conn.close()
    print("SUCCESS")
except Exception as e:
    print(f"ERROR: {e}")
    import sys
    sys.exit(1)
EOF

if [ $? -eq 0 ]; then
    print_success "Database ready"
else
    print_error "Failed to create database"
    exit 1
fi

# Update .env file
print_info "Updating .env file..."

ENV_FILE=".env"

# Backup existing .env if it exists
if [ -f "$ENV_FILE" ]; then
    cp "$ENV_FILE" "${ENV_FILE}.backup.$(date +%Y%m%d_%H%M%S)"
    print_success "Backed up existing .env file"
fi

# Create or update .env file
cat >> "$ENV_FILE" << EOF

# Power BI SQL Server Configuration (Added by setup script)
POWERBI_SQL_SERVER=${SQL_SERVER}
POWERBI_SQL_DATABASE=${SQL_DATABASE}
POWERBI_SQL_USERNAME=${SQL_USERNAME}
POWERBI_SQL_PASSWORD=${SQL_PASSWORD}
POWERBI_SQL_DRIVER=ODBC Driver 17 for SQL Server
EOF

print_success ".env file updated"

# Run the Currency Service to initialize tables
print_info "Initializing database tables..."

python3 << EOF
import sys
import os

# Set environment variables
os.environ['POWERBI_SQL_SERVER'] = '${SQL_SERVER}'
os.environ['POWERBI_SQL_DATABASE'] = '${SQL_DATABASE}'
os.environ['POWERBI_SQL_USERNAME'] = '${SQL_USERNAME}'
os.environ['POWERBI_SQL_PASSWORD'] = '${SQL_PASSWORD}'
os.environ['POWERBI_SQL_DRIVER'] = 'ODBC Driver 17 for SQL Server'

try:
    from powerbi_models import init_powerbi_db
    SessionLocal, engine = init_powerbi_db()
    print("Tables and views created successfully")
except Exception as e:
    print(f"ERROR: {e}")
    sys.exit(1)
EOF

if [ $? -eq 0 ]; then
    print_success "Database tables and views initialized"
else
    print_error "Failed to initialize database"
    exit 1
fi

# Test the API endpoints (if service is running)
print_info "Checking if Currency Service is running..."

if curl -s -f http://localhost:5004/health > /dev/null 2>&1; then
    print_success "Currency Service is running"
    
    print_info "Testing Power BI endpoints..."
    
    # Test status endpoint
    if curl -s -f http://localhost:5004/api/currency/powerbi/status > /dev/null 2>&1; then
        print_success "Power BI status endpoint working"
    else
        print_error "Power BI status endpoint not responding"
    fi
else
    print_info "Currency Service is not running. Start it with: python app.py"
fi

echo ""
echo "====================================="
echo "Setup Complete!"
echo "====================================="
echo ""
echo "SQL Server Details:"
echo "  Server: ${SQL_SERVER}"
echo "  Database: ${SQL_DATABASE}"
echo "  Username: ${SQL_USERNAME}"
echo ""
echo "Available Views for Power BI:"
echo "  - dbo.DataSammendrag (Data Summary)"
echo "  - dbo.Valutakurser (Currency Rates)"
echo "  - dbo.SisteKurser (Latest Rates)"
echo "  - dbo.HistoricalRate (Historical Data)"
echo ""
echo "Next Steps:"
echo "  1. Start the Currency Service: python app.py"
echo "  2. Trigger an initial sync: curl -X POST http://localhost:5004/api/currency/powerbi/sync"
echo "  3. Connect Power BI Desktop to: ${SQL_SERVER}"
echo "  4. Read the documentation: POWERBI_INTEGRATION.md"
echo ""
print_success "Setup completed successfully!"
