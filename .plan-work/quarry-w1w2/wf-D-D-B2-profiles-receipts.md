# D-D-B2-profiles-receipts hand-over (2026-10-27)

Done: every named test passes (profile_owner 5, import_over_cap_is_413, redaction_wire 3, the two quarry-runtime tests); D1-030 (qr-redact/4: password and session-cookie pairs, CSRF headers, any-case JWT), D2-064 (event masks an unknown raw-type control), D2-045 (the owner guard is now called on a start_run that names a profile_id; the WS8.F attach.rs names no profile and is untouched).

Traps found:
- A member's token with no `initiated_by` is a Human step; on a non-ZDR run its receipt is metadata-only (`[redacted]`), so older WS12.S3 tests that expect `[redacted:password]` must send `initiated_by: agent` (REST body and WS message).
- `SessionSnapshot.session_storage` has no serde default: a partial snapshot body is a 422, not a size refusal.
- The save route needs `quarry:profiles:import`; tests that call `save_profile` directly need it in their claims.
- The harness driver double reports no atomic target actions; call `edge.driver.claim_atomic_targets()` for a ref action to reach the snapshot check.
- Pin trap: fingerprint changes with the probe corpus too (renaming a probe key re-pins). Order: edit rules, run redaction_golden for the printed fingerprint, then golden sha256, .sha256 file, sync script digest.
- Durable masks a whole value under a key named `cookies` (key rule), so pair tests use another key.
