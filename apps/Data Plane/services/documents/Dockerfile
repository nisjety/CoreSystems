FROM python:3.11-slim

WORKDIR /app

# System deps for asyncpg + grpcio
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc libpq-dev curl \
    && rm -rf /var/lib/apt/lists/*

COPY services/documents/requirements.txt ./requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

COPY services/documents/app ./app
COPY services/shared ./shared
COPY proto ./proto

EXPOSE 8001 50051

CMD ["python", "-m", "app.main"]
