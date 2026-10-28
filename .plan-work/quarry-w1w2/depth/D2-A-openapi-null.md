# D2-A-openapi-null: findings not fixed (2026-10-28)

1. spa-verify has not run on a real CI runner. Everything above ran locally with the same generator image (7.22.0), the same TypeScript line (5.9.3) and the Python test file CI uses. The runner could still differ on the Python dependency versions (pydantic is `>= 2.11`, unpinned). Owner: watch the first PR to main.
2. The "13 places" count in the 2026-10-27 notes was wrong (one bare null, 12 type arrays, one unquoted enum null); corrected in D-D-B3-openapi-sdk.md and its progress file.
3. Reader risk: `string | null` on a cookie `value` can suggest that values may be returned. The description ("Always null") and the server test (profile_guard.rs:213) stay the guard. If this matters for outside readers, a later decision could remove `value` from the schema (option C) and accept that the spec no longer matches the wire.
4. docs/openapi.yaml changes that follow (OPENAPI-INTERNAL, DATAPLANE-OPTION-B) must regenerate and commit sdks/typescript/docs in the same commit, with `MSYS_NO_PATHCONV=1 sdks/generate.sh`; the new openapi-no-bare-null test catches the bare-null regression early.
