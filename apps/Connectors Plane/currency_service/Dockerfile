FROM python:3.12-slim

# Set working directory
WORKDIR /app

# Install system dependencies including ODBC driver for SQL Server
# msodbcsql18 supports both amd64 and arm64 (required for Apple Silicon / multi-arch builds)
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    curl \
    gnupg2 \
    gosu \
    unixodbc \
    unixodbc-dev \
    ca-certificates \
    && curl -sSL https://packages.microsoft.com/keys/microsoft.asc | gpg --dearmor > /usr/share/keyrings/microsoft-prod.gpg \
    && echo "deb [arch=amd64,arm64 signed-by=/usr/share/keyrings/microsoft-prod.gpg] https://packages.microsoft.com/debian/12/prod bookworm main" > /etc/apt/sources.list.d/mssql-release.list \
    && apt-get update \
    && ACCEPT_EULA=Y apt-get install -y msodbcsql18 \
    && rm -rf /var/lib/apt/lists/*

# Copy requirements and install Python dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir --upgrade pip && \
    pip install --no-cache-dir -r requirements.txt

ARG DEPLOY_REVISION=local
LABEL org.opencontainers.image.revision=${DEPLOY_REVISION}

# Copy application code
COPY . .

# Create cache directory
RUN mkdir -p /app/cache/rates

# Create non-root user
RUN useradd -m -u 1000 appuser && \
    chown -R appuser:appuser /app

# Copy and configure entrypoint (runs as root to fix bind-mount permissions)
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Expose port
EXPOSE 5004

# Health check
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import os, requests; requests.get(f\"http://localhost:{os.getenv('CURRENCY_PORT', '5004')}/health\", timeout=5).raise_for_status()" || exit 1

# Entrypoint fixes bind-mount permissions then drops to appuser
ENTRYPOINT ["/entrypoint.sh"]
CMD ["python", "app.py"]
