# ---- Build stage ----
FROM python:3.12-slim AS build

WORKDIR /app

# Install shared reasoning_runtime first
COPY --from=shared . /tmp/reasoning_runtime
RUN pip install --no-cache-dir /tmp/reasoning_runtime

COPY requirements.txt .
RUN pip install --no-cache-dir --upgrade pip \
    && pip install --no-cache-dir -r requirements.txt

# ---- Runtime stage ----
FROM python:3.12-slim

WORKDIR /app

# Copy installed packages from build stage
COPY --from=build /usr/local /usr/local

# Copy application code
COPY app/ ./app/
COPY migrations/ ./migrations/
COPY tests/ ./tests/
COPY docker-entrypoint.sh .

RUN chmod +x docker-entrypoint.sh \
    && useradd -m appuser

USER appuser

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

EXPOSE 8002

HEALTHCHECK --interval=30s --timeout=5s --retries=3 --start-period=15s \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8002/health')"

ENTRYPOINT ["./docker-entrypoint.sh"]
