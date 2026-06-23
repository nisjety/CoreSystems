# ColQwen visual reranker — PRODUCTION image (NVIDIA CUDA GPU).
# Build/run on a GPU host (Hetzner GEX44/GEX130, or Azure NV/NC once quota lands).
# NOT used for local Apple-Silicon verification — there, run app.py natively
# against MPS (see README.md), since Docker Desktop has no GPU passthrough.
FROM nvidia/cuda:12.4.1-cudnn-runtime-ubuntu22.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-pip ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# CUDA torch first, then the rest (torch is excluded from requirements.txt).
RUN pip3 install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cu124
COPY requirements.txt .
RUN pip3 install --no-cache-dir -r requirements.txt

COPY app.py .

ENV PORT=8090
EXPOSE 8090
# Model weights download on first start; mount a volume at /root/.cache/huggingface
# to persist them across restarts.
CMD ["python3", "-m", "uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8090"]
