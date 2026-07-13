# Data Plane v2 schema migrator — one-shot deploy-time migration runner.
#
# Built and run as a compose service (`migrate`) that executes `migrator up`
# against the dpv2 Postgres BEFORE the data services start serving. Idempotent
# (the migrator tracks applied versions in schema_migrations and skips
# already-applied ones) and run-once per `up` (the service exits after applying
# pending migrations; compose dependents gate on
# `service_completed_successfully`).
#
# Build context is this directory (apps/Data Plane v2/tools/migrator) so it is
# unaffected by the project-root .dockerignore the Rust services use. The
# migrations are NOT baked in — the compose service bind-mounts
# infra/postgres/migrations read-only at /app/infra/postgres/migrations, so
# adding a migration needs no image rebuild.
# syntax=docker/dockerfile:1.7
FROM golang:1.26.5-alpine3.24 AS build
WORKDIR /src
# Module cache layer: copy manifests first so dependency download is cached
# independently of source changes.
COPY go.mod go.sum ./
RUN go mod download
COPY main.go ./
RUN CGO_ENABLED=0 go build -trimpath -o /out/migrator ./...

FROM alpine:3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
ENV SOURCE_REVISION=$SOURCE_REVISION BUILD_DATE=$BUILD_DATE
RUN apk add --no-cache ca-certificates
RUN addgroup -S -g 10001 dataplane && adduser -S -D -H -u 10001 -G dataplane dataplane
COPY --from=build /out/migrator /usr/local/bin/migrator
# The migrator resolves its migrations dir as ./infra/postgres/migrations
# relative to CWD; WORKDIR=/app makes the bind-mounted tree the default.
WORKDIR /app
USER dataplane
# Apply all pending forward migrations, then exit 0. A non-zero exit fails the
# `up` (compose dependents won't start), surfacing a bad migration at deploy time.
ENTRYPOINT ["migrator"]
CMD ["up"]
