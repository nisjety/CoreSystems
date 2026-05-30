.PHONY: help proto-generate build build-grpc build-dual run run-grpc run-dual test clean tidy

# Variables
BINARY_NAME=aquatiq-user-service
BINARY_DIR=tmp
PROTO_DIR=proto
INTERNAL_DIR=internal

help: ## Display this help screen
	@grep -h -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

proto-generate: ## Generate Go code from proto files
	@echo "Generating Go code from proto files..."
	@protoc --go_out=. --go_opt=paths=source_relative \
		--go-grpc_out=. --go-grpc_opt=paths=source_relative \
		$(PROTO_DIR)/user/v1/*.proto
	@echo "Proto generation complete"

build: ## Build gRPC server binary
	@echo "Building User Service gRPC server..."
	@mkdir -p $(BINARY_DIR)
	@go build -o $(BINARY_DIR)/$(BINARY_NAME)-grpc cmd/server/main.go
	@echo "Build complete: $(BINARY_DIR)/$(BINARY_NAME)-grpc"

build-dual: ## Build dual-mode server (HTTP + gRPC)
	@echo "Building User Service dual-mode server..."
	@mkdir -p $(BINARY_DIR)
	@go build -o $(BINARY_DIR)/$(BINARY_NAME)-dual cmd/dual/main.go
	@echo "Build complete: $(BINARY_DIR)/$(BINARY_NAME)-dual"

build-all: build build-dual ## Build all server variants
	@echo "All builds complete"

run: build ## Build and run gRPC server
	@echo "Starting User Service gRPC server..."
	@./$(BINARY_DIR)/$(BINARY_NAME)-grpc

run-grpc: run ## Alias for run

run-dual: build-dual ## Build and run dual-mode server
	@echo "Starting User Service dual-mode server..."
	@./$(BINARY_DIR)/$(BINARY_NAME)-dual

test: ## Run tests
	@echo "Running tests..."
	@go test -v ./...

test-coverage: ## Run tests with coverage
	@echo "Running tests with coverage..."
	@go test -v -coverprofile=coverage.out ./...
	@go tool cover -html=coverage.out -o coverage.html
	@echo "Coverage report: coverage.html"

tidy: ## Tidy go.mod
	@echo "Tidying go.mod..."
	@go mod tidy

clean: ## Clean build artifacts
	@echo "Cleaning build artifacts..."
	@rm -rf $(BINARY_DIR)
	@rm -f coverage.out coverage.html
	@echo "Clean complete"

deps: ## Download dependencies
	@echo "Downloading dependencies..."
	@go mod download
	@echo "Dependencies downloaded"

fmt: ## Format code
	@echo "Formatting code..."
	@go fmt ./...
	@echo "Format complete"

lint: ## Run linters
	@echo "Running linters..."
	@golangci-lint run ./...

dev: ## Run with hot reload (requires air)
	@echo "Starting development server with hot reload..."
	@air

install-tools: ## Install development tools
	@echo "Installing development tools..."
	@go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
	@go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest
	@go install github.com/air-verse/air@latest
	@echo "Tools installed"

migrate-up: ## Run database migrations up
	@echo "Running migrations up..."
	@psql $(DATABASE_URL) -f migrations/001_init.up.sql
	@echo "Migrations complete"

migrate-down: ## Run database migrations down
	@echo "Running migrations down..."
	@psql $(DATABASE_URL) -f migrations/001_init.down.sql
	@echo "Migrations rolled back"

docker-build: ## Build Docker image
	@echo "Building Docker image..."
	@docker build -t aquatiq-user-service:latest .
	@echo "Docker image built"

docker-run: ## Run Docker container
	@echo "Running Docker container..."
	@docker run -p 50052:50052 --env-file .env aquatiq-user-service:latest
