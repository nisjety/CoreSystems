# INNH depth findings, lane D (not fixed; one line each)

Format: `item | finding`. Wave first, depth later (user order 2026-10-23): none of these is a fix loop.

- INNH-8 | `/v1/answer` and `/v1/answer/stream` under ZDR still send page text to the Model Plane (the model hop has no ZDR posture, ADR-4). `include_answer` now skips under ZDR, so the two surfaces disagree. The `answer` row of the ZDR matrix still says Honored; it should become a gap with a `gap_rows_currently_violate_*` test, or the routes should refuse under ZDR like the AI formats. Needs the owner's choice with INNH-8b.
- INNH-8 | An `include_answer` search runs the provider twice (the route's search, then the pipeline's own) and reads up to 5 pages; the pipeline could take the route's results instead. Cost, not correctness.
- INNH-8 | The answer fetcher (`SimpleHttpMarkdownFetcher`) fetches result URLs from the edge; confirm it goes through the web policy and egress guard (answer candidate filter WS7.G is not done).
- INNH-8 | Answer routes keep the legacy `{error, code, hint, reason}` body, not the typed envelope with `details.reason`; moving them is a breaking change for clients that parse `error` as a string, so it was left. A typed envelope could be offered under a version or header later.
- INNH-8 | `model_plane_rate_limited` only arises from a rate-limited Auth Core mint: `decode_invoke` maps a gateway 429 to `DriverFailed`, so a gateway rate limit shows as `model_plane_error`. Mapping 429 in `mp_client.rs` would make the reason exact.
- INNH-8 | The stream route classifies its own model errors by hand (`classify(true, code)`) instead of through `AnswerPipeline::answer`; if the gateway ever implements `/v1/invoke/stream`, tag the hop in `query_stream` too.
- INNH-8 | The failed-synthesis cache rule is proven by unit tests of `cacheable` and `usable_hit` and by a Redis integration test that skips without Redis; add `answer_provider_status` to the CI step that runs with a Redis service and `REQUIRE_CONTRACT=1`.
- INNH-8 | Deployments that already set `MODEL_PLANE_URL` and relied on synthesis being implicitly on must now also set `QUARRY_EDGE__ANSWER_SYNTHESIS=1`. The live compose has an empty URL so nothing changes there; a side-by-side stack with the URL set will answer 501 `synthesis_disabled` until it does.
