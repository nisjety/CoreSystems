FROM golang:1.25-alpine AS builder
WORKDIR /app
COPY . .
RUN go build -o /service ./cmd/

FROM alpine:3.20
RUN apk --no-cache add ca-certificates
COPY --from=builder /service /usr/local/bin/service
ENTRYPOINT ["service"]
