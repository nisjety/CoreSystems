FROM golang:1.25-alpine AS builder

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/finspo-api ./cmd/api

FROM alpine:3.22

RUN apk add --no-cache wget

WORKDIR /app
COPY --from=builder /out/finspo-api /usr/local/bin/finspo-api

EXPOSE 3130

CMD ["/usr/local/bin/finspo-api"]