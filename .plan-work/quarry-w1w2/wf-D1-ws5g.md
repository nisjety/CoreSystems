# D1-ws5g hand-over (2026-10-21)

Done: WS5.G hunks of the in-flight session finished and committed in three steps.
- 1b131060 model-gateway quarry.rs + grpc.rs (formats, inline markdown first, no escalation on non-html, UNSUPPORTED_MEDIA -> FailedPrecondition)
- 6b9ccd78 gateway knowledge/quarry.rs + search.rs (formats default, 415 -> unsupported_media, maxPages removed)
- 76751ffd SPA errors.ts, knowledge-client.ts, knowledge-preview.ts (+test), the two i18n scan tests (unsupported_media exemptions removed)
Analysis: D1-ws5g-analysis.md. Left uncommitted on purpose: apps/Model Plane/go/go.work.sum (not WS5.G), .plan-work/.

Results: model-gateway --lib 1749/0/4 ignored (native host, PROTOC from WinGet; no docker needed); gateway cargo test 776 passed, 1 ignored; clippy clean in the two gateway files; vitest (i18n, knowledge-preview, api) 581 passed, quarry-client.test.ts suite fails on the unresolved @quarry/client (pre-existing); tsc -b 33 errors = baseline; eslint 0 on the changed files.

Left: (1) Model Plane vendoring of scrape-options.schema.json with a digest pin (B11): nothing under apps/Model Plane references it; (2) live evidence of the plan's four stack checks (needs rebuilt quarry-edge/model-gateway/gateway images, live stack, out of this unit's rules); (3) full pnpm test and pnpm lint of the whole SPA not run; (4) merge with branch model-plane will conflict in model-gateway quarry.rs and grpc.rs.

Traps: model-gateway native test build took 14 minutes cold (shared target dir); running it natively avoided the docker lock and the mp-contracts protoc problem of mp-test.sh. cargo fmt --check on the gateway crate is red in about 20 files before this unit; do not run cargo fmt on the crate.
