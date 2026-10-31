# W3-B2-04 progress (E3.1-f, ADR-2 credential-handoff decision memo)

Status: document built and committed. No code, no Control Plane edit, no plan edit. The 2a/2b split is NOT ratified.

## Built

- `.plan-work/quarry-w1w2/W3-E3.1-f-memo.md`: options for Quarry to IC identity (A shared key rejected; B per-caller static key as fallback; C auth-core plane token for audience `ingestion` with scope `integration:otp:read`, recommended; D Model Plane relay rejected), per-use approval rule, the 2a/2b split as a proposal, the R-13 registry row (JSON snippet 6.A) and the manifest row (JSON snippet 6.B), a ledger pointer for the plan writer, and the decisions requested from the user.
- `.plan-work/quarry-w1w2/depth/W3-B2-04.md`: 10 depth bullets.
- `.plan-work/quarry-w1w2/wf-W3-B2-04.md`: hand-over notes.

## Verified (what was run)

- Read the epic (plan 3160-3180), ADR-2 (plan 367, 501), R-13 (plan 527), the estimate and batch plan (W3-estimate.md), the audit F-t2-024 (audit 701-740), A2a-credentials-design.md.
- Grepped and read code at HEAD: grant.go allowed sensitive actions (101-105); quarry-runtime service_tokens.rs ServiceTokenAudience (40-46, 68-72, 210-233); quarry-edge agent_routes/ split (sensitive_approval.rs:12, admission.rs:415); IC internal/api/server.go auth wiring (about 170-212); IC internal/auth/middleware.go InternalOnly (145), InternalOrBearer (167); IC config.go AllowLegacyTenantKey (280), IngestionAuthAudience (288); IC controlplane/clients.go VerifyToken audience check (106-111, 219); auth-core convex-token.service.ts audience `ingestion` (93, 371-374); plane-service-principals.json quarry-edge row (380-410); IC actions directory (no `otp` match).
- Both JSON snippets in the memo parse (node JSON.parse, 2 of 2 blocks OK).
- Not run: no tests, no cargo, no go, no docker. Document-only unit; per the build-first rule, the only compile-check-like step was the JSON parse.

## Uncommitted files of this unit

None after the commit below.

## Not done

- Ratification of the 2a/2b split and of identity option C: needs the user in chat. E3.1-g and E3.1-h stay blocked.
- R-13 PR 1 rows (section 6.A, 6.B): Control Plane owner; not edited here.
- Plan ledger text (memo section 8): text only; the plan writer applies it.
- RUNBOOKS.md section: not written. This unit is a decision memo, and the memo is the slice's own doc.

## Next

1. The user ratifies or rejects 2a (identity option, per-use approval, retention `zdr`).
2. Control Plane owner signs R-13 PR 1 rows for audience `ingestion`.
3. E3.1-g first slice verifies the mint route for `ingestion`, IC per-operation scope enforcement, and the flag name.

Depth bullets: .plan-work/quarry-w1w2/depth/W3-B2-04.md
Hand-over: .plan-work/quarry-w1w2/wf-W3-B2-04.md
