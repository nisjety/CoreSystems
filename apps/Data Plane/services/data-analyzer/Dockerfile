FROM python:3.12-slim

WORKDIR /app

RUN adduser --disabled-password --gecos "" analyzer

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY app/ ./app/

USER analyzer

EXPOSE 9103

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "9103"]
