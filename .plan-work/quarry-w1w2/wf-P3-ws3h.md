# WS3.H hand-over

Committed: 4e82894b (core contracts + error, runtime challenge.rs, observation wiring, edge construction sites) and ed45220b (openapi ChallengeSignal, SOVEREIGN doc, SDK docs).

Verified: quarry-core contracts 29, quarry-runtime challenge/observation/host_scheduler 69, quarry-edge --lib 341, log_field_discipline 11, cargo check --workspace --all-targets clean for these files, node openapi tests 15.

Left: no live Turnstile/hCaptcha/login evidence (only ContentDriver replay); sdks/generate.sh not run (needs docker generator), SDK docs hand-matched, python SDK not regenerated; scrape path (PageRunner) does not classify yet; WS3.J consumers (Model Plane, gateway, SPA) not started.
Traps: F-t2-034 snippet panic was already fixed in HEAD (3269c377). projection.rs, types.rs, zdr_matrix.rs, zdr-feature-matrix.json* in the tree belong to other units. Main-document status is derived from Network.responseReceived events (challenge::main_document_status); chromiumoxide.rs untouched.

## Review-fix round
Commit de6c83f9. Fixed: widget on thin form page (needs error status/title cue/phrase group/no other fields), "one more step" title doubling, v2+v3 recaptcha masking, Upload/Download refs now gated, tests for same-URL navigate, every AgentAction variant (exhaustive match), driver call counters, log-phase event capture. The "replay of recorded HTML" claim in 4e82894b was false: fixtures are hand-written; SOVEREIGN doc now says so.
Open: no real captured pages; widget-only strong signals still count toward the third strike; scrape path does not classify.
Trap: ref/semantic actions hit the atomic-target capability gate in execute BEFORE the reject check, so a test driver must claim atomic_target_actions (ContentDriver.atomic_targets) to test them.
