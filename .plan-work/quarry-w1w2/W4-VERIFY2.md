# W4 verify 2: E4.4-a dirty diff and E4.1-4 export tests (2026-11-08)

Worktree `gracious-gagarin-087139`, HEAD `b696f4d22`. Writer files untouched. Targeted cargo only, `-j 2`.

## Diff reviewed (13 modified files, 2 new, about +1036/-22; `git diff --check` clean)

E4.4-a: `locale`, `timezone` and `location` on `POST /v1/scrape`, behind `QUARRY_EDGE__PER_PAGE_LOCALE` (off by default).
- quarry-core: new `page_emulation.rs` (parse and range checks), `scrape.rs` options, limit, resolved and effective echo, canonical key, `is_default` false.
- quarry-edge: `scrape_contract.rs` (while off the keys stay unknown keys and are stripped before the options read), `scrape_actions.rs` (browser plan, hints), `routes.rs` (501 when no browser, stream refuses with `unsupported_in_stream` while on).
- quarry-runtime: the capability check runs before the pool is touched, an emulated fetch gets its own isolated lease, emulation is set between acquire and the egress policy and goto, and a failure releases the session.
- quarry-browser: CDP locale, timezone and geolocation overrides, with the geolocation grant for the URL origin only. The profile's emulation is replaced field by field. A failure fails the tab open, and the error carries a class and never the value.
- Docs: openapi, ERROR_REASONS, RUNBOOKS. They match the code (the `invalid_value`, `already_set` and `cdp_error` classes exist).

Review notes (none block):
1. The live Chromium tests (3, `#[ignore]`) did not run. The geolocation grant is untested end to end. The writer says so in the file and in the RUNBOOKS entry.
2. The stream route refusal is only covered by the pure helper, not by a route test.
3. `location.accuracy` is only read as a JSON number, so an integer such as `100` is fine.

## Tests run

| Command | Result |
|---|---|
| `cargo test -p quarry-core --lib -- page_emulation scrape` | 67 passed, 0 failed |
| `cargo test -p quarry-browser --lib -- page_emulation profile_restore capability_wire` | 25 passed |
| `cargo test -p quarry-browser --features chromiumoxide --lib -- page_emulation profile_restore` | 25 passed, 3 ignored (live Chromium) |
| `cargo test -p quarry-runtime --lib -- page_emulation_tests` | 5 passed (see environment note) |
| `cargo test -p quarry-edge --lib -- scrape_contract scrape_actions page_emulation job_results export` | 54 passed |
| `cargo test -p quarry-edge --test job_results` (export, E4.1-4) | 8 passed |
| `cargo test -p quarry-edge --test scrape_max_age` (a `ScrapeLimits` user) | 15 passed |

Total: 199 passed, 0 failed, 3 ignored.

## Environment note (not a code defect)

The first two `quarry-runtime` runs in `C:\qv2t` failed to compile. `quarry-runtime` linked an old `quarry-core` and `quarry-browser` (missing `driver_failed_reason`, `PublicBrowserAsset`, `ElementRect` and others, which exist in this tree). Six other cargo processes were sharing `C:\qv2t` ("Blocking waiting for file lock"). The likely cause is stale or colliding artifacts from a sibling worktree. The runtime tests then passed in a separate, clean target dir (`C:\qv2t-w4v2`, a one-off deviation from `C:\qv2t`, with the same `-j 2`). If another session reports `quarry-runtime` import errors in `C:\qv2t`, suspect the shared target dir before the code.

## Not done

No full workspace test. No Go test. No live Chromium test. No commit of writer files.
