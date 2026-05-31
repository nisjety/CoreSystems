FROM golang:1.25-alpine AS builder

WORKDIR /src
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org

COPY go.mod go.sum ./
RUN set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done; \
    exit 1

COPY . .
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/finspo-api ./cmd/api

FROM alpine:3.22

RUN apk add --no-cache wget

WORKDIR /app
COPY --from=builder /out/finspo-api /usr/local/bin/finspo-api

EXPOSE 3130

CMD ["/usr/local/bin/finspo-api"]
