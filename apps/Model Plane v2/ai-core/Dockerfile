FROM python:3.12-slim AS build

WORKDIR /app

# Install shared reasoning_runtime first
COPY --from=shared . /tmp/reasoning_runtime
RUN pip install --no-cache-dir /tmp/reasoning_runtime

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Compile protobuf stubs (grpc_tools bundled with grpcio-tools in requirements)
COPY proto/ ./proto/
COPY app/grpc_gen/ ./app/grpc_gen/
RUN mkdir -p app/grpc_gen && \
    python -m grpc_tools.protoc \
        -I proto \
        --python_out=app/grpc_gen \
        --pyi_out=app/grpc_gen \
        --grpc_python_out=app/grpc_gen \
        proto/ai_core.proto

FROM python:3.12-slim

WORKDIR /app
COPY --from=build /usr/local /usr/local
COPY --from=build /app/app/grpc_gen ./app/grpc_gen/
COPY app/ ./app/
COPY docker-entrypoint.sh .
RUN chmod +x docker-entrypoint.sh

ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1

RUN useradd -m appuser
USER appuser

EXPOSE 8001

HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8001/health')"

ENTRYPOINT ["./docker-entrypoint.sh"]
