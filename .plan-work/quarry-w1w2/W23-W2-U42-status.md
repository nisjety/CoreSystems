# W2-U42 status: quarry-control request span so MarkZDR works (D2-021, D2-143)

Lane c, branch `w23/lane-c` (fast-forwarded to `w23/integ` first). Plane: Ingestion (quarry-control, pkg/quarryotel).

## Built

The gap was the whole middleware: quarry-control opened no request span anywhere, so `quarryotel.MarkZDR(r.Context())` in `change_webhook.go:86` and `cycle23.go:199` found no valid span and returned. The two call sites themselves were already right and are untouched.

- `internal/httpx/zdr_span.go`: `ZdrSpan` / `ZdrSpanWith(tp)`. One server span per request, adopts an inbound W3C `traceparent`, carries only contract fields (`method`, `request_id`, `status_code`; never the URL, query or a header). With `MarkZDR` called in the handler, the trace is flagged on the shared registry and the span gets `zdr=true`, so the processor in pkg/quarryotel drops the whole trace. No exporter: the global no-op provider, the span is free.
- `pkg/quarryotel/provider.go`: exported `NewTracerProvider(exp, registry, exportOn, opts...)` that wires the real ZDR sampler, span processor and redacting exporter over any exporter. Needed because the processor is unexported and the test must use the real one, not a copy. `Init` is unchanged.
- `cmd/control/main.go`: one added line, `r.Use(httpx.ZdrSpan)`, after the existing `r.Use(RequestID, Logger, Recover)` (inside Recover, after RequestID so `request_id` is on the span).
- `go.mod` (quarry-control): `otel`, `otel/sdk`, `otel/trace` lose their `// indirect` mark (now imported directly). Not in the file list, required for a tidy module.

## Commits

- `eb9ca86cd` fix(quarry-control): request span middleware so MarkZDR drops ZDR traces (W2-U42, D2-021)
- plus a docs commit for this file and the depth rows

## Tests (all run, all pass)

- Go `services/quarry-control/internal/httpx`: 5 new `TestZdrSpan*` tests (clean span kept with method/status_code/request_id and no sensitive key or query leak; `MarkZDR` in the handler gives zero exported spans, child dropped, trace on the shared registry; valid span in the handler context; inbound traceparent adopted as trace and parent; no-op global provider passes the request through). Package ok.
- Go `pkg/quarryotel` (`go test ./...`): 6 existing tests pass (goldens `TestFieldConstantsEqualGolden`, sampler, MarkZDR, registry FIFO, redacting exporter, export-off). Nothing needed fixing.
- Go `services/quarry-control`: `go build ./...`, `go vet` of httpx and cmd/control, `go test ./internal/resources/` ok.
- Rust (lane c, wrapper), D2-143 "written, never run": `cargo test -p quarry-edge --lib otel_zdr` 12 passed, 0 failed; `--test zdr_span_export` 5 passed, 0 failed. Run for the first time here; nothing to fix.

## Flag and default

None (telemetry hygiene, a bug fix). The span is a no-op unless `OTEL_EXPORTER_OTLP_ENDPOINT` is set and `QUARRY_TELEMETRY_EXPORT` is not off.

## Assumed decisions

- Span name is the method only, no `route` attribute (httpx has no chi dependency; route would need a chi-aware wrapper). Recorded as a depth row.
- Middleware lives inside Recover/Logger rather than editing the existing `r.Use` line, to keep the main.go change append-only.
- Exported `NewTracerProvider` in quarryotel rather than duplicating the unexported processor in the httpx test.

## Model Plane asks

None.

## Not verified

- No live run with a real OTLP collector: the end-to-end drop is proven against the recording exporter with the real sampler/processor, not over the wire.
- The real `change_webhook` / `cycle23` handlers were not driven through `ZdrSpan` with a ZDR body (the test calls `MarkZDR` from a stub handler; the call sites are unchanged).
- Whole-module `go test ./...` of quarry-control was not run, only httpx, resources, and `go build ./...`.

## Depth rows

Four rows appended to `W23-depth-lane-c.md` (no route attribute, orchestrator has no request span, statusRecorder lacks Flusher, no handler-level ZDR test).
