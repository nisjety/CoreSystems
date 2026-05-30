FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends gcc libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY services/knowledge-index/requirements.txt ./requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

COPY services/knowledge-index/worker ./worker
COPY services/shared ./shared
COPY proto ./proto

CMD ["python", "-m", "worker.main"]
