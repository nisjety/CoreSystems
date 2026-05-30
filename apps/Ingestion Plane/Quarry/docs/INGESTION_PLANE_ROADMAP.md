# Ingestion Plane Implementation Roadmap

**Project:** Quarry Microservices Architecture - Ingestion Plane  
**Architecture:** 100% Service Isolation (No Shared Libraries)  
**Status:** Planning  
**Estimated Duration:** 8-12 weeks  
**Last Updated:** February 19, 2026

---

## Executive Summary

Transform Quarry from a monolithic structure into a microservices architecture with 4 specialized ingestion services:
- **query-service** (Port 3040) - API gateway, request routing
- **import-service** (Port 3041) - File parsing, bulk imports
- **integration-service** (Port 3042) - External API connectors
- **crawler-service** (Port 3043) - Web crawling (existing functionality)

**Key Benefits:**
- **100% Service Isolation:** No shared libraries - clear boundaries, independent evolution
- Independent scaling (crawler can scale separately from API)
- Technology diversity (Go for crawler, Python for ML-heavy import processing)
- Fault isolation (integration failures don't crash crawler)
- Team autonomy (different teams can own services)

---

## Phase 0: Foundation & Planning (Week 0-1)

### Objectives
- Define service contracts via proto files
- Set up infrastructure (NATS, monitoring)
- Establish service boundaries (no shared code)

### Design Principle: 100% Service Isolation
**Each service:**
- Has its own `go.mod` (separate module)
- Implements its own auth, storage, queue packages internally
- Communicates only via gRPC or NATS messages
- Can be deployed, scaled, and versioned independently
- Has clear ownership boundaries

**No shared libraries** - Code can be copy/pasted between services but evolves independently.

### Tasks

#### 0.1 Service Boundaries Definition
**Each service will maintain its own internal packages:**

```
services/crawler/
├── internal/
│   ├── auth/         # API key validation (crawler-specific)
│   ├── storage/      # Postgres/Redis clients (crawler-specific)
│   ├── queue/        # NATS/Temporal wrappers (crawler-specific)
│   └── telemetry/    # Metrics/logging (crawler-specific)

services/query/
├── internal/
│   ├── auth/         # JWT validation (query-specific)
│   ├── storage/      # Request logging
│   └── router/       # Service routing logic

services/import/
├── internal/
│   ├── parsers/      # PDF, DOCX, CSV parsers
│   ├── chunking/     # Text chunking strategies
│   └── storage/      # Import-specific database access

services/integration/
├── internal/
│   ├── oauth/        # OAuth 2.0 flows
│   ├── connectors/   # Notion, Google Drive clients
│   └── storage/      # Token storage
```

**Rationale:** True microservices independence - no dependency version conflicts, clear ownership.

**Code Reuse Strategy:**
- **Initial Development:** Copy common code (auth, storage helpers) from Quarry monolith into each service's `internal/` directory
- **Ongoing Evolution:** Each service evolves its copy independently based on its needs
  - Example: crawler-service might optimize Postgres queries for read-heavy workloads
  - Example: import-service might add batch insert optimizations
- **Consistency:** Use linters, code templates, and code reviews to maintain similar patterns (not identical code)
- **Bug Fixes:** Critical security fixes are applied to all services (announced via Slack/Jira)
- **Tradeoff Accepted:** Some duplication is acceptable for the benefits of service autonomy

#### 0.2 Define gRPC Service Contracts
**Create:** `proto/` directory with service definitions

```protobuf
// proto/crawler/v1/crawler.proto
syntax = "proto3";
package crawler.v1;

service CrawlerService {
  rpc StartCrawl(CrawlRequest) returns (CrawlResponse);
  rpc GetCrawlStatus(GetCrawlStatusRequest) returns (CrawlStatus);
  rpc CancelCrawl(CancelCrawlRequest) returns (CancelCrawlResponse);
  rpc ListCrawls(ListCrawlsRequest) returns (ListCrawlsResponse);
}

message CrawlRequest {
  string url = 1;
  repeated string formats = 2;
  int32 max_depth = 3;
  string collection = 4;
  // ... other fields from internal/models/request.go
}

// proto/importer/v1/importer.proto
service ImportService {
  rpc ImportFile(ImportFileRequest) returns (ImportFileResponse);
  rpc ImportBulk(stream BulkImportRequest) returns (BulkImportResponse);
  rpc GetImportStatus(GetImportStatusRequest) returns (ImportStatus);
}

// proto/integration/v1/integration.proto
service IntegrationService {
  rpc ConnectNotion(NotionConnectRequest) returns (ConnectResponse);
  rpc SyncGoogleDrive(GoogleDriveSyncRequest) returns (SyncResponse);
  rpc ListConnections(ListConnectionsRequest) returns (ListConnectionsResponse);
}

// proto/query/v1/query.proto (Internal service mesh only)
service QueryService {
  rpc DispatchCrawl(DispatchCrawlRequest) returns (DispatchCrawlResponse);
  rpc DispatchImport(DispatchImportRequest) returns (DispatchImportResponse);
}
```

**Note on Proto Files:**
- Proto definitions (`proto/`) **are the only shared artifact**
- They define service **interfaces** (contracts), not implementation
- Each service imports generated proto code (e.g., `import pb "quarry/proto/crawler/v1"`)
- This is necessary for gRPC communication
- **No Go code** is shared - only interface definitions

#### 0.3 Update Docker Compose Foundation
**File:** `docker-compose.yml`

Add NATS for service-to-service messaging:
```yaml
services:
  nats:
    image: nats:2.10-alpine
    ports:
      - "4222:4222"   # Client connections
      - "8222:8222"   # HTTP monitoring
    command: [
      "--jetstream",
      "--store_dir=/data",
      "--max_memory_store=1GB",
      "--max_file_store=10GB"
    ]
    volumes:
      - nats-data:/data
    healthcheck:
      test: ["CMD", "wget", "--spider", "-q", "http://localhost:8222/healthz"]
      interval: 10s
      timeout: 5s
      retries: 3

volumes:
  nats-data:
```

#### 0.4 Development Tooling
```bash
# Install protobuf compiler
brew install protobuf         # macOS
# or apt-get install protobuf-compiler  # Ubuntu

# Install Go plugins
go install google.golang.org/protobuf/cmd/protoc-gen-go@latest
go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest

# Install grpcurl (like curl for gRPC)
brew install grpcurl

# Create Makefile for proto generation
```

**Create:** `Makefile`
```makefile
.PHONY: proto
proto:
	protoc --go_out=. --go_opt=paths=source_relative \
	       --go-grpc_out=. --go-grpc_opt=paths=source_relative \
	       proto/**/*.proto

.PHONY: build-services
build-services:
	cd services/crawler && go build -o ../../bin/crawler ./cmd/server
	cd services/query && go build -o ../../bin/query ./cmd/server
	cd services/import && go build -o ../../bin/import ./cmd/server
	cd services/integration && go build -o ../../bin/integration ./cmd/server

.PHONY: test-integration
test-integration:
	go test -v -tags=integration ./tests/integration/...
```

### Deliverables (Week 1)
- [ ] Proto definitions for all 4 services
- [ ] NATS JetStream running in Docker
- [ ] Makefile for proto code generation
- [ ] ADR (Architecture Decision Record) documenting isolation principle
- [ ] Service template directory structure (internal packages blueprint)

### Risks & Mitigations
- **Risk:** Proto schema changes break clients
- **Mitigation:** Use buf.build for schema linting, backwards compatibility checks

---

## Phase 1: Extract Crawler Service (Week 2-3)

### Objectives
- Move crawler functionality into independent service
- Maintain API compatibility
- Zero downtime migration

### Tasks

#### 1.1 Create Service Structure
```bash
mkdir -p services/crawler/{cmd/server,internal/{api,handler,worker},pkg}
```

**Directory structure:**
```
services/crawler/
├── cmd/
│   └── server/
│       └── main.go              # Service entrypoint
├── internal/
│   ├── api/
│   │   ├── grpc/
│   │   │   └── server.go        # gRPC server implementation
│   │   └── http/
│   │       └── server.go        # REST API (optional, for health checks)
│   ├── handler/
│   │   ├── crawl.go             # StartCrawl handler
│   │   ├── status.go            # GetCrawlStatus handler
│   │   └── cancel.go            # CancelCrawl handler
│   ├── worker/
│   │   └── temporal.go          # Temporal worker registration
│   ├── scraper/                 # MOVED from Quarry/internal/scraper
│   ├── driver/                  # MOVED from Quarry/internal/driver
│   ├── batch/                   # MOVED from Quarry/internal/batch
│   └── actions/                 # MOVED from Quarry/internal/actions
├── Dockerfile
├── go.mod
└── README.md
```

#### 1.2 Implement gRPC Server
**Create:** `services/crawler/internal/api/grpc/server.go`

```go
package grpc

import (
	"context"
	pb "quarry/proto/crawler/v1"
	"quarry/services/crawler/internal/scraper"
	"quarry/services/crawler/internal/storage"  // Service-local storage package
)

type CrawlerServer struct {
	pb.UnimplementedCrawlerServiceServer
	scraper *scraper.Scraper
	db      *storage.PostgresClient  // Crawler-specific DB client
}

func NewCrawlerServer(cfg *Config) (*CrawlerServer, error) {
	db, err := storage.NewPostgresClient(cfg.DatabaseURL)
	if err != nil {
		return nil, err
	}
	
	scraperCfg := &scraper.Config{
		UserAgent:    cfg.UserAgent,
		EnableStealth: cfg.EnableStealth,
	}
	
	return &CrawlerServer{
		scraper: scraper.NewScraper(scraperCfg),
		db:      db,
	}, nil
}

func (s *CrawlerServer) StartCrawl(ctx context.Context, req *pb.CrawlRequest) (*pb.CrawlResponse, error) {
	// Convert proto request to internal models
	// Enqueue to Temporal
	// Return job ID
	jobID := uuid.New().String()
	
	// Store job metadata
	job := &models.CrawlJob{
		ID:         jobID,
		URL:        req.Url,
		MaxDepth:   req.MaxDepth,
		Collection: req.Collection,
		Status:     "pending",
	}
	
	if err := s.db.CreateJob(ctx, job); err != nil {
		return nil, err
	}
	
	// Dispatch to Temporal workflow
	workflowID := fmt.Sprintf("crawl-%s", jobID)
	_, err := s.temporal.ExecuteWorkflow(ctx, temporal.StartWorkflowOptions{
		ID:        workflowID,
		TaskQueue: "crawler-queue",
	}, workflows.CrawlWorkflow, req)
	
	if err != nil {
		return nil, err
	}
	
	return &pb.CrawlResponse{
		JobId:  jobID,
		Status: "pending",
	}, nil
}

func (s *CrawlerServer) GetCrawlStatus(ctx context.Context, req *pb.GetCrawlStatusRequest) (*pb.CrawlStatus, error) {
	job, err := s.db.GetJob(ctx, req.JobId)
	if err != nil {
		return nil, err
	}
	
	return &pb.CrawlStatus{
		JobId:     job.ID,
		Status:    job.Status,
		Progress:  job.Progress,
		CreatedAt: timestamppb.New(job.CreatedAt),
		UpdatedAt: timestamppb.New(job.UpdatedAt),
	}, nil
}
```

#### 1.3 Create Dockerfile
**Create:** `services/crawler/Dockerfile`

```dockerfile
FROM golang:1.24-alpine AS builder

RUN apk add --no-cache git ca-certificates chromium nss freetype harfbuzz ttf-freefont

WORKDIR /build

# Copy proto definitions (for gRPC code generation)
COPY proto/ ./proto/

# Copy service code only (no shared libs)
COPY services/crawler/ ./services/crawler/

# Build
WORKDIR /build/services/crawler
RUN go mod download
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags='-s -w' -o /out/crawler ./cmd/server

# Runtime image
FROM alpine:3.20
RUN apk add --no-cache ca-certificates chromium nss freetype harfbuzz ttf-freefont

COPY --from=builder /out/crawler /usr/local/bin/crawler

ENV CHROME_BIN=/usr/bin/chromium-browser
ENV CHROME_PATH=/usr/lib/chromium/

EXPOSE 3043 9090

CMD ["/usr/local/bin/crawler"]
```

#### 1.4 Update Docker Compose
**File:** `docker-compose.yml`

```yaml
services:
  crawler-service:
    build:
      context: .
      dockerfile: services/crawler/Dockerfile
    ports:
      - "3043:3043"  # gRPC
      - "9093:9090"  # Metrics
    environment:
      - CRAWLER_GRPC_PORT=3043
      - CRAWLER_METRICS_PORT=9090
      - DATABASE_URL=postgresql://quarry:secret@postgres:5432/quarry?sslmode=disable
      - REDIS_URL=redis://redis:6379
      - TEMPORAL_HOST=quarry-temporal:7233
      - NATS_URL=nats://nats:4222
    depends_on:
      postgres:
        condition: service_healthy
      redis:
        condition: service_started
      temporal:
        condition: service_healthy
      nats:
        condition: service_healthy
    healthcheck:
      test: ["CMD", "grpcurl", "-plaintext", "localhost:3043", "grpc.health.v1.Health/Check"]
      interval: 10s
      timeout: 5s
      retries: 3
```

#### 1.5 Migration Strategy (Strangler Fig Pattern)

**Step 1:** Deploy crawler-service alongside existing monolith
```yaml
# Both services run simultaneously
quarry-api:        # Existing monolith (Port 8090)
crawler-service:   # New service (Port 3043)
```

**Step 2:** Route 10% of traffic to new service (feature flag)
```go
// In quarry-api
if featureFlags.Get("use_crawler_service") && rand.Float64() < 0.10 {
    // Call crawler-service via gRPC
    resp, err := crawlerClient.StartCrawl(ctx, req)
} else {
    // Use old monolith code
    resp, err := h.scraper.Crawl(ctx, req)
}
```

**Step 3:** Gradually increase traffic (10% → 50% → 100%)
**Step 4:** Remove old code from monolith once 100% migrated

### Deliverables (Week 3)
- [ ] crawler-service running independently
- [ ] gRPC API tested with grpcurl
- [ ] Both services running in parallel
- [ ] Metrics dashboard showing traffic split
- [ ] Integration tests passing

### Testing Strategy
```bash
# Unit tests
cd services/crawler && go test ./...

# gRPC integration test
grpcurl -plaintext -d '{
  "url": "https://example.com",
  "max_depth": 2,
  "collection": "test"
}' localhost:3043 crawler.v1.CrawlerService/StartCrawl

# Load test (compare old vs new)
k6 run tests/load/crawler-comparison.js
```

---

## Phase 2: Extract Query Service (Week 4-5)

### Objectives
- Create API gateway/router
- Handle request validation
- Route to appropriate backend service

### Tasks

#### 2.1 Create Service Structure
```bash
mkdir -p services/query/{cmd/server,internal/{api,router,middleware}}
```

**Directory structure:**
```
services/query/
├── cmd/
│   └── server/
│       └── main.go
├── internal/
│   ├── api/
│   │   └── http/
│   │       ├── server.go         # Fiber HTTP server
│   │       ├── crawl.go          # POST /v1/crawl
│   │       ├── import.go         # POST /v1/import
│   │       └── integration.go    # POST /v1/integration/*
│   ├── router/
│   │   └── dispatcher.go         # Routes to backend services
│   └── middleware/
│       ├── auth.go               # API key validation
│       ├── ratelimit.go          # Rate limiting
│       └── logging.go            # Request logging
├── Dockerfile
└── go.mod
```

#### 2.2 Implement Router/Dispatcher
**Create:** `services/query/internal/router/dispatcher.go`

```go
package router

import (
	"context"
	"fmt"
	
	crawlerpb "quarry/proto/crawler/v1"
	importerpb "quarry/proto/importer/v1"
	integrationpb "quarry/proto/integration/v1"
)

type Dispatcher struct {
	crawlerClient     crawlerpb.CrawlerServiceClient
	importClient      importerpb.ImportServiceClient
	integrationClient integrationpb.IntegrationServiceClient
}

func NewDispatcher(cfg *Config) (*Dispatcher, error) {
	// Create gRPC client connections
	crawlerConn, err := grpc.Dial(cfg.CrawlerServiceURL, grpc.WithInsecure())
	if err != nil {
		return nil, fmt.Errorf("failed to connect to crawler service: %w", err)
	}
	
	importConn, err := grpc.Dial(cfg.ImportServiceURL, grpc.WithInsecure())
	if err != nil {
		return nil, fmt.Errorf("failed to connect to import service: %w", err)
	}
	
	integrationConn, err := grpc.Dial(cfg.IntegrationServiceURL, grpc.WithInsecure())
	if err != nil {
		return nil, fmt.Errorf("failed to connect to integration service: %w", err)
	}
	
	return &Dispatcher{
		crawlerClient:     crawlerpb.NewCrawlerServiceClient(crawlerConn),
		importClient:      importerpb.NewImportServiceClient(importConn),
		integrationClient: integrationpb.NewIntegrationServiceClient(integrationConn),
	}, nil
}

func (d *Dispatcher) DispatchCrawl(ctx context.Context, req *CrawlRequest) (*CrawlResponse, error) {
	// Convert HTTP request to gRPC
	grpcReq := &crawlerpb.CrawlRequest{
		Url:        req.URL,
		MaxDepth:   int32(req.MaxDepth),
		Collection: req.Collection,
		Formats:    req.Formats,
	}
	
	// Call crawler service
	grpcResp, err := d.crawlerClient.StartCrawl(ctx, grpcReq)
	if err != nil {
		return nil, err
	}
	
	// Convert gRPC response to HTTP
	return &CrawlResponse{
		JobID:  grpcResp.JobId,
		Status: grpcResp.Status,
	}, nil
}
```

#### 2.3 Implement HTTP API
**Create:** `services/query/internal/api/http/crawl.go`

```go
package http

import (
	"net/http"
	"github.com/gofiber/fiber/v2"
)

func (h *Handler) HandleCrawl(c *fiber.Ctx) error {
	var req models.CrawlRequest
	if err := c.BodyParser(&req); err != nil {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"success": false,
			"error":   "invalid request body",
		})
	}
	
	// Validate request
	if err := req.Validate(); err != nil {
		return c.Status(http.StatusBadRequest).JSON(fiber.Map{
			"success": false,
			"error":   err.Error(),
		})
	}
	
	// Dispatch to crawler service
	resp, err := h.dispatcher.DispatchCrawl(c.Context(), &req)
	if err != nil {
		return c.Status(http.StatusInternalServerError).JSON(fiber.Map{
			"success": false,
			"error":   err.Error(),
		})
	}
	
	return c.JSON(fiber.Map{
		"success": true,
		"job": fiber.Map{
			"id":     resp.JobID,
			"status": resp.Status,
		},
	})
}
```

#### 2.4 Update Docker Compose
```yaml
services:
  query-service:
    build:
      context: .
      dockerfile: services/query/Dockerfile
    ports:
      - "3040:3040"  # HTTP API
      - "9090:9090"  # Metrics
    environment:
      - QUERY_HTTP_PORT=3040
      - QUERY_METRICS_PORT=9090
      - CRAWLER_SERVICE_URL=crawler-service:3043
      - IMPORT_SERVICE_URL=import-service:3041
      - INTEGRATION_SERVICE_URL=integration-service:3042
    depends_on:
      - crawler-service
```

### Deliverables (Week 5)
- [ ] query-service routing to crawler-service
- [ ] HTTP API backwards compatible with existing clients
- [ ] API documentation (OpenAPI spec)
- [ ] Rate limiting implemented
- [ ] Circuit breaker for downstream services

---

## Phase 3: Build Import Service (Week 6-8)

### Objectives
- Parse multiple file formats (PDF, DOCX, CSV, JSON, XLSX)
- Chunk large documents intelligently
- Handle bulk imports efficiently

### Tasks

#### 3.1 Create Service Structure
```bash
mkdir -p services/import/{cmd/server,internal/{api,parser,chunker,validator}}
```

#### 3.2 Implement File Parsers
**Create:** `services/import/internal/parser/pdf.go`

```go
package parser

import (
	"bytes"
	"io"
	
	"github.com/gen2brain/go-fitz"  // PDF parsing
)

type PDFParser struct{}

func (p *PDFParser) Parse(r io.Reader) (*Document, error) {
	// Read entire file to memory
	data, err := io.ReadAll(r)
	if err != nil {
		return nil, err
	}
	
	// Open PDF
	doc, err := fitz.NewFromMemory(data)
	if err != nil {
		return nil, err
	}
	defer doc.Close()
	
	// Extract text from all pages
	var text strings.Builder
	for i := 0; i < doc.NumPage(); i++ {
		pageText, err := doc.Text(i)
		if err != nil {
			continue
		}
		text.WriteString(pageText)
		text.WriteString("\n\n")
	}
	
	return &Document{
		Type:     "pdf",
		Text:     text.String(),
		Metadata: map[string]string{
			"pages": fmt.Sprintf("%d", doc.NumPage()),
		},
	}, nil
}
```

**Create:** `services/import/internal/parser/docx.go`

```go
package parser

import (
	"archive/zip"
	"encoding/xml"
	"io"
)

type DOCXParser struct{}

func (p *DOCXParser) Parse(r io.Reader) (*Document, error) {
	// DOCX is a ZIP archive
	data, _ := io.ReadAll(r)
	zipReader, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if err != nil {
		return nil, err
	}
	
	// Find document.xml
	var documentXML *zip.File
	for _, file := range zipReader.File {
		if file.Name == "word/document.xml" {
			documentXML = file
			break
		}
	}
	
	if documentXML == nil {
		return nil, fmt.Errorf("document.xml not found")
	}
	
	// Parse XML
	rc, _ := documentXML.Open()
	defer rc.Close()
	
	var doc WordDocument
	xml.NewDecoder(rc).Decode(&doc)
	
	// Extract text from paragraphs
	var text strings.Builder
	for _, p := range doc.Body.Paragraphs {
		for _, r := range p.Runs {
			text.WriteString(r.Text)
		}
		text.WriteString("\n")
	}
	
	return &Document{
		Type: "docx",
		Text: text.String(),
	}, nil
}
```

#### 3.3 Implement Intelligent Chunking
**Create:** `services/import/internal/chunker/splitter.go`

```go
package chunker

type ChunkStrategy string

const (
	StrategyFixedSize   ChunkStrategy = "fixed"
	StrategySentence    ChunkStrategy = "sentence"
	StrategyParagraph   ChunkStrategy = "paragraph"
	StrategySemantic    ChunkStrategy = "semantic"  // Use embeddings
)

type Chunker struct {
	maxChunkSize int
	overlap      int
	strategy     ChunkStrategy
}

func (c *Chunker) Chunk(text string) ([]Chunk, error) {
	switch c.strategy {
	case StrategyFixedSize:
		return c.chunkFixedSize(text)
	case StrategySentence:
		return c.chunkBySentence(text)
	case StrategyParagraph:
		return c.chunkByParagraph(text)
	case StrategySemantic:
		return c.chunkBySemantic(text)
	default:
		return c.chunkFixedSize(text)
	}
}

func (c *Chunker) chunkFixedSize(text string) ([]Chunk, error) {
	chunks := []Chunk{}
	runes := []rune(text)
	
	for i := 0; i < len(runes); i += c.maxChunkSize - c.overlap {
		end := i + c.maxChunkSize
		if end > len(runes) {
			end = len(runes)
		}
		
		chunks = append(chunks, Chunk{
			Text:     string(runes[i:end]),
			Position: i,
			Size:     end - i,
		})
		
		if end >= len(runes) {
			break
		}
	}
	
	return chunks, nil
}

func (c *Chunker) chunkBySentence(text string) ([]Chunk, error) {
	// Use sentence boundary detection (nltk equivalent in Go)
	sentences := splitSentences(text)
	
	chunks := []Chunk{}
	currentChunk := strings.Builder{}
	currentSize := 0
	
	for _, sentence := range sentences {
		sentenceLen := len([]rune(sentence))
		
		if currentSize+sentenceLen > c.maxChunkSize && currentSize > 0 {
			// Save current chunk
			chunks = append(chunks, Chunk{
				Text: currentChunk.String(),
				Size: currentSize,
			})
			currentChunk.Reset()
			currentSize = 0
		}
		
		currentChunk.WriteString(sentence)
		currentChunk.WriteString(" ")
		currentSize += sentenceLen + 1
	}
	
	// Save last chunk
	if currentSize > 0 {
		chunks = append(chunks, Chunk{
			Text: currentChunk.String(),
			Size: currentSize,
		})
	}
	
	return chunks, nil
}
```

#### 3.4 Implement Bulk Import Handler
**Create:** `services/import/internal/api/grpc/server.go`

```go
func (s *ImportServer) ImportBulk(stream importerpb.ImportService_ImportBulkServer) error {
	jobID := uuid.New().String()
	
	// Process stream of files
	filesProcessed := 0
	totalBytes := int64(0)
	
	for {
		req, err := stream.Recv()
		if err == io.EOF {
			// Stream finished
			break
		}
		if err != nil {
			return err
		}
		
		// Determine file type
		parser, err := s.parserFactory.GetParser(req.MimeType)
		if err != nil {
			return err
		}
		
		// Parse file
		doc, err := parser.Parse(bytes.NewReader(req.Content))
		if err != nil {
			// Log error but continue processing other files
			continue
		}
		
		// Chunk document
		chunks, err := s.chunker.Chunk(doc.Text)
		if err != nil {
			continue
		}
		
		// Store chunks in database
		for _, chunk := range chunks {
			s.db.CreateChunk(ctx, &models.Chunk{
				JobID:     jobID,
				FileName:  req.FileName,
				Text:      chunk.Text,
				Position:  chunk.Position,
				Metadata:  doc.Metadata,
			})
		}
		
		filesProcessed++
		totalBytes += int64(len(req.Content))
	}
	
	return stream.SendAndClose(&importerpb.BulkImportResponse{
		JobId:          jobID,
		FilesProcessed: int32(filesProcessed),
		TotalBytes:     totalBytes,
	})
}
```

### Deliverables (Week 8)
- [ ] Support for PDF, DOCX, CSV, JSON, XLSX
- [ ] 3 chunking strategies implemented
- [ ] Bulk import via streaming gRPC
- [ ] Import progress tracking
- [ ] Error handling with partial success

---

## Phase 4: Build Integration Service (Week 9-11)

### Objectives
- Connect to Notion, Google Drive, Slack
- Handle OAuth flows
- Rate limiting per integration
- Webhook receivers for real-time sync

### Tasks

#### 4.1 Create Service Structure
```bash
mkdir -p services/integration/{cmd/server,internal/{api,connectors/{notion,gdrive,slack},oauth,webhook}}
```

#### 4.2 Implement Notion Connector
**Create:** `services/integration/internal/connectors/notion/client.go`

```go
package notion

import (
	"context"
	"fmt"
	"time"
	
	"github.com/jomei/notionapi"
)

type NotionConnector struct {
	client      *notionapi.Client
	rateLimiter *rate.Limiter  // 3 requests/second per Notion docs
}

func NewNotionConnector(accessToken string) *NotionConnector {
	return &NotionConnector{
		client:      notionapi.NewClient(notionapi.Token(accessToken)),
		rateLimiter: rate.NewLimiter(rate.Every(time.Second/3), 1),
	}
}

func (c *NotionConnector) SyncDatabase(ctx context.Context, databaseID string) (*SyncResult, error) {
	// Rate limit
	if err := c.rateLimiter.Wait(ctx); err != nil {
		return nil, err
	}
	
	// Query database
	resp, err := c.client.Database.Query(ctx, notionapi.DatabaseID(databaseID), &notionapi.DatabaseQueryRequest{
		PageSize: 100,
	})
	if err != nil {
		return nil, err
	}
	
	result := &SyncResult{
		DatabaseID:  databaseID,
		PagesFound:  len(resp.Results),
		SyncStarted: time.Now(),
	}
	
	// Process each page
	for _, page := range resp.Results {
		// Extract page content
		blocks, err := c.client.Block.GetChildren(ctx, notionapi.BlockID(page.ID), nil)
		if err != nil {
			continue
		}
		
		// Convert to internal document format
		doc := c.convertPageToDocument(page, blocks.Results)
		
		// Send to import service for chunking/storage
		// ... (call import-service gRPC)
		
		result.PagesSynced++
	}
	
	// Handle pagination
	if resp.HasMore {
		// Recursively fetch next page
	}
	
	return result, nil
}

func (c *NotionConnector) SetupWebhook(ctx context.Context, callbackURL string) error {
	// Notion doesn't have native webhooks, use polling or Zapier integration
	// For this implementation, we'll use polling
	return fmt.Errorf("Notion webhooks not supported, use polling")
}
```

#### 4.3 Implement Google Drive Connector
**Create:** `services/integration/internal/connectors/gdrive/client.go`

```go
package gdrive

import (
	"context"
	"io"
	
	"google.golang.org/api/drive/v3"
	"google.golang.org/api/option"
)

type GoogleDriveConnector struct {
	service *drive.Service
}

func NewGoogleDriveConnector(accessToken string) (*GoogleDriveConnector, error) {
	srv, err := drive.NewService(context.Background(), option.WithHTTPClient(
		oauth2.NewClient(context.Background(), oauth2.StaticTokenSource(
			&oauth2.Token{AccessToken: accessToken},
		)),
	))
	if err != nil {
		return nil, err
	}
	
	return &GoogleDriveConnector{service: srv}, nil
}

func (c *GoogleDriveConnector) SyncFolder(ctx context.Context, folderID string) (*SyncResult, error) {
	result := &SyncResult{FolderID: folderID}
	
	// List files in folder
	query := fmt.Sprintf("'%s' in parents and trashed = false", folderID)
	fileList, err := c.service.Files.List().
		Q(query).
		Fields("files(id, name, mimeType, modifiedTime, size)").
		PageSize(100).
		Do()
	if err != nil {
		return nil, err
	}
	
	for _, file := range fileList.Files {
		// Skip folders (process recursively if needed)
		if file.MimeType == "application/vnd.google-apps.folder" {
			continue
		}
		
		// Download file content
		resp, err := c.service.Files.Get(file.Id).Download()
		if err != nil {
			continue
		}
		defer resp.Body.Close()
		
		content, _ := io.ReadAll(resp.Body)
		
		// Send to import service
		// ... (call import-service gRPC with streaming)
		
		result.FilesSynced++
		result.BytesProcessed += file.Size
	}
	
	return result, nil
}

func (c *GoogleDriveConnector) WatchFolder(ctx context.Context, folderID string, callbackURL string) (*WatchResponse, error) {
	// Use Google Drive Push Notifications
	channel := &drive.Channel{
		Id:      uuid.New().String(),
		Type:    "web_hook",
		Address: callbackURL,
	}
	
	watchResp, err := c.service.Files.Watch(folderID, channel).Do()
	if err != nil {
		return nil, err
	}
	
	return &WatchResponse{
		ChannelID:     watchResp.Id,
		ResourceID:    watchResp.ResourceId,
		ExpiresAt:     time.Unix(watchResp.Expiration/1000, 0),
	}, nil
}
```

#### 4.4 Implement OAuth Flow
**Create:** `services/integration/internal/oauth/handler.go`

```go
package oauth

type OAuthHandler struct {
	providers map[string]*OAuthProvider
}

type OAuthProvider struct {
	ClientID     string
	ClientSecret string
	RedirectURL  string
	Scopes       []string
	AuthURL      string
	TokenURL     string
}

func (h *OAuthHandler) HandleAuthorize(c *fiber.Ctx) error {
	provider := c.Params("provider")  // notion, google, slack
	
	config := h.providers[provider]
	if config == nil {
		return c.Status(404).JSON(fiber.Map{"error": "provider not found"})
	}
	
	// Generate state token (CSRF protection)
	state := generateSecureToken()
	c.Cookie(&fiber.Cookie{
		Name:     "oauth_state",
		Value:    state,
		MaxAge:   600,  // 10 minutes
		HTTPOnly: true,
		Secure:   true,
		SameSite: "Lax",
	})
	
	// Redirect to provider's OAuth page
	authURL := fmt.Sprintf("%s?client_id=%s&redirect_uri=%s&scope=%s&state=%s&response_type=code",
		config.AuthURL,
		config.ClientID,
		url.QueryEscape(config.RedirectURL),
		url.QueryEscape(strings.Join(config.Scopes, " ")),
		state,
	)
	
	return c.Redirect(authURL)
}

func (h *OAuthHandler) HandleCallback(c *fiber.Ctx) error {
	// Verify state token
	storedState := c.Cookies("oauth_state")
	receivedState := c.Query("state")
	if storedState != receivedState {
		return c.Status(400).JSON(fiber.Map{"error": "invalid state"})
	}
	
	// Exchange code for token
	code := c.Query("code")
	provider := c.Params("provider")
	config := h.providers[provider]
	
	token, err := h.exchangeCodeForToken(config, code)
	if err != nil {
		return c.Status(500).JSON(fiber.Map{"error": err.Error()})
	}
	
	// Store token in database (encrypted)
	// ... (save to postgres)
	
	return c.JSON(fiber.Map{
		"success": true,
		"provider": provider,
	})
}
```

### Deliverables (Week 11)
- [ ] Notion connector with OAuth
- [ ] Google Drive connector with OAuth & webhooks
- [ ] Slack connector (bonus)
- [ ] OAuth flow UI (simple HTML pages)
- [ ] Token refresh mechanism
- [ ] Rate limiting per integration

---

## Phase 5: Testing & Hardening (Week 12)

### Tasks

#### 5.1 Integration Tests
**Create:** `tests/integration/crawler_import_flow_test.go`

```go
package integration_test

func TestCrawlToImportFlow(t *testing.T) {
	// 1. Start crawl via query-service
	resp := callQueryService("/v1/crawl", CrawlRequest{
		URL: "https://example.com",
		Collection: "test",
	})
	
	jobID := resp.JobID
	
	// 2. Wait for crawl completion
	waitForJobCompletion(jobID, 30*time.Second)
	
	// 3. Verify data in crawler-service
	crawlData := getCrawlResult(jobID)
	assert.NotNil(t, crawlData)
	assert.True(t, len(crawlData.Pages) > 0)
	
	// 4. Import crawled data
	importResp := callImportService("/v1/import", ImportRequest{
		Source: "crawler",
		JobID:  jobID,
	})
	
	assert.True(t, importResp.Success)
}
```

#### 5.2 Load Testing
**Create:** `tests/load/all-services.js` (k6 script)

```javascript
import http from 'k6/http';
import { check, sleep } from 'k6';

export let options = {
  stages: [
    { duration: '2m', target: 50 },   // Ramp up
    { duration: '5m', target: 50 },   // Stay at 50 RPS
    { duration: '2m', target: 100 },  // Ramp to 100 RPS
    { duration: '5m', target: 100 },  // Stay at 100 RPS
    { duration: '2m', target: 0 },    // Ramp down
  ],
};

export default function () {
  // Test crawl endpoint
  let crawlResp = http.post('http://localhost:3040/v1/crawl', JSON.stringify({
    url: 'https://example.com',
    collection: 'load-test',
    maxDepth: 1,
  }), {
    headers: { 
      'Content-Type': 'application/json',
      'X-API-Key': 'secret',
    },
  });
  
  check(crawlResp, {
    'crawl status is 200': (r) => r.status === 200,
    'crawl returns job ID': (r) => JSON.parse(r.body).job.id !== undefined,
  });
  
  sleep(1);
}
```

#### 5.3 Chaos Engineering
**Create:** `tests/chaos/kill-random-service.sh`

```bash
#!/bin/bash
# Randomly kill services to test fault tolerance

services=("crawler-service" "import-service" "integration-service")
random_service=${services[$RANDOM % ${#services[@]}]}

echo "Killing $random_service to test fault tolerance..."
docker compose kill $random_service

sleep 10

echo "Restarting $random_service..."
docker compose up -d $random_service

echo "Verifying all services recovered..."
./tests/integration/health-check-all.sh
```

### Deliverables (Week 12)
- [ ] 50+ integration tests covering all service interactions
- [ ] Load test results documenting throughput/latency
- [ ] Chaos engineering test suite
- [ ] Performance benchmarks (before/after comparison)
- [ ] Production readiness checklist completed

---

## Deployment Strategy

### Blue-Green Deployment
```yaml
# docker-compose.blue.yml (current production)
services:
  quarry-api-blue:
    image: quarry-api:v1.5.0
    
# docker-compose.green.yml (new microservices)
services:
  query-service-green:
    image: query-service:v2.0.0
  crawler-service-green:
    image: crawler-service:v2.0.0
```

**Nginx routing:**
```nginx
upstream backend {
  server quarry-api-blue:8090 weight=90;
  server query-service-green:3040 weight=10;
}
```

### Rollback Plan
1. Monitor error rates in Grafana
2. If error rate > 5%, revert nginx config
3. Scale down green deployment
4. Investigate issues in staging

---

## Monitoring & Observability

### Metrics to Track
```go
// Prometheus metrics per service
var (
	requestDuration = prometheus.NewHistogramVec(
		prometheus.HistogramOpts{
			Name: "http_request_duration_seconds",
			Buckets: []float64{.005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10},
		},
		[]string{"service", "endpoint", "method"},
	)
	
	grpcDuration = prometheus.NewHistogramVec(
		prometheus.HistogramOpts{
			Name: "grpc_request_duration_seconds",
			Buckets: []float64{.005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10},
		},
		[]string{"service", "method"},
	)
	
	activeJobs = prometheus.NewGaugeVec(
		prometheus.GaugeOpts{
			Name: "active_jobs",
		},
		[]string{"service", "type"},
	)
)
```

### Grafana Dashboards
1. **Service Health Dashboard** - CPU, memory, request rate per service
2. **Job Processing Dashboard** - Queue depth, processing time, success rate
3. **Integration Dashboard** - API rate limits, OAuth token expiry, sync lag

---

## Success Metrics

| Metric | Baseline (Monolith) | Target (Microservices) |
|--------|---------------------|------------------------|
| API Response Time (p95) | 250ms | <150ms (query-service) |
| Crawl Throughput | 10 pages/sec | 50 pages/sec (horizontal scaling) |
| Import Throughput | 100 files/min | 500 files/min (parallel processing) |
| Service Availability | 99.5% | 99.9% (fault isolation) |
| Deployment Frequency | 1x/week | 5x/week (independent deployments) |

---

## Risk Register

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| gRPC contract changes break clients | High | Medium | Use buf.build for compatibility checks, versioned APIs |
| Database transaction spanning services | High | Low | Use saga pattern, event sourcing for consistency |
| Increased latency from network calls | Medium | High | Use connection pooling, gRPC keepalive, service mesh |
| OAuth token storage security | High | Medium | Encrypt tokens at rest, use Vault/K8s secrets |
| Service discovery in production | Medium | High | Use Consul/Kubernetes DNS, health checks |
| Code duplication from no shared libs | Low | High | Accept as tradeoff for independence; use code templates, linters for consistency |

---

## Next Steps

**Week 1 Immediate Actions:**
1. ✅ Review this roadmap with team
2. ✅ Create JIRA/Linear tickets from tasks
3. ✅ Set up proto repository
4. ✅ Configure CI/CD for proto generation
5. ✅ Start Phase 0 (Foundation)

**Decision Points:**
- [ ] Week 3: Go/No-go on crawler-service extraction (review metrics)
- [ ] Week 5: Choose between NATS vs Temporal for job queue
- [ ] Week 8: Decide on import-service language (Go vs Python for ML)
- [ ] Week 11: Production deployment approval

---

## Appendix A: Technology Stack

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| Service Framework | Go + Fiber | Existing expertise, high performance |
| gRPC | Protocol Buffers | Strongly typed, efficient, code generation |
| Message Queue | NATS JetStream | Lightweight, native Go, easier than Kafka |
| Service Mesh | Linkerd (optional) | mTLS, observability, lightweight |
| Config Management | Viper + etcd | Dynamic config, service discovery |
| Secrets | Vault / K8s Secrets | OAuth tokens, API keys |
| Observability | OpenTelemetry + Prometheus + Grafana | Industry standard |
| Load Balancing | Nginx / Traefik | Reverse proxy, TLS termination |

## Appendix B: Team Structure

**Recommended team assignments:**

- **Platform Team (2 engineers):** Phase 0, proto contracts, infrastructure (NATS, monitoring), CI/CD
- **Crawler Team (2 engineers):** Phase 1, crawler-service extraction
- **API Team (1 engineer):** Phase 2, query-service
- **Import Team (2 engineers):** Phase 3, file parsing, chunking
- **Integration Team (2 engineers):** Phase 4, OAuth, connectors
- **QA/DevOps (1 engineer):** Phase 5, testing, deployment

**Total:** 8-10 engineers for 12-week timeline

---

**Document Version:** 1.1  
**Last Updated:** February 19, 2026  
**Owner:** Platform Engineering Team  
**Status:** ✅ Ready for Review  
**Architecture:** 100% Service Isolation (No Shared Libraries)
