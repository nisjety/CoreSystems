# syntax=docker/dockerfile:1.7

FROM python:3.13.12-slim-bookworm AS builder

WORKDIR /build
ENV PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_ROOT_USER_ACTION=ignore \
    PATH=/root/.local/bin:$PATH

# Install dependencies in a dedicated layer — only rebuilds when requirements.txt changes
COPY requirements.txt .
RUN --mount=type=cache,target=/root/.cache/pip \
    pip install --quiet --no-warn-script-location --user -r requirements.txt

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM python:3.13.12-slim-bookworm
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem imports-core" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

ENV PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PIP_ROOT_USER_ACTION=ignore

RUN addgroup --system appgroup \
    && adduser --system --ingroup appgroup appuser \
    && mkdir -p /home/appuser \
    && chown appuser:appgroup /home/appuser

WORKDIR /app

COPY --from=builder --chown=appuser:appgroup /root/.local /home/appuser/.local
COPY --chown=appuser:appgroup app/ ./app/
COPY --chown=appuser:appgroup migrations/ ./migrations/

# Set environment variables
ENV HOME=/home/appuser \
    PATH=/home/appuser/.local/bin:$PATH \
    PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PORT=3025

USER appuser

# Health check — use python stdlib so we don't need requests at build time
HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 CMD ["python3", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:3025/ready', timeout=5)"]

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "3025"]
