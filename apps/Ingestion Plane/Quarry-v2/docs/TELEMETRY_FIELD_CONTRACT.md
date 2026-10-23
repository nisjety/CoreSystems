# Telemetry field contract (WS10.S8, F-t2-004 phase 2)

Status: built in the W2 batch (unit W2-U42), NOT run. The hard CI job `quarryotel-test`
(`.github/workflows/quarry-v2-ci.yml`) and the Rust tests (`cargo test -p quarry-edge --lib otel_zdr`,
`--test zdr_span_export`) are the check. Export is off in no environment until the evidence run
(see "Rollout").

## What the contract is

A span or log field is in one of three classes. The golden is the single source:

`apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/telemetry-field-contract.json`
(pinned by `.sha256` beside it and by `scripts/tests/zdr-contract-sync.test.sh`). Its shape is
`{ "version": 1, "safe": [...], "zdr_sensitive": [...], "never": [...] }`.

| Class | Meaning | Where it is enforced |
|---|---|---|
| `safe` | ids, enum outcomes, durations, counts, byte sizes, status codes, driver names, `request_id`, `run_id`, `step`, `zdr`, `error_class` | Nothing strips these; they may stay on any span. |
| `zdr_sensitive` | the field names that say "this holds what the caller asked about": url, uri, query, selector, title, host, text, error and similar. It equals WS4 `SENSITIVE_FIELDS` exactly. | Log: dropped while a ZDR span is in scope (`zdr_log.rs`). Spans: stripped from every span that is kept; a ZDR span is dropped whole. |
| `never` | credentials and typed input: authorization, cookie, set-cookie, token, password, otp, secret, api_key, typed_text | Stripped from every kept span, ZDR or not. |

`zdr_sensitive` includes `target` and `q`, which WS4 `SENSITIVE_FIELDS` already carries. The golden
follows the code, not the earlier draft list.

## Error classes

A failure is logged and exported as `error_class` (a closed class name from `zdr_log::class`), never
as its text. Span attributes use the same vocabulary as log fields: a span attribute whose name is in
`zdr_sensitive` is removed, so a span cannot carry a URL or an error message past the exporter.

## Span export switch

`QUARRY_TELEMETRY_EXPORT` (Rust and Go):

- unset or `on` (default): the exporter is built when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
- `off`, `0`, `false` (any case): no exporter and no provider are built, even if the endpoint is set.
  The process logs `telemetry export disabled by flag` and runs the log-only subscriber.

`QUARRY_TELEMETRY_ZDR_REGISTRY_CAP` bounds the ZDR trace registry (default 100000, FIFO eviction).

## How a ZDR trace is kept out

1. The sampler (`ZdrSampler` in Rust, `NewZdrSampler` in Go) drops a span created with `zdr=true`,
   or any span of a flagged trace, at span start.
2. A span that records `zdr = true` after it started flags its trace (the `ZdrFlagLayer` wrapper in
   Rust; `MarkZDR(ctx)` in Go where a ZDR posture is parsed after the span started, as in
   quarry-control's change webhook and schedule handlers).
3. The span processor drops a flagged or `zdr` span at end, and flags its trace so later spans of it
   are dropped too.
4. The Go exporter wrapper (`redactingExporter`) strips the `zdr_sensitive` and `never` keys from
   every span it sends.

## Limits (do not rely on more)

- A span that ENDED before its trace was flagged is already in the batch and is not recalled. The
  same limit holds for logs (`zdr_log.rs`).
- The sampler sees only the attributes present when the span starts. A `zdr` recorded later is
  caught by the flagged-trace registry, not by the sampler.
- Event fields on OTel span events are not stripped by the span processor. Log events are filtered
  by the ZDR log rule; span events inherit only the exporter-level trace drop (depth item).
- Model Plane `mp-telemetry` (otel 0.26) is a sibling-(b) follow-up with the same flag and golden.
  The gateway has no OTLP export, so its phase 2 is empty.

## Per-plane mirroring

- Quarry Rust: `crates/quarry-edge/src/otel_zdr.rs` (constants pinned by the test
  `field_constants_equal_the_golden`, which reads the golden in place).
- Quarry Go: `pkg/quarryotel/fieldcontract.go` (constants; `testdata/` holds a byte-identical copy
  checked by `TestFieldConstantsEqualGolden`).
- Changing the vocabulary: edit the golden, refresh its `.sha256`, update the digest in
  `zdr-contract-sync.test.sh`, re-vendor the Go copy, and change the two constant sets. The span
  vocabulary PR (WS6 L2 / F-t2-029) adds its attribute names to the golden in the same change.

## Rollout

- `QUARRY_TELEMETRY_EXPORT=on` in dev first. No environment with a real collector sets
  `OTEL_EXPORTER_OTLP_ENDPOINT` before the evidence run.
- Evidence (owed, not run in this batch): `deploy/compose/docker-compose.otel-test.yml` with the
  collector in `deploy/compose/otel-test/collector.yaml`. One non-ZDR and one ZDR scrape plus step:
  the collector shows spans for the non-ZDR trace only, the ZDR container log has no probe URL, and
  with `QUARRY_TELEMETRY_EXPORT=off` the collector receives nothing.
