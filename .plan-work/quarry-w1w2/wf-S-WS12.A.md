# S-WS12.A hand-over (WS12.S0)

Commits (local, not pushed): d405aa87 feat(quarry-core) module + golden + harness; 61499db8 docs + sync rows + CI paths + register.

## What is there
quarry_core::redaction (crates/quarry-core/src/redaction/{mod,shapes,field,luhn,url,json,stamp}.rs), tests/redaction_golden.rs, tests/fixtures/redaction/secret-shapes.v1.json + .sha256, docs/SECRET_REDACTION.md. Design note: wf-S-WS12.A-design.md. Ledger text: wf-S-WS12.A-ledger.md.

## How to use it in S1/S2/S3/S6
- Always pass an explicit mode in tests (`redact_text_with(Mode::Enforce, ..)`); `redact_text` reads the process-wide mode and the FIRST read pins it, so a test binary that reads it can no longer init_mode (that is why the lib unit tests and tests/redaction_golden.rs split the two halves).
- Edge boot (S2): `Mode::parse(env)`, refuse `!permitted_in_production()` when ENVIRONMENT=production, then `init_mode` BEFORE anything redacts.
- Counting: `Tally` then `tally.stamp(profile, mode)`; the stamp's probe and screenshot fields are for S1 and S5 to set.
- `field::classify` returns the rule id too; `field::mask` returns a Cow.

## Traps
- Changing ANY pattern, key list, label, threshold or the card table fails redaction_rule_drift_requires_a_version_bump: bump RULE_VERSION, add the pair to PINNED_FINGERPRINTS, edit the golden, refresh its .sha256, the digest in scripts/tests/zdr-contract-sync.test.sh, every vendored copy.
- The golden pin must be recomputed from the final LF bytes (`sha256sum`); the test and the sync script both check it.
- `.sha256` files are not pinned to LF by .gitattributes (only *.json is), same as the existing pins; if S4 vendors the pin into the Model Plane a Windows checkout may turn it CRLF (the Rust test tolerates it; a byte-compare VENDORED row would not).
- A mutating `python3 -` / `cat >` with no heredoc hangs this Bash tool (waits on stdin): write scripts with the Write tool.
- The quarry-rust-env image has no clippy (verified); rustfmt was run on the host over the new files.
- Do NOT use the Bash tool `sleep N; cat` to wait: use qwait.sh.

## Decisions I took (flag if wrong)
- 64 KiB JSON leaf cap NOT implemented (truncation breaks extraction results, skipping leaks; the engine is linear and the 1 MiB budget test is the guard).
- Object-key rule runs in Durable only (Live would blank `password_requirements`-style extraction fields).
- Extra rule ids: shape.whsec, json.key.
- bearer/basic need an Authorization header context or a long token with a digit or base64 symbol, so prose survives; the cost is that a short bare `Bearer abc` outside a header is not masked.
- redact_sensitive is documented superseded but left in place (only AgentScratchpad::note calls it).

## Not done / next
S1 driver probe (+ Q10), S2, S3 (needs X-19), S4 mp-scrub crate (vendor the golden, add the VENDORED rows, the consumer row then passes; keep REQUIRE_CONTRACT unset until every consumer has vendored), S5, S6, S7. cargo clippy not run.

## Review round (commits 83b72118, 8aaaa717)
- Fixed: Authorization/Proxy-Authorization JSON keys and {name|key,value} pairs masked under both profiles (RULE_VERSION qr-redact/2); fingerprint now hashes outcomes over probe corpora (shapes BEHAVIOUR_PROBES, luhn group_behaviour_probe, json behaviour_probe); new golden negatives (bare letters-only Bearer, 7-digit and 2-digit PAN group) and 3 JSON header cases; CI step runs redaction_perf_budget in release; doc corrected.
- Verified both review mutants (M2 bearer digit clause, M3 group size 3..=7) now fail 3 tests each incl. the drift test; reverted and compared byte for byte.
- Not done: CI step never ran on GitHub; fingerprint covers probe outcomes, not code; {"name":"password","value"} pairs still uncovered; "bare leaf exactly Bearer x" variant deliberately not added (false positive on "Basic infrastructure").
- Finding 2 (applies_to wording) was a commit-message inaccuracy only; corrected in 83b72118's body, docs were already right.
