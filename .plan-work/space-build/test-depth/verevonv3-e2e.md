# verevonv3-e2e (W3 test phase): depth findings

- P1 bug-fixed: audit-role-gate, org-identity-admin and space-policy-admin built browser contexts without a baseURL, so their relative API and goto calls would have thrown; they now use E2E_BASE_URL (default http://localhost:5199).
- P1 bug-found-not-fixed: standing-rule creation cannot succeed (the approval projection lacks agent_subject_id and action_schema_hash, so grant.status is not_recorded); the owner is the Model Plane approval projection (WS10-S6 ResolveApprovalAuthority or an additive projection), and the WS10-S4 flow in space-approvals.spec.ts is test.fixme until it lands.
- P1 bug-found-not-fixed: quarantine never fires (quarantine_mode is parsed but no component reads it, and EXECUTION_CORE_CONTROL_APPROVAL_GATE_TOKEN is not configured); the owner is the Control train plus execution-core, and the WS10-S8 test is test.fixme.
- P1 blocked: the Playwright Chromium binary is not installed, so every browser-launching spec errored at launch before its skip logic ran; `npx playwright install chromium` is a download and needs owner approval.
- P1 test-gap: space-timeline (WS15-S1, WS15-S8) and space-run-live.mock (WS09-S5) follow the client types but are not checked against a live response, and they need a Vite server at E2E_BASE_URL because the config has no webServer for mocked specs.
- P1 test-gap: the WS15-S1 jump-pill check needs features.timeline_anchor, which the gateway does not send yet (WS15-S1 depth), so on a live stack the pill stays dark until that gateway slice lands.
- P2 test-gap: the WS01-S7a viewer restricted-state test needs a fixture thread (E2E_SPACE_VIEWER_RESTRICTED_THREAD_ID) that the seed does not create.
- P2 test-gap: the WS19 create-retry test needs an empty-canvas Space (E2E_SPACE_REF_NO_CANVAS) and assumes the SPA replays a dropped save with the same clientSaveId; neither is verified live.
- P2 pre-existing: real-authority-knowledge.spec.ts throws at collection without REAL_AUTHORITY_FIXTURE_FILE, so `playwright test --list` on the e2e project fails before it lists anything.
- P2 follow-up: nine owed entries (WS01-S1, WS01-S4, WS02-S3, WS06-S1, WS08-S3, WS09-S1, WS13-S2, WS15-S6, WS15-S9) are Rust model-gateway and session-core tests outside this area; not written here.
- P3 follow-up: the WS05-S8 SCIM spec assumes a replayed POST returns the same id and that a revoked token gets 401 or 403; the depth notes mark the SCIM path unverified live.
