# syntax=docker/dockerfile:1.7
FROM python:3.12-slim-bookworm

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    QUARRY_VISION_CV_THREADS=1

WORKDIR /app

RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends \
        ca-certificates \
        libglib2.0-0 \
        libgomp1 \
        wget; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd -r quarry --gid 65532; \
    useradd -r -g quarry --uid 65532 --home-dir /nonexistent --shell /usr/sbin/nologin quarry

COPY services/quarry-vision-sidecar/requirements.txt ./requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

COPY services/quarry-vision-sidecar/app ./app

USER quarry
EXPOSE 8091
HEALTHCHECK --interval=30s --timeout=10s --start-period=20s --retries=3 \
    CMD wget --quiet --tries=1 -O /dev/null http://localhost:8091/health || exit 1

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8091"]

