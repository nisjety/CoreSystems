FROM golang:1.25-alpine AS build

WORKDIR /src
COPY go.mod go.sum* ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/integration-corev2 ./cmd/api && \
    CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/finspo-worker ./cmd/finspo-worker

FROM alpine:3.21
RUN addgroup -S app && adduser -S app -G app
USER app
WORKDIR /app
COPY --from=build /out/integration-corev2 /app/integration-corev2
COPY --from=build /out/finspo-worker /app/finspo-worker
EXPOSE 3026
ENTRYPOINT ["/app/integration-corev2"]
