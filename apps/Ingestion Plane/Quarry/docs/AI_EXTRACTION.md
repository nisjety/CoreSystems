# AI Extraction Integration

## Overview

Quarry uses an intelligent dual-mode extraction system that combines AI-powered analysis with fast heuristic fallback for optimal performance and reliability.

## Architecture

```
┌─────────────────┐
│  Scraper Engine │
└────────┬────────┘
         │
         v
┌─────────────────┐
│  LLM Extractor  │
└────────┬────────┘
         │
         ├──[AI Enabled]──> ┌──────────────┐
         │                  │  ai-core     │
         │                  │  (gRPC)      │──> Success ──> Return Data
         │                  └──────┬───────┘
         │                         │
         │                         v Failure/Timeout
         │                         │
         └─────────────────────────┴──> ┌──────────────┐
                                        │  Heuristic   │
                                        │  (goquery)   │──> Return Data
                                        └──────────────┘
```

## How It Works

### 1. Primary Path: AI Extraction
When `ENABLE_AI_EXTRACTION=true` and `ai-core` is available:

- **Timeout**: configurable per deployment via `AI_EXTRACTION_TIMEOUT_SEC` (default: 60 seconds)
- **Schema**: JSON schema defines expected product fields
- **Transport**: TOON is used only on the Quarry `internal/ai` to `ai-core` boundary
- **Format**: Quarry caches and API responses remain structured JSON
- **Fallback**: On any failure, immediately falls back to heuristic

### 2. Fallback Path: Heuristic Extraction
Used when:
- AI client is disabled (`ENABLE_AI_EXTRACTION=false`)
- AI client failed to initialize
- AI extraction times out or errors
- ai-core is unavailable

**Method**: CSS selector-based parsing with goquery

## Configuration

### Environment Variables

```bash
# Enable/disable AI extraction (default: true)
ENABLE_AI_EXTRACTION=true

# ai-core gRPC address (default: localhost:50851)
AI_CORE_GRPC_ADDR=localhost:50851
```

### Performance Tuning

- **AI Extraction Timeout**: configurable via `AI_EXTRACTION_TIMEOUT_SEC` (default: 60 seconds)
 - Fast enough for real-time scraping
- Prevents hanging on slow AI responses

**Heuristic Fallback**: <100ms typical
- Instant fallback on AI failure
- Zero external dependencies

## Metrics & Monitoring

### Log Events

**AI Success**:
```json
{"level":"debug","method":"ai","message":"extraction successful via ai-core"}
```

**AI Fallback**:
```json
{"level":"warn","error":"context deadline exceeded","message":"ai extraction failed, falling back to heuristic"}
```

**Heuristic Extraction**:
```json
{"level":"debug","method":"heuristic","message":"using heuristic extraction"}
```

### Expected Behavior

| Scenario | Extraction Method | Latency | Notes |
|----------|------------------|---------|-------|
| ai-core healthy | AI | 1-5s | Best quality |
| ai-core slow | AI (times out) → Heuristic | 60s + <100ms | Automatic failover |
| ai-core down | Heuristic | <100ms | No degradation |
| AI disabled | Heuristic | <100ms | Consistent performance |

## Production Recommendations

### Phase 1 (Current)
- **Setting**: `ENABLE_AI_EXTRACTION=true`
- **Strategy**: Let heuristic handle failures gracefully
- **Monitoring**: Watch for high fallback rates

### Phase 2.1 (AI Reliability Gate)
- Add circuit breaker (skip AI if failure rate >20%)
- Add AI health polling
- Add extraction method metrics counter

### Phase 2.2 (AI Efficiency Gate)
- Cache AI responses (plan: 24h, extract: 1h)
- Keep TOON isolated to the `internal/ai` transport layer and JSON everywhere else
- Add cost/latency telemetry

## Code Locations

| Component | Path | Purpose |
|-----------|------|---------|
| Extractor Logic | `internal/extractor/llm.go` | Dual-mode extraction |
| AI Client | `internal/ai/client.go` | gRPC communication |
| AI Transport Codec | `internal/ai/transport_codec.go` | TOON-only ai-core boundary |
| Config | `internal/config/config.go` | Feature flags |
| Wiring | `cmd/api/main.go` | AI client initialization |

## Testing

### Test AI Extraction
```bash
# Start ai-core (if not running)
cd /Volumes/Lagring/Triodelab/CoreSystem/apps/backend/ai-core
python -m uvicorn main:app --port 50851

# Start Quarry with AI enabled
cd /Volumes/Lagring/Triodelab/Quarry
ENABLE_AI_EXTRACTION=true go run ./cmd/api
```

### Test Heuristic Fallback
```bash
# Disable AI
ENABLE_AI_EXTRACTION=false go run ./cmd/api

# OR leave AI enabled but stop ai-core to test automatic fallback
```

### Verify Integration
```bash
# Check logs for extraction method
curl -X POST http://localhost:8090/v1/scrape \
  -H "Content-Type: application/json" \
  -d '{"url":"https://example.com","collection":"test"}'

# Look for log line: "method":"ai" or "method":"heuristic"
```

## FAQ

**Q: What happens if ai-core crashes mid-request?**  
A: The configured `AI_EXTRACTION_TIMEOUT_SEC` deadline triggers, and heuristic extraction completes the request.

**Q: Does the heuristic method affect quality?**  
A: Heuristic extraction is optimized for Norwegian e-commerce sites and works well for structured product pages. AI extraction provides better accuracy for complex/varied layouts.

**Q: Can I disable AI extraction for specific requests?**  
A: Not yet. It's a global flag. Phase 2 will add per-request control.

**Q: How do I monitor AI usage?**  
A: Phase 2.2 will add Prometheus metrics. For now, grep logs for `"method":"ai"` vs `"method":"heuristic"`.
