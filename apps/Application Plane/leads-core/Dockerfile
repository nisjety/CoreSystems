FROM golang:1.25-bookworm AS build

WORKDIR /src
ARG TARGETOS=linux
ARG TARGETARCH
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org

COPY go.mod go.sum* ./
RUN set -e; \
    if [ -f go.sum ]; then go mod download; else go mod download && go mod tidy; fi

COPY . .
RUN CGO_ENABLED=0 GOOS="${TARGETOS}" GOARCH="${TARGETARCH:-$(go env GOARCH)}" go build -o /out/leads-core ./cmd/server

FROM debian:bookworm-slim

WORKDIR /app
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends ca-certificates wget; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser; \
    chown -R appuser:appuser /app

COPY --from=build /out/leads-core /usr/local/bin/leads-core

USER appuser
EXPOSE 3164
CMD ["leads-core"]
