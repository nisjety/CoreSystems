# syntax=docker/dockerfile:1.7
# media-embedder — dense audio (LAION-CLAP) and video (SigLIP 2 over sampled
# frames) embeddings.
#
# CPU base by default, and now for BOTH arms: CLAP was chosen to be CPU-viable,
# and the video tower became CPU-viable when LanguageBind was replaced by SigLIP
# 2 frame sampling (see app.py's module docstring). Build with --build-arg
# BASE=nvidia/cuda:12.4.1-cudnn-runtime-ubuntu22.04 for GPU throughput; nothing
# requires it any more.
#
# There is no VIDEO_EXTRAS build arg. It existed to install decord, av,
# pytorchvideo, opencv, timm, ftfy and languagebind — ~2 GB of wheels, several
# unmaintained, needed only to make LanguageBind import. The frame-sampling
# tower needs ffmpeg (already here for audio) plus pillow, so the video arm is
# now a runtime flag only.
ARG BASE=python:3.11-slim-bookworm
FROM ${BASE}

ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE \
      org.opencontainers.image.source="CoreSystem/apps/Data Plane v2/services/media-embedder"

# ffmpeg serves BOTH arms: audio decode to the 48 kHz mono PCM CLAP expects, and
# video frame sampling (ffprobe for duration, ffmpeg for the frames). ffprobe
# ships in the same package. python3/pip are only installed when the CUDA base
# is used — the python base already has them.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
    && (command -v python3 >/dev/null || apt-get install -y --no-install-recommends python3 python3-pip) \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# CPU torch keeps the default image ~1 GB instead of ~5 GB. The CUDA base
# overrides this via TORCH_INDEX.
ARG TORCH_INDEX=https://download.pytorch.org/whl/cpu
RUN python3 -m pip install --no-cache-dir --break-system-packages torch --index-url ${TORCH_INDEX}

COPY requirements.txt .
RUN python3 -m pip install --no-cache-dir --break-system-packages -r requirements.txt

COPY app.py .

RUN groupadd --system --gid 10002 dataplane \
    && useradd --system --uid 10002 --gid dataplane --create-home --shell /usr/sbin/nologin dataplane \
    && mkdir -p /cache/huggingface \
    && chown -R dataplane:dataplane /app /cache

ENV PORT=8095 \
    HF_HOME=/cache/huggingface
USER dataplane
EXPOSE 8095
# Model weights download on first start; mount a writable volume at
# /cache/huggingface to persist them without granting root access.
HEALTHCHECK --interval=30s --timeout=5s --start-period=180s --retries=5 \
    CMD ["python3", "-c", "import urllib.request;urllib.request.urlopen('http://127.0.0.1:8095/healthz',timeout=5)"]
CMD ["python3", "-m", "uvicorn", "app:app", "--host", "0.0.0.0", "--port", "8095"]
