#!/bin/bash

# Start script for SuperOffice CRM Service

set -e

echo "Starting SuperOffice CRM Service..."

# Extract database host from DATABASE_URL
# Wait for PostgreSQL to be ready
if [ -n "$DATABASE_URL" ]; then
    echo "Waiting for PostgreSQL..."
    # Extract host from postgresql://user:pass@host:port/db
    DB_HOST=$(echo $DATABASE_URL | sed -n 's/.*@\([^:]*\):.*/\1/p')
    DB_PORT=$(echo $DATABASE_URL | sed -n 's/.*:\([0-9]*\)\/.*/\1/p')
    echo "Database host: $DB_HOST:$DB_PORT"
    while ! pg_isready -h "$DB_HOST" -p "${DB_PORT:-5432}" -q; do
        sleep 1
    done
    echo "PostgreSQL is ready!"
fi

# Extract Redis host from REDIS_URL
# Wait for Redis to be ready
if [ -n "$REDIS_URL" ]; then
    echo "Waiting for Redis..."
    # Extract host from redis://:password@host:port or redis://host:port
    REDIS_HOST=$(echo $REDIS_URL | sed -n 's/.*@\([^:]*\):.*/\1/p')
    if [ -z "$REDIS_HOST" ]; then
        REDIS_HOST=$(echo $REDIS_URL | sed -n 's#redis://\([^:]*\):.*#\1#p')
    fi
    REDIS_PORT=$(echo $REDIS_URL | sed -n 's/.*:\([0-9]*\)$/\1/p')
    echo "Redis host: $REDIS_HOST:${REDIS_PORT:-6379}"
    while ! redis-cli -h "$REDIS_HOST" -p "${REDIS_PORT:-6379}" ping > /dev/null 2>&1; do
        sleep 1
    done
    echo "Redis is ready!"
fi

# Run database migrations if needed
# python -m alembic upgrade head

# Start the application
echo "Starting application on $SOCRM_HOST:${SOCRM_PORT:-5005}"
exec gunicorn --bind "$SOCRM_HOST:${SOCRM_PORT:-5005}" \
    --workers 4 \
    --timeout 120 \
    --access-logfile - \
    --error-logfile - \
    --log-level info \
    app:app
