# syntax=docker/dockerfile:1.7
FROM golang:1.26.5-alpine3.24 AS build

WORKDIR /src
COPY --from=studio-plane-client . /pkg/planeclient
COPY go.mod ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build go mod download

COPY . .
ARG TARGETOS TARGETARCH
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -trimpath -ldflags="-s -w" -o /out/visibility-core ./cmd/server && \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -trimpath -ldflags="-s -w" -o /out/integration-bearer-refresh ./cmd/integration-bearer-refresh

FROM alpine:3.24.1 AS runtime-base
RUN --mount=type=cache,target=/var/cache/apk apk add --no-cache ca-certificates wget && \
    addgroup -S app && adduser -S app -G app
USER app
WORKDIR /app

FROM runtime-base AS bearer-refresh
COPY --from=build /out/integration-bearer-refresh /app/integration-bearer-refresh
ENTRYPOINT ["/app/integration-bearer-refresh"]

# Keep the API as the default target for the existing Compose build.
FROM runtime-base AS api
COPY --from=build /out/visibility-core /app/visibility-core
EXPOSE 3165
ENTRYPOINT ["/app/visibility-core"]
