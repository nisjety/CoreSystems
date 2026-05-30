# ── Stage 1: build ──────────────────────────────────────────────
FROM python:3.12-slim AS build

WORKDIR /app

RUN apt-get update && \
    apt-get install -y --no-install-recommends build-essential && \
    rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir --upgrade pip && \
    pip install --no-cache-dir -r requirements.txt

# ── Stage 2: runtime ───────────────────────────────────────────
FROM python:3.12-slim

WORKDIR /app

RUN useradd -m appuser

COPY --from=build /usr/local /usr/local
COPY app ./app
COPY migrations ./migrations
COPY docker-entrypoint.sh .
RUN chmod +x docker-entrypoint.sh

USER appuser

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

EXPOSE 8004

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8004/health')"

ENTRYPOINT ["./docker-entrypoint.sh"]
