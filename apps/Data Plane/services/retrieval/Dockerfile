FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends gcc libpq-dev curl \
    && rm -rf /var/lib/apt/lists/*

COPY services/retrieval/requirements.txt ./requirements.txt
RUN pip install --no-cache-dir -r requirements.txt

COPY services/retrieval/app ./app
COPY services/shared ./shared
COPY proto ./proto

EXPOSE 8004 50052

CMD ["python", "-m", "app.main"]
