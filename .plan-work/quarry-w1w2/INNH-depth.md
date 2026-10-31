# INNH depth findings (not fixed; one line each)

Format: `item | unit | finding`. Wave first, depth later (user order 2026-10-23): none of these is a fix loop.

- INNH-1 | U1 | The hand-written `ScrapeFormat` JsonSchema (scrape.rs) repeats the AI limits (200..10000, 1..10, 40..1000, 10000, 1000) by hand; `openapi-scrape-parity.test.mjs` checks OpenAPI against the golden but nothing checks the golden against the `ai_spec` constants. A Rust test in `tests/scrape_schema.rs` that reads the consts would close it.
- INNH-1 | U1 | The effective-options golden shares the `ScrapeFormat` schema, whose string enum lists the input names (`rawHtml`), while `wire_name()` writes `raw_html` and now `json`, `question`, `highlights`. The effective golden has described the echo loosely since WS13.4b; a separate `EffectiveFormat` schema would make it exact.
- INNH-1 | U1 | `empty` (blank prompt, question or query) is a code the design table does not list; ERROR_REASONS.md and CONTRACTS.md now do. Confirm with the Innhenting UI copy, or fold it into `missing_field`.
- INNH-1 | U1 | AI object fields are snake_case only (`max_chars`), as the design says, while the capture options and the root options also take camelCase (`fullPage`, `maxInlineBytes`). A Firecrawl-style caller sending `maxChars` gets `unknown_field`. Add aliases only if the UI needs them.
- INNH-1 | U1 | The `scrape` and `scrape_stream` rows of `zdr_matrix.rs` describe the AI refusal in `basis` but list no `enforced_by` test: `tests/zdr_feature_matrix.rs` verifies each reference exists, and the U3 tests do not exist yet. U3 adds `ai_formats::...` to `enforced_by` and regenerates the golden.
- INNH-1 | U1 | Schema `type` may be a list (`["integer","null"]`) and `enum` any array; the validator in `json_schema.rs` supports both but a list with an unknown member is `invalid_value` here while the validator itself would report `InvalidSchema` at run time. Same family as the WS16.S4 validator widening.
- INNH-1 | U1 | `schema_too_complex` counts `properties` entries but not `enum` members or `required` names; the 65,536-byte cap bounds them. A schema of 65 KB made of one huge `enum` is accepted and costs model input tokens.
