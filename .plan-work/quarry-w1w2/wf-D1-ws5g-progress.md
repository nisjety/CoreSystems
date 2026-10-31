# D1-ws5g progress (updated 2026-10-21)

Committed: 1b131060, 6b9ccd78, 76751ffd. Unit complete.
Analysis: D1-ws5g-analysis.md written.
My uncommitted edits (on top of the in-flight WS5.G hunks): gateway knowledge/quarry.rs (rustfmt fix of one test), SPA knowledge-preview.ts (+raw_html), knowledge-preview.test.ts (+1 test), gateway-error-codes.test.ts and browser-errors.test.ts (unsupported_media exemptions removed).
Results so far (scratchpad D1-ws5g/ logs):
- gateway native `cargo test --no-fail-fast`: 758+1+13+3+1 = 776 passed, 0 failed, 1 ignored (gw-test.log)
- vitest src/shared/i18n + knowledge-preview + src/shared/api: 581 passed; 1 failed SUITE = quarry-client.test.ts (@quarry/client import unresolved, pre-existing baseline)
- model-gateway --lib native (PROTOC from WinGet): running -> mg-lib.log
- tsc -b + eslint on changed files: running -> tsc.log, eslint.log
Next: confirm results, commit in this order: (1) model-gateway quarry.rs+grpc.rs, (2) gateway quarry.rs+search.rs, (3) SPA files. Use qcommit.sh with message files in scratchpad/D1-ws5g/.
Never commit: apps/Model Plane/go/go.work.sum, .plan-work/.

## Round 4 (review fix)
Committed 96953ef2: zdr_scrape_does_not_resolve_artifacts now mounts GET artifact with expect(0); mutant (&& !options.zdr removed) fails it, restored, lib 1749/0/4.

Round 4 part 2: committed e92d055d (gateway tests: 415 static + preview cache key). Both mutants caught, restored. All round-4 findings addressed (3rd by 96953ef2).
