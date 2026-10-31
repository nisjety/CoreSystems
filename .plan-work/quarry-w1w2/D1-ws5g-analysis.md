# D1-ws5g: analysis of the uncommitted WS5.G hunks (2026-10-21)

Source of truth: plan "Slice WS5.G detail" (docs/plans/quarry-browser-build-plan-2026-10-14.md), W1-work-breakdown item i, `git diff` of the seven files at the start of this unit. Nothing was discarded or reverted; every hunk below is kept.

## Hunks, file by file

### apps/Model Plane/rust/services/model-gateway/src/quarry.rs (+408/-45)
All of it is WS5.G (the Model Plane consumer side of the formats/inline-content contract). Nothing in it belongs to another slice.

| Hunk | What it does | WS5.G? | Unfinished? |
|---|---|---|---|
| `ScrapeOptions.formats: Vec<&'static str>` plus a hand-written `Default` (formats = `["markdown"]`), `browser()` and `scrape_readable` use `..Default` | the client always asks for inline markdown | yes | no |
| `Body.formats` with `skip_serializing_if = is_empty` | the request carries `formats` unless the list is empty | yes | no |
| `project()` reads top-level `markdown` first, then legacy `formats.markdown` (string), `text` falls back to markdown; `formats.markdown` as a `FormatRef` object stays for `artifact_ref` | the consumer half of the inline contract | yes | no |
| `scrape_with_options` comment rewrite; the existing `result.text.trim().is_empty() && !options.zdr` guard now makes the artifact read a fallback only | skips the second call when inline text is present, keeps it for an edge that did not inline | yes | no |
| `content_kind()` helper and the early return in `scrape_readable` | no browser escalation when the edge says the body is not html | yes (plan: "must not escalate on `content_kind != html`") | no |
| tests: 5 existing `body_json` mocks now include `formats`; new `inline_markdown_skips_the_artifact_call`, `zdr_inline_markdown_does_not_escalate_to_the_browser`, `edge_without_inline_support_falls_back_to_the_artifact_call`, `typed_unsupported_media_does_not_escalate`, `a_non_html_body_with_no_text_does_not_escalate`, `an_empty_html_page_still_escalates_once` (zdr false and true), `empty_formats_are_omitted_from_the_request`, `project_prefers_top_level_markdown_over_the_legacy_formats_string`, `project_leaves_a_format_ref_for_the_artifact_path` | all four wiremock tests the plan names plus extra counterparts | yes | no |

Observations: the typed 415 reaches the caller because `scrape_readable` propagates the `Err` with `?` (no escalation code needed); the debug line added for the non-html skip logs `%url` like the neighbouring existing debug line (Model Plane, not under the Quarry log-field lint). Branch `model-plane` also changes this file (work-breakdown line 138): expect textual conflicts at merge, not this unit's concern.

### apps/Model Plane/rust/services/model-gateway/src/grpc.rs (+29)
`quarry_err_to_status`: `UNSUPPORTED_MEDIA` becomes `FailedPrecondition` with "code: message" (plan: "`UNSUPPORTED_MEDIA` to `FailedPrecondition` with the reason"); one unit test (also checks an unknown code still maps to `Internal`). WS5.G, complete.

### apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs (+159)
- `scrape_preview` sets `formats: ["markdown"]` when the caller sent none, before the variant digest: WS5.G, complete.
- `classify_scrape_failure` maps 415 to `unsupported_media` (Norwegian text inline): WS5.G, complete (the WS3.J-era edits of the same function are committed).
- Tests `preview_makes_one_quarry_call_when_markdown_is_inline`, `preview_keeps_the_formats_the_caller_sent`, `a_415_is_classified_as_unsupported_media`, `preview_of_an_unsupported_media_type_is_a_typed_415` with two small helpers: WS5.G. Unfinished: one `rustfmt` deviation in `a_415_is_classified_as_unsupported_media` (fixed in this unit). The other `cargo fmt --check` diffs in the file are older than the hunks.
- Not WS5.G: nothing.

### apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs (+46/-2)
`maxPages` deleted from both fetch payloads (plan: the edge warns on it and the read of `/data/markdown` starts working) and the test `fetch_mode_excerpt_is_populated_from_inline_markdown`. WS5.G, complete.

### apps/Frontend Plane/verevonv3/src/shared/api/knowledge-client.ts (+8/-4)
Removes `renderHints` and `cachePolicy` from `ScrapeRequest`, adds `formats`, `only_main_content`, `include_tags`, `exclude_tags`; `ScrapeResult.rawHtml` becomes `raw_html`. WS5.G. Unfinished: a consumer still read `rawHtml` (`src/features/dashboard/home/knowledge-preview.ts`, the html fallback of `markdownFromResult`), so the renamed field was not read anywhere. Fixed in this unit (reads `raw_html`, keeps `rawHtml` as a fallback, one test).

### apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts (+1)
`unsupported_media` copy in Norwegian and English. WS5.G, complete. The two scan tests that exempted this code (`gateway-error-codes.test.ts`: `MAY_LAND_BEFORE_ITS_COPY`; `browser-errors.test.ts`: `code !== 'unsupported_media'` and `continue`) were written to survive either landing order. After the gateway code and the copy land together the exemptions are dead weight, so they are removed and replaced by one positive test (the scan sees the code in `domains/knowledge/quarry.rs`, both languages have copy).

### apps/Model Plane/go/go.work.sum (+1)
`go.uber.org/automaxprocs v1.6.0/go.mod`: a side effect of a `go` command, NOT WS5.G. Left uncommitted and untouched.

## What else the plan names, and where it stands
- `scrape-options.schema.json` golden and the Model Plane tool schema: the golden exists in Quarry-v2 (`docs/schemas/scrape-options.schema.json`, tested by `crates/quarry-core/tests/scrape_schema.rs` and `scripts/tests/openapi-scrape-parity.test.mjs`). The Model Plane has NO vendored copy and no drift guard: `git grep` for `scrape-options`, `scrape_options` and `only_main_content` under `apps/Model Plane` finds nothing. That vendoring is backlog item B11 (WS5.H last sentence: "The snapshot is vendored by the Model Plane as the tool schema with a digest pin (B11)"), not WS5.G, and it needs the Model Plane tool definition that does not exist yet. Reported, not built.
- `tool_loop.rs`: plan says no edit needed; none made.
- `execution-core/src/web_tools.rs`: unchanged by plan.
- `apps/Model Plane/go/pkg/quarry/client.go` dead code: plan says separate docs-hygiene PR.
- Deploy evidence (rebuilt images, four stack checks) needs live containers and the lock; out of this unit's rules (never restart a live container).
