# Execution state (lead keeps this current)

Batch results are appended to `progress.md`. Queue statuses in queue.json are updated by the lead after each batch commit.

## B01 (2026-10-21)
- WS01-S6: done (code and tests done; ZDR live proof waits for Z-40/Z-60)
- WS02-S5: done (code done; rebuilt-image flagged 3-turn run is a lead step)
- WS12-S3: done (code done; locale screenshots on a second dev server pending)
- WS11-S1.code: done (code done; N1 cache proof after WS11-S5)
- WS09-S1: partial (measurement code done; three-run baseline on a second dev server still to record in chat-perf-budget.json; the temporary-thread first-send local write defect is pinned and needs a fix)

## B02 (2026-10-21)
- WS02-S6: done (committed bc4cd7b2; rebuilt-image proof with MODEL_GATEWAY_RUN_CONTEXT on is a lead step)
- WS05-S0: done (committed 2aaf2e02)
- WS14-S3: done (committed defaa28b (+24ac2a20 sse.rs fix); live create/list/answer proof is a lead step)
- WS12-S6: done (committed 283af93e)
- WS12-S1: partial (committed cbdff1b5; recorded runs on the integration branch need a push (blocked external))

## B03 (2026-10-21)
- WS02-S7: done (committed 0d64d9b9; rebuilt-executor run with MODEL_GATEWAY_RUN_CONTEXT=full is a lead step)

## B03 (2026-10-21)
- WS11-S2.code: done (committed fd7c0433; live proof waits for WS11-S5 and a route)
- WS11-S3.code: done (committed 4b1da1f3; probe P0(iii) decides the default)
- WS12-S4: done (committed 8e88560e; CI run needs a push)
- WS04-S1: partial (committed b0cb039a; /readyz run_control and the live NATS tests (5) not done)

## B04 (2026-10-21)
- WS09-S2: done (committed (see git log); Playwright before/after on a rebuilt frontend and a manual streamed-answer check are lead steps)
- WS13-S0: done (committed; ADR-0011 not written; ledger/matrix lines pending)
- WS11-S4.code: done (committed; not proven live)

## B04 (2026-10-21)
- WS05-S1: done (committed 2c877270; capability-core rebuild and capability-health-proof are lead steps)
- WS3J-MP: partial (committed e336de32; live proof owed (third attempt refused by Quarry with enforce))

## B05 (2026-10-22)
- WS13-S1: done (committed 709785fa; live pause on rebuilt images pending)
- WS11-S5.code: done (committed 5430397f; live proof pending)
- WS09-S3: done (committed 7f1a4612; plus ZDR first-send fix 60748925 and refusal naming 868a23c9)

## B05 (2026-10-22)
- WS05-S2: partial (committed 9d299857; live fixture proof and openapi entries and ZDR golden follow-up pending;WS03-S9=committed f8f1bfd3; static and DB stages green; live runner built but not executed)

## B05 (2026-10-22)
- WS03-S9: partial (committed f8f1bfd3; static and DB stages green; live runner built but not executed)

## B06 (2026-10-22)
- WS13-S2: done (committed 771f2f60; live card check and SDK regeneration pending)
- WS12-S5: partial (committed 1b752d45; pins remain for strings in files outside the slice)

## B06 (2026-10-22)
- WS05-S3: done (committed (see git log) after an Opus review and fix round; rebuilt stack with the gate in shadow pending)

## B06 (2026-10-22)
- WS02-S3: done (committed 7b0b7d62 after an Opus review and fix round; live ledger row pending)
- WS27-S0: done (committed 49bda016)

## B07 (2026-10-23)
- WS14-S1: done (committed feca82e4; screenshots and keyboard pass and reload check pending (need a second dev server);WS11-S6=committed ef4c69ce; live select check pending (needs the effort ladder on))

## B07 (2026-10-23)
- WS14-S1: done (committed feca82e4; screenshots and keyboard pass and reload check pending (need a second dev server))

## B07 (2026-10-23)
- WS11-S6: done (committed ef4c69ce; live select check pending (needs the effort ladder on))

## B07 (2026-10-23)
- WS27-S1: done (committed (see git log); callers of cost_usd_for now show no cost for unlisted models)

## B07 (2026-10-23)
- WS05-S4: done (committed 730a0d1d after two review rounds; live proof pending)

## B07 (2026-10-23)
- WS04-S4: done (committed 8930af52 after two review rounds; migration 0050 never run; P0 open in the register)

## W1-02 (2026-10-23)
- WS04-S1: done (committed b0cb039a; the /readyz run_control line lands with WS04-S2 (http_routes.rs); live NATS tests are depth)

## W2-44 (2026-11-01)
- WS13-S7: partial (rebuilt in W2-44 (e833e5647 + 9e48b2d8c); depth phase: digest producers in agent.rs/grpc.rs (D2-1122)/the proposal list-confirm-dismiss chain (D2-1127)/rule_used and rule_expired rows/digest-bound grants/ZDR rule use)
- WS27-S7: partial (built (4b6b062c9 + 9e48b2d8c); depth phase: live proof with INFERENCE_ROUTE_NOTE and MODEL_GATEWAY_USAGE_PROGRESS on)

## W2-46 (2026-11-01)
- WS14-S5: partial (contract and spend built (e94252c1b + ed218ccda + the W2-46 shared commit); depth phase: the executor input pause and apply on resume (agent.rs/resume.rs/mod.rs) / the gateway continue branch and awaiting_input frame / the BFF and SPA answer path / then create_run_question accepts delivery resume)

## W2-47 (2026-11-02)
- WS14-S2: partial (finished in source (d7eba9ba1); depth phase: invoke-level execute-turn tests / e2e spec / ZDR goldens / chat.reject_plan / live proof)
- WS14-S7: partial (docs and verify-hitl.sh (7969b3c97 + 04909cf58); depth phase: the .env.example block (owner) and the proof-stack run)

## W3-04 (2026-11-03)
- EP-6-S0: done (decision records applied by the lead (W3-04 lead edits commit))

## W3-04 (2026-11-03)
- EP-13-S4: partial (committed f2b155d5f + lead fe4b53b03 (gRPC handlers wired; compose switch; regen); owed: a gRPC-level verevon-role test and the live proof (design 4 S4))
