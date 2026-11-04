# W2-FE-WS16S7 status: WS16.S7 gateway forwards schema and prompt on extract (lane b, branch w23/lane-b)

## Built
Checked first. Already in `runs.rs` before this unit: `create_extract` forwards `schema` and `prompt` to quarry-edge `POST /v1/extract` (`extract_schema`, `extract_prompt`), the typed 501 mapping (`extract_refusal_code`, `with_extract_refusal_code`) and its unit tests. The gap was the status of a malformed schema (`validation()` answers 422; the scope says 400) and the missing forward and reject tests. Only that gap was built:

- `shared.rs`: `bad_request(code, message)`, the 400 twin of `validation()`, with the standard `{error:{code,message}}` envelope. `validation()` is unchanged.
- `runs.rs` `create_extract`: a schema that is present but is not a JSON object (or text that is not one) answers 400 `invalid_schema` before any upstream call. A missing URL and the prompt limit keep 422.
- `runs.rs` `extract_schema`: doc comment updated to the new refusal.
- Tests: unit test `extract_schema_accepts_an_object_or_its_text_and_refuses_the_rest` in `mod tests`; new module `extract_forward_tests` with wiremock doubles of quarry-edge: schema and trimmed prompt forwarded beside the URL; schema sent as text forwarded as an object; the markdown route sends only the URLs when the schema is null and the prompt blank; malformed schema is a 400 with zero upstream calls; over-long prompt is a 422 with zero upstream calls; quarry-edge's 501 AI-formats refusal keeps its status and gets `extract_ai_formats_off`.

Files: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ingestions/runs.rs`, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ingestions/shared.rs`.

## Commits (lane b, local, not pushed)
- f8c8373e1 fix(gateway): a malformed extract schema answers 400 (WS16.S7)
- 5b961623b test(gateway): extract forwards schema and prompt; refusals never reach quarry (WS16.S7)
- this file and the lane b depth rows: docs(plan-work) commit that adds it (sha in the lane log)

## Code only, not compiled
No cargo check, build, test or clippy was run (owner order 2026-11-03). `rustfmt --check` was run on copies in the scratchpad: the new code has no formatting hunks; the file's pre-existing unformatted hunks were left alone. Wiremock 0.6.5 signatures were checked in the registry source (`Times: From<u64>`, `MockServer::received_requests() -> Option<Vec<Request>>`, `Request.body: Vec<u8>`, `ResponseTemplate::new<S: TryInto<StatusCode>>`). The integration step must compile-check and run them: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets ingestions`, then clippy.

## Flag and default
No flag. The forward already ran in this route without one. This unit changes only the status code of a request that was already refused, and adds tests. The live switch for structured extraction stays quarry-edge's `QUARRY_EDGE__AI_FORMATS` (501 when off). A gateway flag around the forward would silently drop `schema` and `prompt`, so none was added.

## Assumed decisions
1. 400, not 422, for a malformed schema (scope). Done with a new `bad_request()` rather than by changing `validation()`, so every other 422 keeps its status.
2. Code `invalid_schema`, a domain code in the style of `invalid_artifact` and `unsupported_schedule_kind` elsewhere in the gateway. Message: "The extraction schema must be a JSON object."
3. A blank schema text is "no schema", not a refusal. The SPA's optional field accepts a string (`ingestionsCreateRunInput`).
4. A schema sent as text is parsed by the gateway and forwarded as an object. quarry-edge takes any JSON value (`Option<serde_json::Value>`), so this check is the only object-shape check on the run path.
5. The prompt limit stays 422 `validation_error` (the scope names only the schema). quarry-edge answers 400 for the same case. Depth row W23-b-WS16S7-3.
6. A non-string `prompt` is still dropped silently, as before (not named in the scope). Depth row W23-b-WS16S7-4.
7. No UI work. `ingestions.create_run` already carries `schema` and `prompt`, and neither `ingestions-client.ts` nor the knowledge features branch on a 422 from extract.

## MP asks
None. This unit calls only quarry-edge. No Model Plane call, so no MP-ASKS entry.

## Not verified
- Nothing compiled or run. The unit test and the six `extract_forward_tests` functions have not executed.
- No live quarry-edge run. The forward is proven only against wiremock doubles, once the integration step runs them.
- Two findings sit outside this unit's files and were not changed: the schedule path copies `schema` without the object check (`schedules.rs`), and quarry-core's `json_schema::validate` reads a non-object schema as permissive. Both are in the lane b depth rows.
