# Wave 5 decision notes

- **Date:** 2026-11-11
- **Unit:** Q5-S6b. Decision notes for Quarry Wave 5 (plan E5.2, E5.3 and ADR-5 / OQ-5 in `docs/plans/quarry-browser-build-plan-2026-10-14.md`).
- **Sources:** `.plan-work/quarry-w1w2/W5-S6-progress.md` (the 077 and 075 sections), `.plan-work/quarry-w1w2/W5-ORCH.md` section 1 (the ADR-5 guard rules), `docs/plans/quarry-w45-code-completion-2026-11-10.md` (units Q5-F1a to Q5-F1d).
- **Scope:** This note changes no code and no switch. It records three decisions and what would reopen each.

| Item | Status | Code or switch |
|---|---|---|
| F-t2-077, the user's own Chrome | Parked | None |
| F-t2-075, deprecation, signed images, usefulness event | Closed: nothing to delete | None |
| ADR-5 amendment, reject-only cookie dismissal (D-W5-4 option a) | Amended, built dark | Q5-F1a to Q5-F1d, off by default |

## 1. F-t2-077: the user's own Chrome (parked)

- **Status:** Parked. Decision note only.
- **Ruling:** Owner, 2026-09-28, section 5 of `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_PAGE_GAP_PLAN_2026-09-28.md`. Hosted, governed browser first. The user's own Chrome and its logins are not driven.
- **Evidence:**
  - `require_isolated_agent_egress` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes/admission.rs:146`, called from `lifecycle.rs:259`) refuses an agent run unless the driver reports both `isolated_egress` and `security_evidence`.
  - The chromiumoxide driver reports both, and only when a pinned egress proxy is set (`apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:5097`). A user's own Chrome is not a Quarry-run driver, so it cannot report that boundary, and the gate refuses it.
- **What reopens it:** all three are needed.
  1. A new owner ruling that replaces the 2026-09-28 row.
  2. A multi-backend browser abstraction (plan E4.5, gated by ADR 10).
  3. A redesigned fail-closed gate that can attest a user-browser backend. The egress check cannot simply be relaxed.

## 2. F-t2-075: deprecation, signed images, usefulness event (nothing to delete)

- **Status:** Closed. Nothing to delete. No code and no flag.
- **Ruling:** Plan E5.2 recommends deleting F-t2-075. Each item it names is either built under plan D5 or has no premise.
- **Evidence:**
  - Deprecation and Sunset headers are built, not deleted. A scrape response that carries unknown-field warnings gets `Deprecation: true`, and `Sunset` when a strict-after date is set (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/scrape_contract.rs:374`, used at `routes.rs:1078`). They are part of plan D5, WS5.D (slice 4). The unit spec's "slice 8" is WS5.H, which covers OpenAPI from types and route-coverage CI.
  - No signed-image premise. A case-insensitive search for "signed image", "image signing", "signed_image" and "sign_image" in `apps/Ingestion Plane/Quarry-v2`, `apps/Frontend Plane/verevonv3/src` and `apps/Frontend Plane/verevonv3/apps/gateway/src` finds no match (checked 2026-11-11). Plan E5.2 places image signing in Support Plane (parked) and the signing keys in Control Plane.
  - No usefulness event exists in the Quarry crates or docs.
- **What reopens it:** Signed images need an owner ruling first, and their signing keys would sit in Control Plane (plan E5.2). A usefulness event needs an owner decision. When built, it carries ids and domains only, never a path, a query or content.

## 3. ADR-5 amendment: reject-only cookie dismissal (D-W5-4 option a)

- **Status:** Amended. Built dark: off by default. Not yet tested or live-proven.
- **Ruling:** D-W5-4 option (a), without the "customise" fallback. The desktop browser may dismiss a cookie banner only by clicking a reject-only control. It never accepts, never clicks a "customise or reject" control, and does nothing unless the organization's policy key `browser.consent.reject` is `allowed`. Every completed dismissal leaves a receipt. The rule "no consent auto-accept" (ADR-5 rule 4) is unchanged. The code completion plan (`docs/plans/quarry-w45-code-completion-2026-11-10.md`, header) took open owner questions at their recommended option, for dark code, under the owner's delegation of 2026-10-23 and 2026-10-27. D-W5-4 was taken that way.
- **Rules as built:**
  - **Off by default.** `startRun({ consentReject })` opts in one run, and only boolean `true` counts (`apps/Ingestion Plane/Quarry-v2/adapters/desktop-browser/index.mjs:486`). The SPA passes `true` only when the org's effective policy says `allowed` for the key. No org, a failed or slow read (5 s), or any other value is off (`apps/Frontend Plane/verevonv3/src/features/core/assistant-agent/desktop-consent-policy.ts`).
  - **Reject labels only.** `consentChoice` (`adapters/desktop-browser/page.mjs:51`) returns a control only when its label matches a fixed list of English and Norwegian reject labels, such as "reject all", "only necessary cookies", "avvis alle" and "bare nødvendige". "Customise" and "manage" controls are never returned. No match means no click.
  - **Receipt.** A completed reject click adds `{ kind: 'consent_reject', host, at }` to the adapter's queue, publishes a `receipt` event, and returns the queue as `receipts` on the next snapshot (`index.mjs:289-290`). It holds the host only, never the path or the query. The queue lives in memory until the next snapshot, and nothing is written to a store, so ZDR holds.
  - **Run trace.** The SPA adds "consent banner rejected on <host>" to the planner history.
- **Evidence:**
  - Commits on branch `quarry-w45`: Q5-F1a and Q5-F1b in `3190cbb13`, Q5-F1c in `1cf692f59`, Q5-F1d in `8184a0eb0`.
  - Code: the adapter (`index.mjs`, `page.mjs`); the desktop app shims (`apps/Frontend Plane/verevonv3/apps/desktop-browser/main.mjs:156`, `preload.cjs:12`); the catalogue key (`apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/web_policy/catalogue.rs:168`: baseline disabled, unlockable, not ZDR-forced, not reserved); the SPA (`assistant-desktop-agent.ts`, `CoreShell.tsx:168`, `BrowserPolicySection.tsx:167`). Operator notes: `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md`, "Cookie-banner opt-out (desktop browser)".
  - Control Plane copies: the org-core and auth-core copies of the catalogue were byte-identical to the Quarry contract at Q5-F1c (sha256 `b6c0a104...`). The contract has since gained `browser.reputation.block` (QF-5, `6ce5bf359`, sha256 `772f02d7...`). The Control Plane copies are one revision behind until QF-6 re-vendors them, and QF-6 is not built yet.
  - Open gap: Quarry's own policy cannot turn the key on. `PolicyLayer::new` drops an "allowed" entry as neutral, and the catalogue baseline (disabled) wins the fold in `resolve()` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-core/src/web_policy/mod.rs`), so every Quarry principal resolves it disabled. The desktop gate reads the org's effective view instead. An unlock path in `resolve()` is needed before Quarry can say "allowed" itself.
  - Rollout: a resolved policy snapshot now carries the key. An edge build that predates the catalogue row refuses such a snapshot with 400 `invalid_policy_snapshot` (`routes.rs:2603`, when the snapshot switch is on). Roll the edge replicas together.
- **What reopens it:**
  - A new owner ruling on D-W5-4. Option (b), removing the auto-dismiss, is the reversal.
  - Any change that widens the rule: a click outside the reject list, an accept click, a customise fallback, a dismissal with no receipt, or the key on by default. Each breaks ADR-5 rule 4 and needs a new ADR before it merges.
  - Adding an unlock path to `resolve()` keeps the default off, so it does not reopen this amendment.
