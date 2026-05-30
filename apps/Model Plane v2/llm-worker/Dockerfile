FROM python:3.12-slim AS build

WORKDIR /app

# Install shared reasoning_runtime first
COPY --from=shared . /tmp/reasoning_runtime
RUN pip install --no-cache-dir /tmp/reasoning_runtime

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

FROM python:3.12-slim

WORKDIR /app
COPY --from=build /usr/local /usr/local
COPY app/ ./app/
COPY docker-entrypoint.sh .
RUN chmod +x docker-entrypoint.sh

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

RUN useradd -m appuser
USER appuser

EXPOSE 8005

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8005/health')"

ENTRYPOINT ["./docker-entrypoint.sh"]
