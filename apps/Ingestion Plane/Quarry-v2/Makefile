.PHONY: bootstrap build build-rust build-go test test-rust test-go test-vision bench-browser fmt lint dev down clean tidy

bootstrap:
	@command -v cargo >/dev/null || { echo "install rust toolchain: https://rustup.rs"; exit 1; }
	@command -v go >/dev/null || { echo "install go: https://go.dev/dl/"; exit 1; }
	$(MAKE) tidy

tidy:
	cd pkg/quarrycontracts && go mod tidy
	cd services/quarry-control && go mod tidy
	cd services/quarry-orchestrator && go mod tidy

build: build-rust build-go

build-rust:
	cargo build --workspace

build-go:
	cd services/quarry-control && go build ./...
	cd services/quarry-orchestrator && go build ./...

test: test-rust test-go

test-rust:
	cargo test --workspace

test-go:
	cd services/quarry-control && go test ./...
	cd services/quarry-orchestrator && go test ./...

test-vision:
	cd services/quarry-vision-sidecar && python -m pytest

bench-browser:
	cargo run -p quarry-evals --bin quarry-browser-benchmark -- --frames 30 --actions 10 --out lab/evals/browser_benchmark.json

fmt:
	cargo fmt
	cd services/quarry-control && gofmt -w .
	cd services/quarry-orchestrator && gofmt -w .

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd services/quarry-control && go vet ./...
	cd services/quarry-orchestrator && go vet ./...

dev:
	docker compose -f deploy/compose/docker-compose.yml up --build

down:
	docker compose -f deploy/compose/docker-compose.yml down -v

clean:
	cargo clean
	rm -rf services/*/bin
