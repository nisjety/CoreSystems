# syntax=docker/dockerfile:1.7
FROM golang:1.25.12-alpine3.24 AS builder
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/nats-provisioner
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    GOWORK=off go mod download && \
    GOWORK=off CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /nats-provisioner .

FROM alpine:3.24
RUN addgroup -S -g 10001 modelplane && adduser -S -D -H -u 10001 -G modelplane modelplane
COPY --from=builder /nats-provisioner /usr/local/bin/nats-provisioner
USER modelplane
ENTRYPOINT ["/usr/local/bin/nats-provisioner"]
