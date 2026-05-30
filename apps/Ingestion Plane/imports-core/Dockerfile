# syntax=docker/dockerfile:1.7

FROM python:3.11-slim AS builder

WORKDIR /build

# Install dependencies in a dedicated layer — only rebuilds when requirements.txt changes
COPY requirements.txt .
RUN pip install --no-cache-dir --user -r requirements.txt

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM python:3.11-slim

# Create non-root user before copying files
RUN addgroup --system appgroup && adduser --system --ingroup appgroup appuser

WORKDIR /app

# Copy Python packages from builder
COPY --from=builder /root/.local /root/.local

# Ensure the non-root app user can execute installed scripts
RUN mkdir -p /home/appuser/.local \
    && cp -a /root/.local/. /home/appuser/.local/ || true \
    && chown -R appuser:appgroup /root/.local /home/appuser/.local

# Copy application code
COPY app/ ./app/
COPY migrations/ ./migrations/

# Set environment variables
ENV HOME=/home/appuser \
    PATH=/home/appuser/.local/bin:$PATH \
    PYTHONUNBUFFERED=1 \
    PORT=3025

# Ensure app user's home directory exists and is owned
RUN mkdir -p /home/appuser && chown -R appuser:appgroup /home/appuser

# Give appuser ownership of the app directory
RUN chown -R appuser:appgroup /app

USER appuser

# Health check — use python stdlib so we don't need requests at build time
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD python3 -c "import urllib.request; urllib.request.urlopen('http://localhost:${PORT}/health')" || exit 1

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "3025"]
