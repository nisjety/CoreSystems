FROM golang:1.25-bookworm AS build

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/notification-core ./cmd/server

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 appuser \
    && chown -R appuser:appuser /app

COPY --from=build /out/notification-core /usr/local/bin/notification-core
COPY migrations /app/migrations

USER appuser

EXPOSE 3140

CMD ["notification-core"]
