.PHONY: proto proto-clean install-tools help

# Proto compilation
proto: proto-clean
	@echo "Generating gRPC code from proto files..."
	protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		api/proto/health/v1/health.proto
	protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		api/proto/docker/v1/docker.proto
	protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		api/proto/whitelist/v1/whitelist.proto
	protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		api/proto/database/v1/database.proto
	@echo "✅ gRPC code generation complete"

# Clean generated proto files
proto-clean:
	@echo "Cleaning generated gRPC code..."
	@rm -f api/proto/health/v1/*.pb.go
	@rm -f api/proto/docker/v1/*.pb.go
	@rm -f api/proto/whitelist/v1/*.pb.go
	@rm -f api/proto/database/v1/*.pb.go
	@echo "✅ Clean complete"

# Install required tools
install-tools:
	@echo "Installing protobuf compiler tools..."
	go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
	go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
	@echo "✅ Tools installed"

# Build the gateway
build:
	@echo "Building integration gateway..."
	go build -o bin/gateway ./cmd/gateway
	@echo "✅ Build complete: bin/gateway"

# Run the gateway
run: build
	@echo "Starting integration gateway..."
	./bin/gateway

# Run tests
test:
	@echo "Running tests..."
	go test -v -race -coverprofile=coverage.out ./...
	@echo "✅ Tests complete"

# Format code
fmt:
	@echo "Formatting code..."
	go fmt ./...
	@echo "✅ Format complete"

# Lint code
lint:
	@echo "Linting code..."
	golangci-lint run
	@echo "✅ Lint complete"

# Help
help:
	@echo "Available targets:"
	@echo "  proto          - Generate gRPC code from proto files"
	@echo "  proto-clean    - Remove generated gRPC code"
	@echo "  install-tools  - Install protoc plugins"
	@echo "  build          - Build the gateway binary"
	@echo "  run            - Build and run the gateway"
	@echo "  test           - Run all tests"
	@echo "  fmt            - Format Go code"
	@echo "  lint           - Run linter"
	@echo "  help           - Show this help message"
