## WS15-S1
shared-edit apps/Model Plane/api/contracts/sse-event-payloads.json: from apps/Model Plane/rust run `MP_UPDATE_GOLDEN=1 CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0 cargo test -q -p model-gateway --lib sse_event_payload_golden`, then re-run without MP_UPDATE_GOLDEN to confirm it passes. Expected diff: exactly three added events, retry_scheduled (family steps), compaction and context_change (control, no family); the existing 21 entries are unchanged (verified). Then refresh the golden's .sha256 pin, the SPA vendored copy, and the pinned digest in scripts/tests/zdr-contract-sync.test.sh, and run `bash scripts/tests/zdr-contract-sync.test.sh` from the repository root. The golden is LF; keep it LF.

## WS15-S1
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/ag-ui-client.ts: in toAgUiEvent, before `    case 'unknown':` (line 193, file is LF) add:
    // Lifecycle cues are Verevon's own projection; AG-UI names none for them yet.
    case 'lifecycle.retry':
    case 'lifecycle.compaction':
    case 'lifecycle.context':
      return null
This clears TS2366 at ag-ui-client.ts(126,53). Not verified by me, since the file is shared; re-run `pnpm exec tsc -p tsconfig.app.json --noEmit` afterwards and expect only the 33 pre-existing quarry-client.ts and RemoteSupportPage.tsx errors.

## WS23-S1.code
shared-edit apps/Model Plane/rust/crates/mp-content-safety/src/trust.rs: in TOOL_TRUST after the browser_agent row add `("read_provider_action", TrustClass::ConnectorContent),` and `("execute_provider_action", TrustClass::ConnectorContent),`; in the_untrusted_tools_classify_as_external add asserts for both names; change the ConnectorContent doc from 'No tool maps to it yet.' to 'Content read from, or acted on in, a connected account.' Then rerun: cargo test -q -p execution-core --lib -- connector_result_is_framed ; cargo test -q -p execution-core --test compaction_parity_contract ; cargo test -q -p mp-content-safety

## WS23-S1.code
shared-edit scripts/tests/zdr-contract-sync.test.sh: in the GOLDENS row for apps/Model Plane/api/contracts/zdr-refusals.json replace digest 9c2b5458aac4b2b779ab9b63acb987f63df6c41b85a93a32ccfcaf97251b6d1e with 15b12d17d34c49270889ce7a3b5fc66471bce3646499888ef7e7ee2cc1178cf1

