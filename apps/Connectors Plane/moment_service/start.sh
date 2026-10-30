#!/bin/bash
set -e

echo "Starting Moment Integration Service..."
echo "Company: ${MOMENT_COMPANY_CODE}"
echo "Environment: ${MOMENT_ENVIRONMENT}"

# Run the Flask application
exec python app.py
