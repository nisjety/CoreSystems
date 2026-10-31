# INNH-1 depth findings (logged, not for this wave)

From the contract design of 2026-10-31 (INNH-1-design.md). None blocks U1-U3.

1. ZDR, `/v1/extract` path: `ai_formats.rs` `validate_input` ends `let _ = zdr;` (302-310) and `ModelPlaneInvokeRequest` has no `zdr`, so a ZDR `/v1/extract` with a schema (when `MODEL_PLANE_URL` is set) sends page text to `/v1/invoke`, whose non-ZDR path writes a Session Core thread (plan WS16 state bullets; not re-verified on the Model Plane tree). INNH-1 does not use this path; INNH-2/3 own it. Fix: move `/v1/extract` onto the INNH-1 engine (`/v1/ai/chat`, ZDR refused) or refuse schema extraction under ZDR.
2. Validator subset: `$ref`, `anyOf`, `oneOf`, `const`, `pattern` are refused in v1 (D9). Widening is WS16.S4 (`ai_schema.rs` normalise + validator extension).
3. Harvest: JSON-LD / microdata / hydration context block and exact-name prefill (WS16.S5) are not in v1; pages whose facts live only in hydration state (SSB-type) will extract worse.
4. Output cap: `/v1/ai/chat` hard-codes `max_tokens: 4096`, `temperature: 0.7` (gateway http_routes.rs ai_chat); Quarry cannot cap per-format output until WS16.S1.
5. Cost: `/v1/ai/chat` returns no `cost_usd`, publishes no usage envelope, shares the org's rate bucket with chat users. WS16.S1.
6. inference-core response cache: non-ZDR prompts (page text) may be cached inference-side (plan cites `cache.rs` 105 skipping only ZDR). Retention of page text in that cache is unreviewed.
7. Token cap overshoot: with 4 calls in flight the per-request token cap can be exceeded by the in-flight calls; acceptable at one page, needs a reservation scheme for crawl/batch (WS16.S9).
8. Offsets: highlight offsets are Unicode scalar offsets; JS clients slicing with UTF-16 indices are wrong for astral characters. `text` is returned so slicing is not needed; revisit if the UI slices.
9. Model aliases: `/v1/ai/chat` acceptance of routing aliases (`verevon-balance`, `verevon-budget`) in `model` is assumed, not verified against inference-core routing.
10. Crawl/batch AI formats (WS16.S9) and an AI result cache are out of scope; `max_age` never applies to AI requests.
11. Two engines: `AiFormatRunner` (extract, answer pipeline) and the new `AiScrapeEngine` coexist until `/v1/extract` migrates; delete the old json path and `structured_extract.rs` (phantom `/v1/structured/extract` route, DEC-15) then.
12. Content-free counters for AI calls are tracing fields only; a Prometheus counter (`metrics.rs`) was left out to keep U3 small.
