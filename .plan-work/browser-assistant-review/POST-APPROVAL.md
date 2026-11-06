# Post-approval evidence + rollout runbook — agent-first browser

**Approval:** User `approve` this turn. This doc records what was executed after approval (verified by running) vs what remains ops-side. No push, no PR. Branch `feat/browser-assistant-slices` @ `3425e9447` (test fix on top of `487627f60`).

## Executed after approval (verified by running)

- **3-test triage fix (test-helper only):** `ai_run_upstreams` mocks org-core `GET /api/v1/organizations/*` → `zdr:false`, routes `state.org_core_url` to the quarry mock server. Session `zdr=true` still wins via `metadata.zdr()`. Verified: `domains::browser` **221 passed, 0 failed** (was 218/3). Committed locally as `3425e9447` with hooks (no `--no-verify`, no push).
- **Compose validation:** `docker compose config` on both compose files parses (fails only on missing required secrets — by design, no change-me defaults). Our added flags render as `false` by default. Verified by running `docker compose version` (v5.4.0) + `config` (expected missing-secret errors, no YAML syntax errors). Live `up` NOT started (requires real secrets + infra).
- **Merge dry-run:** detached temp worktree at `3bb5c96db` + `git merge --no-commit --no-ff feat/browser-assistant-slices` → **Automatic merge went well, no conflicts** (33 files). Temp worktree removed. Main checkout NOT merged (it is dirty with others' uncommitted work; plan requires merge from a clean checkout — a human must clean/commit, then merge).

## D1–D8 status after approval

User approval is recorded as **waiver to proceed with waived scope staying flagged-off**, not as cross-plane ratification. ADR `adr-agent-browser-phase0-decisions.md` §0.1–0.8 remains **proposed**. Below-the-line builds (broker/Quarry gate, `fill_secret`, planner tuning, audit publisher, attachment contract, server log, step-picker UI, cross-plane session store) stay **blocked** until plane owners ratify. No cross-plane semantics changed this turn.

## SC1–SC7 status

- **SC1 compose smoke (stack):** NOT run live (no secrets/infra here). Compose files parse; unit suites green.
- **SC2 ZDR refusal:** Unit green (incl. new `assistant_task` refusal test + fall-closed behaviour). Live 409 NOT run.
- **SC3 navigation persistence:** Unit green (2 + 13). Live run round-trip NOT run.
- **SC4 bookmarks purge:** Unit green. Live purge NOT run.
- **SC5 PDF content-type:** Unit green (incl. `%PDF-` cases). Live fetch NOT run.
- **SC6 live view + shell:** Unit green (`BrowserChrome.reload` 7 ✓). Live render NOT run.
- **SC7 flag-off parity:** Unit green (`domains::browser` now 221 ✓, heading test 7 ✓). Live flag-off parity NOT run.
- Stack portion requires a live stack with real org-core/Quarry/model-gateway — **still pending ops run** (commands below).

## Runtime rollout runbook — one flag at a time (ops env, not repo defaults)

Repo defaults stay **off** (`ASSISTANT_NAVIGATION_ENABLED=false`, `MODEL_GATEWAY_ASSISTANT_NAVIGATION=false`, all `VITE_*` unset). Rollout flips **env at deploy time**, one at a time, re-running the affected SC gate after each flip. Never bulk-flip. Rollback per flag: set the flag back to `false`, redeploy the affected plane, confirm old behaviour.

1. `MODEL_GATEWAY_ASSISTANT_NAVIGATION=true` → redeploy Model Plane → re-run SC3 (flag-off refused / flag-on run persists).
2. `ASSISTANT_NAVIGATION_ENABLED=true` → redeploy Verevon gateway → re-run SC2/SC3 (gateway kill-switch).
3. Then each `VITE_*` flag one at a time (build the SPA with that var set, redeploy web, re-run SC6):
   `VITE_ASSISTANT_DOCK_POSITION`, `VITE_BROWSER_TAB_MENU`, `VITE_BROWSER_BOOKMARKS` (SC4), `VITE_BROWSER_ANNOTATE`, `VITE_BROWSER_ZOOM`, `VITE_BROWSER_KEYMAP`, `VITE_BROWSER_NEWTAB`, `VITE_WORKSPACE_SPLIT`, `VITE_ASSISTANT_FLOATING`, `VITE_ASSISTANT_REDACT`, `VITE_BROWSER_ELEMENT_FILTER`.
4. Never enable `VITE_AGENT_BROWSER_WORKSPACE` / `VITE_AGENT_BROWSER_LIVE_WS` — those declarations were removed (no implementation); re-add only with code + D8 ratification.

**Merge command (after main checkout is clean, human-run):**
`git merge --no-ff feat/browser-assistant-slices` from a clean `C:\dev\CoresSystem` on `main`, then `git push` only if the team chooses to push (this workstream: local only).
