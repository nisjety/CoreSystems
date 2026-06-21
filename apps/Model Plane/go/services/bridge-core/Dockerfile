# syntax=docker/dockerfile:1.7
FROM golang:1.25-alpine AS builder
WORKDIR /app
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build go mod download
COPY . .
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build go build -o /service ./cmd/

FROM alpine:3.20
RUN --mount=type=cache,target=/var/cache/apk apk add ca-certificates curl
COPY --from=builder /service /usr/local/bin/service
ENTRYPOINT ["service"]
