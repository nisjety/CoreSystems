# syntax=docker/dockerfile:1.7
# ColQwen visual reranker — PRODUCTION image (NVIDIA CUDA GPU).
# Build/run on a GPU host (Hetzner GEX44/GEX130, or Azure NV/NC once quota lands).
# NOT used for local Apple-Silicon verification — there, run app.py natively
# against MPS (see README.md), since Docker Desktop has no GPU passthrough.
FROM nvidia/cuda:12.4.1-cudnn-runtime-ubuntu22.04

ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE \
      org.opencontainers.image.source="CoreSystem/apps/Data Plane v2/services/colqwen-reranker"

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# CUDA torch first, then the rest (torch is excluded from requirements.txt).
RUN pip3 install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cu124
COPY requirements.txt .
RUN pip3 install --no-cache-dir -r requirements.txt
# bitsandbytes lives here rather than in requirements.txt because it is CUDA-only
# (no MPS/CPU backend) and requirements.txt is also installed on the Apple-Silicon
# verification path, where it would be dead weight at best. It backs
# COLQWEN_QUANT=nf4|int8, which is what lets the model fit in real VRAM on a card
# smaller than ~8 GB — see the COLQWEN_QUANT comment in app.py for the measured
# numbers behind that.
RUN pip3 install --no-cache-dir bitsandbytes==0.50.1

COPY app.py .

RUN groupadd --system --gid 10001 dataplane \
    && useradd --system --uid 10001 --gid dataplane --create-home --shell /usr/sbin/nologin dataplane \
    && mkdir -p /cache/huggingface \
    && chown -R dataplane:dataplane /app /cache

ENV PORT=8090 \
    HF_HOME=/cache/huggingface \
    TRANSFORMERS_CACHE=/cache/huggingface
USER dataplane
EXPOSE 8090
# Model weights download on first start; mount a writable volume at
# /cache/huggingface to persist them without granting root access.
HEALTHCHECK --interval=30s --timeout=5s --start-period=120s --retries=5 \
    CMD ["python3", "-c", "import socket; s=socket.create_connection(('127.0.0.1',8090),3); s.close()"]
CMD ["python3", "-m", "uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8090"]
