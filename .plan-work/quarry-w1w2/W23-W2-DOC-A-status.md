# W2-DOC-A status: ledger and ownership docs, WS16.S0 stale claims, key custody, plan line (lane c, branch w23/lane-c)

## Built / partly

| Scope item | State | Where |
|---|---|---|
| WS7.A ownership: web policy posture paragraph | Built | apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md, "Web Policy Posture" section and two data-boundary rows; the retention section link fixed |
| Key-management responsibility for org-core | Built | same file, "Organisation Key Custody" section |
| Matrix: Auth Core is the only issuer of org web policy; org-core owns org keys | Built | docs/master-ownership-matrix.md, rules 19 and 20 (path note below) |
| Claim edge rows | Built | docs/CROSS_PLANE_CONTRACT_MATRIX.md, "Claim Edges" C1 (quarry-edge) and C2 (browser-broker), plus one pointer sentence |
| QB-T17 prerequisites | Built | apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md: prerequisites paragraph in the QB-T17 section; structured extract and answer synthesis paragraphs state that a granted row does not open `/v1/invoke` for a service |
| WS16.S0 model_plane_gate.rs header | Built, comment only | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/model_plane_gate.rs, `//!` header |
| WS16.S0 json_schema.rs header | Already correct, no edit | apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/json_schema.rs: this tree's header says Quarry uses the validator for structured-extract responses. The phrase "used across Quarry + Model Plane" is absent here; it survives only in the nested worktree vigilant-banach-6933aa, outside this lane |
| Plan section 3.3 sequencing line | Built | docs/plans/quarry-browser-build-plan-2026-10-14.md, Wave 3 sequencing item (3): adds WS2 S3 and WS8.F |
| Model Plane ledger entries (WS7.A, DR-15.1, WS8.A for U03) and the gateway.proto comment | Asks only (Model Plane rule) | .plan-work/quarry-w1w2/MP-ASKS.md, section "W2-DOC-A" (four entries) |
| docs-contract test | Not run (code-only rule) | static review only, below |

Partly built: none. The Model Plane items are asks by rule, not partial work.

## Commits (lane c, local, not pushed)

- `7e9435fc6` docs(control-plane): web policy posture and org key custody in the ownership doc
- `916d35ad3` docs(matrix): rules 19 and 20 for the web policy issuer and org key custody
- `6c4c797ee` docs(contracts): web_policy claim edges in the cross-plane contract matrix
- `1bd801394` docs(quarry): QB-T17 prerequisites and the closed /v1/invoke route in SELF_HOST
- `a66b247ed` docs(quarry-edge): model_plane_gate header says what a grant does not open
- `07e1b67b3` docs(plan): WS2 S3 and WS8.F in the Wave 3 sequencing line for E3.1
- `5ecc734fd` docs(plan-work): W2-DOC-A Model Plane asks for the ledger entries and the gateway.proto comment

## Code only, not compiled

- The only Rust change is a `//!` doc comment in model_plane_gate.rs. Nothing was compiled (owner order: no builds in unit work).
- Not run: `node --test` on the docs-contract test. The scope names `scripts/tests/docs-contract.test.mjs`, which does not exist at the repo root; the file is `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs`. Static review of the assertions that read the edited files:
  - SELF_HOST: the "Quarry → Model Plane" section's names are checked against the code. The new text adds no `QUARRY_EDGE_*` or bare `MODEL_PLANE_*` name. The QB-T17 section is not in that check. The `QUARRY_EDGE__AI_FORMATS*` scan over every doc finds no new name.
  - The `currentDocs()` phrase checks (no "semantic reranker" next to a Model Plane caller): no match added.
  - The compose-default and rerank-default checks read compose and runbook text, which this unit does not edit.
  - model_plane_gate.rs: the check for `Self::AnswerSynthesis => "QUARRY_EDGE__ANSWER_SYNTHESIS"` is untouched.
- `scripts/tests/ownership-docs-contract.test.mjs` (not in the scope list) reads `apps/master-ownership-matrix.md`, which does not exist, so the suite already fails at that read. Statically, the new rules 19 and 20 keep the numbering contiguous, and the edited CONTROL_PLANE_OWNERSHIP.md and matrix keep CRLF with no NUL bytes.

## Flag and default

None. Docs and comments only; no behaviour change. The flags described in the text (`WEB_POLICY_CLAIM_ENABLED` off, `ORG_CORE_KEYS_ENABLED` off) are described, not changed.

## Assumed decisions

1. ADR-1 outcome C is recorded as the recommended default, because the owner's decision list has no separate ruling for it and the built units WS7.H and WS7.I implement it. The Control and Model Plane sign-offs are marked pending. The fail-mode mismatch (design: fail to a static floor after 15 minutes; built: 503 with no token) is written as an open point and not resolved.
2. X-8: org-core owns key custody; the environment master key is the logged interim (decision list item 6).
3. DR-15.1 is written as `proposed`, with G1 and O4 as recommended under X-7. The owner is not named.
4. Matrix location: `docs/master-ownership-matrix.md`. The scope's `apps/master-ownership-matrix.md` does not exist; git history shows the file moved to `docs/`. Rules 19 and 20 are added after rule 18.
5. The retention section's link `../master-ownership-matrix.md` was broken (no such file in `apps/`); it now points at `../../docs/master-ownership-matrix.md`.
6. Plan line: WS2 S3 and WS8.F are added to Wave 3 sequencing item (3), because E3.1 names both. Item (4), E3.2, also needs WS2 S3 in its own dependencies; it is left as is (depth row).
7. Model Plane ledger text uses the newest-entry heading style (`## YYYY-MM-DD — title`, `**State:**`), dated 2026-11-03, state `proposed`.
8. The U03 ledger text is kept as drafted in wf-U03.md. The measurement list is reworded without the file path.
9. The claim-edge rows name `execution-core` and `model-gateway` as the service principals that hold a `quarry` tenant row in `webPolicyByAudience`.
10. The SELF_HOST QB-T17 text keeps the name `DEF-QB-T17-PART2` (the Model Plane ledger's rerank test asserts it) and points it at DR-15.1.

## MP asks

Four entries in `.plan-work/quarry-w1w2/MP-ASKS.md`, section "W2-DOC-A": (1) the WS7.A owner ledger entry; (2) the DR-15.1 ledger entry; (3) the WS8.A ledger entry for U03; (4) the `extracted_json` comment in `gateway.proto`. Nothing under `apps/Model Plane` was edited.

## Not done / not verified

- The docs-contract test was not run (owner rule). The static review above is the only evidence.
- Not edited, outside the scope list: the RUNBOOKS.md Prometheus "Not built" sentences and the GDPR erasure residue list (both addressed to W2-DOC-A in the lane-d depth rows); `docs/CROSS_PLANE_AUTH.md`'s model-plane row; the `model-invoke` notes in `cross-plane-token-requests.json`; the root `CLAUDE.md`, which still names `apps/master-ownership-matrix.md`.
- The Model Plane ledger and the proto are Model Plane files, so they are asks only.
- Nothing is deployed and no live proof was run.
- W23-depth-lane-c.md: five rows committed by b5273c38f (W2-CP-S7b) end in LF, not CRLF. They are left as committed; this unit appended its ten rows in CRLF, append-only.

See `.plan-work/quarry-w1w2/W23-depth-lane-c.md` for the depth rows (W2-DOC-A rows).
