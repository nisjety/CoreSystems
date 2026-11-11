# Model Plane code completion: handoff (2026-11-10)

This document is the starting point for the next agent. It says what the owner wants, where the build stands, what is
left to code, how the build loop works, and which rules and traps matter. Read it fully before you start.

## 1. The owner's order

Finish **coding** the whole Model Plane, waves 1 to 5 of the Model Plane audit, before any testing or verification.

- Code everything first. No test runs, no Docker image builds, no deploys, no live runs. Testing and verification come
  after the whole Model Plane is coded, as a separate phase.
- Choose the best practice and the optimal solution, not the easiest one. A shortcut that would need rework later is
  the wrong choice. Each unit should leave the Model Plane in a state that is good for future improvement.
- Verevon is sovereignty first. It supports full Zero Data Retention (ZDR) and non-ZDR use. The user (and the org)
  decides which approach they take, and that choice is respected at every level: the SPA, the BFF, the Model Plane
  services, storage, telemetry, audit, background work and every cross-plane call.
- The focus is the Model Plane. Touch other planes only where a Model Plane feature needs a contract or route there,
  and then follow that plane's own patterns and ownership rules.
- Goal of this phase: the audit's coding part is complete. Every audit item is either coded (dark, behind a default-off
  switch) or has a written reason why it is not code (owner decision, external proof, forbidden boundary).

**How "no build" is read here.** The only check allowed is the cheap compile-only type check after each batch
(`cargo check`, `go build` + `go vet`, `tsc6 --noEmit`, `buf generate` for changed protos). It builds no images, runs
nothing, and catches broken code early (it caught a brace lost in merge 292ded7ad). If the owner says the compile
check must go too, drop the fixer stage in `build-v2.js` and say so in the ledger. The generated-artifacts step
(FU-11 / FUGEN) runs its generators once, at the very end; its `cargo test` calls are golden generators, not tests.

Standing owner orders that still apply:
- "Stop asking, just do it" (2026-10-27): inside this approved task, do every implied step without confirmation
  questions. Ask only for something genuinely new: a different service, real money, credentials, production, deleting
  user data, or a change to a frozen contract or an ownership rule.
- Weekly usage stop line: 95% weekly. At the line, finish and commit the running batch, then stop and report the
  status of waves 1 to 5. The 5-hour window is shared with the Quarry session (`quarry-w45`), so check usage before
  every launch.
- Never merge into main, never push, never open a PR, unless the owner says so. Commit locally on `mp-w345`.

## 2. Where it stands (2026-11-10, after batch B09)

"Wave" here means the Model Plane audit waves. Quarry and Space have their own wave numbers; they are different
programmes.

| Wave | Audit scope | Code state |
|---|---|---|
| W1 | WS01-WS12 (16 items) | All slices coded dark and in main. Open: external halves (Z-40 provider attestation, key ceremony, counsel wording, provider probes), live proofs, and some partial rows whose notes name unapplied shared edits (not re-verified). |
| W2 | WS13-WS27 (17 items) | All slices booked, last build batch W2-47 (2026-11-02), in main. **Real code still missing:** WS14-S5 executor input pause / `awaiting_input` / apply-on-resume (blocks EP-4-S7), the WS13-S7 "Always" rule chain (D2-1122..1127). 10 optional-deferred slices were never built. Many partial rows owe depth. |
| W3 | EP-1..EP-14 (23 items) | Coded dark: 105 done / 32 partial / 12 blocked-external / 1 design-needed in the queue, plus 47 mp-w345 units (42 built, 5 partial). |
| W4 | 12 items (F-t3-024, 035, 044-047, 050, 051, 057, 059, 064, 065) | Cells mp-w4a..f in main plus all 45 mp-w345 W4 units committed (41 built, 4 partial: IO-2, CC-2, PL-4, CC-8). Several sub-deliverables deferred by decision (desktop backend C2+, public links, C2PA, API-started Do runs and more). |
| W5 | 30 items (F-t3-069..098) | 24 items have units: 43 of 54 committed (40 built, 3 partial: 079-c finished by the lead, 080-a, 071-b). 11 W5 units not yet built. 6 items have no unit: 072, 082, 084, 085, 088, 089. |

Branch `mp-w345` (worktree `C:/dev/mp-w345`, cut from main 031e7965d) holds the W3-W5 completion batches B01..B09
(134 of 146 inventory units committed). On 2026-11-10 the owner ordered it merged: main was fast-forwarded to
`mp-w345` locally. Nothing is pushed. Keep building on `mp-w345` in `C:/dev/mp-w345` and fast-forward main again only
when the owner asks. Do not edit files in the live checkout `C:/dev/CoresSystem`.
After the merge, main fails some tests that compare against generated files (admission golden, ZDR refusal catalogue
pins, action-registry count, proto bindings) until FUGEN runs. That is expected; do not hand-edit those files.
Truth is git plus `C:/dev/mp-w345-plan/results/*.json`. `queue.json` and `docs/plans/wave3-5-execution-control.md`
are stale.

## 3. What is left to code (in this order)

**A. Finish the queued mp-w345 units.**
1. The 11 W5 units in no batch yet: W5-081-d, 083-b, 092-b, 093-a, 093-b, 095-a, 095-b, 096-a, 097-a, 098-b, 098-c.
2. The follow-up units FU-01..FU-20 (`C:/dev/mp-w345-plan/units/FU-*.json`, written by `make_fu.py`; FU-11 is FUGEN
   and runs last). Read `followups.md` for why each exists. FU-19 (the integration-corev2 route that creates a
   custom_rest connection) touches the area of the FROZEN actions-surface doc that awaits owner review: build the route,
   do not edit the doc again.
3. Plan with `python C:/dev/mp-w345-plan/next.py <name> --dry` first, then without `--dry`.

**B. Inventory and build the W1/W2 code leftovers.** No W1/W2 code-completion inventory exists yet. Run a read-only
inventory (same shape as the W3-W5 one: units with id, wave, source, title, files, spec, size, depends_on, switch) over:
- WS14-S5 (executor input pause, gateway continue branch, `awaiting_input` frame, BFF half; D2-1158, D2-1160), which
  also unblocks EP-4-S7 elicitation.
- WS13-S7 "Always" chain: digest producers (D2-1122), standing-grant digest binding (D2-1123), rule_used and
  rule_expired audit rows (D2-1124/1125), ZDR rule use (D2-1126), proposal list/confirm/dismiss (D2-1127). This also
  unblocks EP-9-E4.
- Partial rows whose notes name unapplied shared edits or uncompiled halves; re-verify each against the tree first:
  WS02-S10.code, WS08-S3.code, WS07-S8, WS02-S11, WS15-S1, WS15-S4, WS17-S8, WS17-S9 (D2-620/626), WS18-S5,
  WS19-S8, WS23-S1.code, WS23-S2, WS23-S3.code, WS23-S4.code, WS23-S6.code, WS24-S4, WS26-S2.code, WS27-S9.
- The 10 optional-deferred W2 slices and WS08-S8: build the ones whose blocking decision is on record; for the rest
  write the decision note (see E).

**C. Fix the open P0 code defects** (depth register `docs/plans/model-plane-depth-register.md`): D2-460, D2-461
(graph grounding without a Space decision), D2-480, D2-481 (content policy bypassed by capability-core workplane APIs
and orchestrator-to-letta-bridge memory writes), D2-509 (skill upsert trusts caller origin), D2-698 (request cap can
loosen the org per-run ceiling, model-gateway `src/budget.rs:66`; the min() fix is owner-visible, so record it),
D2-769 (knowledge list has no Space condition in Space-scoped Do runs), D2-1149 and D2-1178 (waived-run approval
writes not refused), D3-516 (api_key principal shared edits), D3-956 (org skill PATCH/DELETE caller check), D3-243
(public Convex mutations; check the built W3-EP-6-S7-convex unit).

**D. Close the W3-W5 gaps that have no unit yet.**
- W3-EP-1-S5-timeout: SPA/BFF copy for `browser_help_timed_out`.
- W5-069-a: refuse threads holding a `zdr_waived` run before its switch can go on.
- BYO (W4-BYO-1/2/6): persist `model_connection_id` on a turn so replay/regenerate never reaches a platform provider;
  integration-corev2 GET/DELETE for customer-gateway connections; classify `customer-gateway.<id>` as customer-billed
  in cost-core (`pricing.rs` CostBasis); wire AiModelPicker; enforce hidden models beyond listing; read
  `expected_version` in inference-core PUT (409).
- W4-P6 spill: unfiltered workspace upload at lease release when write-through is unset; symlink swap race in
  `create_spill`.
- W4-PL-1 `register_plugin` without source pins (session/capability grpc near line 2214); W4-R2-b timeline entries
  without an action id (needs a Quarry field; coordinate through the Quarry contract).
- ZDR gaps listed in `apps/Model Plane/docs/ZDR.md` with no unit: silent drops of web search and file output under ZDR
  (Z-05a, must become typed refusals), three separate readers of the org posture (Z-03, one shared resolver), ZDR spend
  missing from the ledger (DEF-Z-06b), no retention receipt on threads so a waived append is not checked (Z-17),
  user-subject erasure of request snapshots (Q-ERASE), embeddings/rerank following the org setting (L12, L33: the Model
  Plane must pass the posture; the Data Plane owns embedding).
- Waiver issuer (Z-14 Auth Core decision endpoint and `zdr_waiver` claim with kill switch `ZDR_WAIVER_ENABLED`, and
  Z-19..Z-21b BFF/SPA parts): enforcement exists, the issuer does not. Build it dark if the design on record allows;
  Auth Core is the only issuer.
- Re-check the chat-file hunks merge 6a1ae082e reverted (ComputerViewer mount, EP-4 `!props.spaceScoped`, EP-1
  `onBrowserGiveBack`, EP-3 S4 ArtifactsPanel props, W5-069 rewind lists, W5-078 `feedbackReasonLabels`, W5-098
  ReaderTools, EP-14 S1 settled-fold) against HEAD; restore any that are missing.

**E. Items blocked by a decision, an ADR or an external proof.** For each, write a short decision note with the
recommended option (best practice, sovereignty first) in `docs/plans/model-plane-open-decisions.md`. Then:
- If the recommended option stays inside the standing rules (no frozen contract changed, no ownership rule reversed,
  no new retention of user content without a ZDR clause), build it dark and mark the decision "assumed, owner to
  confirm", as wave 3 did with its 105 decisions.
- If it needs a frozen contract amendment, an ownership-matrix rule reversal, a Channel Plane runtime, real money,
  credentials or an external attestation, do not build it; list it for the owner.
Known items: the goal-judge principal (judge route answers 503 today); thread-bound memory rows under GDPR erasure;
W4-CC-4 Control-signed decision and open point O4; W4-CC-6 extended the FROZEN `docs/actions-surface-operations.md`
(custom_rest section) and needs owner review; W5 items 072 (needs rule 17 reversal), 082 (voice, needs an EU-ZDR STT
design), 084, 085 (thin demand, need EP-9 egress and billing), 088 (deferred to a later wave), 089 (forbidden until a
Channel Plane ADR); W4 deferrals (desktop backend C2+ and viewer per DEC-22/23, continuous clip, public/cross-org
links, C2PA, API-started Do runs, cloud-role BYO); EP-6-P3 unattended named-agent runs; EP-13-S8 Gemini prices and
funded run; W5-093-c `zdr_allowed` values; W5-098-a accent contrast.

**F. Depth register triage.** About 1,135 open follow-up/risk rows look code-flavoured (11 P0, 555 P1). Triage them
into code units, proofs and decisions; build the code units. mp-w345 batches never updated the register, so many D3
rows already covered by units still read "open": close those with the commit id.

**G. Generated artifacts, last and alone (FU-11 / FUGEN).** Admission golden plus vendored copies and `.sha256` pins,
ZDR refusal catalogue plus Go copy plus the digest pin in `scripts/tests/zdr-contract-sync.test.sh`, contract fixture
copies, Go and Python proto bindings for every proto changed since 031e7965d. Never hand-edit generated content.

**H. Docs consistency pass.** ZDR.md section 5.8 and its boundary table, ARCHITECTURE.md and GOAL.md
"implementation truth" sections, the stale wave3-5 board, `.plan-work/execution` queue statuses, ledger entries that
still say "proposed". Then write the coding-complete report per wave and hand over to the verification phase.

## 4. How the build loop works

- Plan folder `C:/dev/mp-w345-plan`: `inventory.json`, `units/<id>.json` (one contract per unit), `slim.json`
  (batches), `results/<batch>.json` (committed batches), `followups.md`, `make_fu.py`, `next.py`, `build-v2.js`,
  `close.py`, `summ.py`, `tscdiff.py`.
- **Plan a batch:** `python next.py <name> [--sizes S,M,L] [--max 20] [--tracks 5] --dry`, then again without `--dry`
  (writes `slim.json[<name>]` and `args-<name>.json`). Done = the id is in any `results/*.json`; a partial counts as done,
  so its gap must become an FU unit.
- **Launch:** `Workflow({scriptPath: "C:/dev/mp-w345-plan/build-v2.js", args: <contents of args-<name>.json>})`.
  Tracks run in parallel, units in a track run in sequence, all writers share the tree (no isolation, Edit only, no git
  writes). S/M units run on Haiku (effort max), L units on Sonnet (effort high); then three Sonnet compile fixers
  (Model Plane Rust; frontend tsc6 + BFF gateway; Go/protos/JSON/YAML/migrations). For units that touch ZDR,
  authorization, credentials, migrations or cross-plane contracts, prefer a stronger model over the cheapest one.
- **New units:** write `units/<id>.json` with the same fields (id, size, wave, depends_on, source, title, files, spec,
  lang, planes, risk, switch) and add tracks to `slim.json` (see `make_fu.py` for the pattern). Specs must name the
  files, the switch, the ZDR behaviour and the exact gap; vague specs produce vague code.
- **After the workflow:** `python summ.py <transcript dir>`; then the lead re-runs the checks itself (fixers have
  wrongly said "untouched" twice):
  - `cd "C:/dev/mp-w345/apps/Frontend Plane/verevonv3" && node node_modules/typescript/bin/tsc6 -p tsconfig.app.json --noEmit > out.txt; python C:/dev/mp-w345-plan/tscdiff.py out.txt` (baseline 33 errors / 16 keys, must show "new 0")
  - `CARGO_TARGET_DIR=C:/dev/mp-target-w345-gw cargo check --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (exit 0)
  - `cd "apps/Model Plane/rust" && CARGO_TARGET_DIR=C:/dev/mp-target-w345 cargo check --workspace --all-targets` (exit 0)
- **Commit:** check `git -C C:/dev/mp-w345 status --short` for stray files, then
  `python close.py <batch> <transcript dir>` (writes `results/<batch>.json`, appends
  `docs/plans/wave3-5-code-completion-2026-11-10.md`, runs `git add -A` and commits). If only the commit failed, re-run
  `git commit -F C:/dev/mp-w345-plan/msg.txt`, never close.py twice.
- **Pace:** a 20-unit S/M batch costs about 25-55% of a 5-hour window and 5-12% weekly; all-Sonnet cost 94%. Never run
  two batches in the shared tree at once. Session-only crons die when a session restarts.

## 5. Coding rules: sovereignty, ZDR and the user's choice

1. **ZDR is a monotonic floor.** effective_zdr = issuer claim OR request flag. Code may raise it, never lower it. A
   client's `zdr:false` changes nothing. Every site calls the one shared resolver (model-gateway `zdr_posture.rs`,
   mp-contracts `retention.rs`).
2. **Fail closed, typed.** No attested provider means a ZDR turn is refused before any network call
   (`zdr_unavailable`). An unresolvable posture returns 503 and issues nothing. Attestation is evidence fields plus a
   digest set by an operator; never a boolean in code.
3. **Refusals live in the catalogue** (`mp-contracts/src/zdr_refusal.rs`), never inline, with the posture-matrix row
   and the durable-mutation inventory case. The SPA renders them by code in Norwegian and English.
4. **Every new store answers four questions:** a ZDR run writes nothing (refuse or skip before any state change); a
   waived run counts as ZDR for new stores (DEC-32); rows are erased with their thread and org (cascade plus the GDPR
   purge); no content in logs, audit rows or metrics. Add the row to the ZDR.md boundary table.
5. **Waivers** are per run, bound to one connection, issued only by Auth Core, acknowledged by the member, never
   org-wide, never for Temporary chats, Space ZDR, scheduled, unattended, browser, speech or background work.
6. **The user and the org decide; a plane enforces.** Policy is stored in Control (org-core) and enforced by the Model
   Plane service in front of the provider (ADR-0005 pattern: inference-core enforces, model-gateway asks, fail
   closed). Overrides only narrow. Agent modes: strictest of platform floor, org cap and user choice wins. Approvals
   carry closed cause codes and provenance. The model may propose but never grant its own permission.
7. **Sovereignty and residency.** Privacy tiers UNSPECIFIED < GLOBAL < EU_RESIDENT < ZDR_CONTRACTUAL < SOVEREIGN are
   enforced in inference-core before any network call; ineligible providers are skipped; an empty remainder is a typed
   refusal, never a silent downgrade. A sovereign turn also retrieves and embeds sovereignly. Background work follows
   the tenant posture and never uses a user's subscription or BYO connection.
8. **BYO and credentials.** Keys and tokens live only in integration-corev2 custody; the Model Plane sees an opaque id.
   A BYO route is the lowest privacy tier, never ZDR, never a fallback target, billed to the customer.
9. **Audit rows carry no content:** ids, closed codes, outcomes, counts. Never secrets, prompts, arguments, paths or
   reason text. ZDR and waived runs write no approval or audit content rows.
10. **Switches:** every new behaviour is dark behind a default-off switch read as the strict literal `true`. A new
    Model Plane switch goes into `apps/Model Plane/deploy/docker-compose.yml` (default off) and
    `apps/Control Plane/flag-registry/flag-registry.json` in the same change. SPA flags are `VITE_*`, typed in
    `src/vite-env.d.ts`. BFF flags live on AppState, never `std::env::var` in a handler.
11. **Plane boundaries** (CLAUDE.md): Control owns identity, orgs, billing, entitlements and the retention posture;
    Data Plane owns documents, embeddings and retrieval (API only); provider actions only through integration-corev2's
    FROZEN actions surface; the model proposes browser actions and Quarry executes; no database crossing between
    planes; Channel Plane is docs-only.
12. **Contracts are additive.** Proto fields only added, with a doc comment; check the next free tag and migration
    number right before use (session-core reached 0120, capability-core 0041, integration-corev2 0015); action
    contracts in `src/shared/actions` before UI wiring.

## 6. Traps

- Never run `cargo fmt --all` (the tree is not fmt-clean). Use `rustfmt --check --edition 2021 <file>` only.
- Preserve each file's line endings (`git ls-files --eol`); new files LF. Long Bash heredocs fail: write a script, then
  run it. Python `subprocess.run(["bash", ...])` hits the WSL stub.
- The commit hook runs gitleaks in Docker; with the engine down it falsely reports a secret. Recover with
  `pwsh -NoProfile -File "apps/Model Plane/scripts/dev/docker-desktop-recover.ps1" -Apply`; never `--no-verify`.
- Watch C: free space after each batch; agent cargo targets have filled it before. Use only the targets
  `C:/dev/mp-target-w345`, `-gw` and `-other`.
- Never print, grep or cat `.env*` files. The owner's Anthropic test key lives only outside the repos and is for the
  later live phase.
- Known pre-existing failures (not ours): tsc6 baseline 33 errors; Quarry-v2 `quarry-evals` E0063 (`agent_cursor`);
  two Model Plane targets fail on Windows (use `--lib` when testing later).
- When a cargo result contradicts the tree, the warm target is stale: `cargo clean -p <members>`.
- Workflow `isolation: "worktree"` cuts from the default branch `fix/ui-premium-restore`, not `mp-w345`.

## 7. Reading order for full context

1. This file.
2. Memory: `C:/Users/ImaFernandesDaCosta/.claude/projects/C--dev-CoresSystem/memory/MEMORY.md`, then
   `mp-w345-code-completion.md`, `model-plane-wave-strategy.md`, `owner-says-stop-asking.md`,
   `weekly-usage-stop-at-70.md`, `build-strategy-wave-first.md`.
3. The audit: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` (98 items F-t3-001..098;
   Appendix B at line 1813 is the index).
4. The plan: `C:/dev/mp-w345/docs/plans/model-plane-chat-build-plan-2026-10-14.md` (WS01-WS27, EP-1..EP-14, W5 backlog
   3.3.4, DEC-01..40, Appendix B amendments win).
5. Decisions: `apps/Model Plane/docs/decisions/ledger.md`, `docs/plans/wave3/OWNER-DECISIONS.md`,
   `docs/plans/model-plane-wave1-owner-decisions-2026-10-28.md`.
6. Contracts and principles: `apps/Model Plane/docs/ZDR.md`, `docs/master-ownership-matrix.md` (rules 4, 12, 16-20),
   `apps/MODEL_ENTITLEMENTS_ADR_2026-10-15.md`, `docs/architecture/adr-provider-side-orchestration-privacy-tiers.md`,
   `apps/Model Plane/docs/capability-ownership-matrix.md`, `apps/Model Plane/api/contracts/README.md`,
   `apps/Control Plane/flag-registry/README.md`, `apps/Model Plane/docs/runbooks/migration-policy.md`.
7. Build records: `docs/plans/wave3-5-code-completion-2026-11-10.md` (large; grep it), `C:/dev/mp-w345-plan/followups.md`,
   `results/*.json`, `docs/plans/model-plane-depth-register.md`.

## 8. Prompt to start the next agent

```text
You are continuing the Verevon / CoreSystem Model Plane build. Your job is to finish CODING the whole Model Plane
to the audit, waves 1 to 5, before anyone tests or verifies it.

Get full context first, and read rather than skim:
1. C:/dev/mp-w345/docs/plans/model-plane-code-completion-handoff-2026-11-10.md: the owner's order, status per wave,
   what is left (section 3, A to H), the build loop, coding rules, traps and reading order.
2. Memory: C:/Users/ImaFernandesDaCosta/.claude/projects/C--dev-CoresSystem/memory/MEMORY.md, then
   mp-w345-code-completion.md and the notes it links.
3. The audit C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md (98 items, Appendix B is the
   index) and the plan C:/dev/mp-w345/docs/plans/model-plane-chat-build-plan-2026-10-14.md.
4. C:/dev/mp-w345-plan/followups.md, results/*.json and units/FU-*.json.
Git log and results/*.json are the truth; queue.json and the wave3-5 execution board are stale.

What we are building towards: Verevon is a sovereignty-first AI workspace. It supports full Zero Data Retention and
non-ZDR use side by side, and the user (and their org) chooses the approach. That choice is respected and enforced
at every level: the SPA, the BFF, every Model Plane service, storage, telemetry, audit, background work and every
cross-plane call. It can be tightened, never silently loosened. ZDR is a monotonic floor, refusals are typed and fail
closed, sovereign and EU tiers are enforced before any network call, audit rows carry no content, and keys stay in
integration-corev2 custody.

How to work:
- Code only. No tests, no Docker builds, no deploys, no live runs until the whole Model Plane is coded. The only
  check is the compile-only type check after each batch (cargo check, go build and vet, tsc6 --noEmit), which keeps
  the tree compiling.
- Choose the best practice and the optimal solution, not the easiest. Pick the design that leaves the Model Plane in
  the best shape for future improvement; if a shortcut would need rework later, do not take it. Name the ADR or DEC
  basis for each choice.
- Everything new is dark behind a default-off switch (strict literal "true"), registered in the compose file and the
  flag registry in the same change.
- Focus on the Model Plane. Touch other planes only for the contracts a Model Plane feature needs, and follow their
  ownership rules.
- Work on branch mp-w345 in C:/dev/mp-w345 with the batch loop in C:/dev/mp-w345-plan (next.py, then Workflow with
  build-v2.js, then the compile checks, then close.py). Never run two batches at once. Commit locally; no push, no PR;
  fast-forward main only when the owner asks.
- Use workflows: inventory and design workflows to find the work, build batches to write it, and an adversarial
  review for risky units (ZDR, authorization, credentials, migrations, cross-plane contracts).
- Owner standing orders: do not ask for confirmation inside this task; stop at 95% weekly usage (finish and commit
  the running batch, then report wave 1 to 5 status); check usage before every launch because the Quarry session
  shares the window; never run cargo fmt --all; never print .env files or the owner's test key.

Order of work (details in handoff section 3):
A. Build the 11 remaining W5 units and FU-01..FU-20 (FU-11 last).
B. Inventory and build the W1/W2 code leftovers (WS14-S5 input pause, the WS13-S7 "Always" chain, partial rows with
   unapplied shared edits).
C. Fix the 12 open P0 code defects from the depth register.
D. Close the W3-W5 gaps that have no unit (BYO replay and billing, the ZDR gaps in ZDR.md, the waiver issuer, the
   spill race, chat-file hunks lost in merge 6a1ae082e).
E. For items blocked by decisions, write docs/plans/model-plane-open-decisions.md with a recommended option for each;
   build dark where the recommendation stays inside the standing rules; list the rest for the owner. Never change a
   FROZEN contract or an ownership rule, and never build against the Channel Plane.
F. Triage the depth register's open code rows into units and build them.
G. FUGEN last and alone (generated artifacts, never hand-edited).
H. Docs consistency pass, then a coding-complete report per wave and per audit item, and a plan for the
   verification phase.

Done means: every F-t3 audit item is coded dark or has a written reason why it is not code; the tree compiles; the
ledger, the handoff and memory are current.
```

## 9. Status at the end of the code phase (2026-11-12)

The code phase is finished. Every unit adopted from the handoff plan, the round-2 design scopes (ws13, ws14, zdr2, r2)
and the lead's follow-ups is built dark and compile-checked; nothing is tested or live-proven.

- Batches after this handoff was written: B10 349abd38c, B11s 8c43757d4, B11t 2db962669, B11 4b0cf580a,
  B12 5d5096414, B13 63f62cb0f, B14 1c5394660, B15 0120a8e84, B16 d5f89aab0, B17 211b2a404, B18 68e247254 (FU-02,
  FU-11 FUGEN, R2-OD-40-c). Docs: 14d0d1f96, ec555bc79, 8b8cb1e47, 6e7d1bc83, eeb23dea8 and the closing docs commit.
- Read next, in this order: `docs/plans/model-plane-coding-complete-2026-11-11.md` (status per wave, per audit item
  and per depth row, switches, what is not built), `docs/plans/model-plane-open-decisions.md` (section 3.6 holds the
  owner's answers of 2026-11-11 to all triage decisions), `docs/plans/model-plane-verification-plan-2026-11-11.md`
  (the order of work for the verification phase).
- Switches that must stay off until their follow-up is deployed and proven: `EXECUTION_BROWSER_HUMAN_ASSIST`
  (FU-25, built; deploy session-core first), `MODEL_GATEWAY_ZDR_LEDGER_ROWS` (FU-30, built; the owner confirms
  cost_basis in the ZDR spend allowlist and ZDR spend rows in the billing outbox).
- Not built in this phase: 194 depth-register triage units (written, reviewed by the merge pass, each carrying the
  owner's answer where one applies), W3-EP-4-S7 MCP elicitation (unblocked by the owner's DEC-F08-05), and the owner
  answers that need new work (verification plan section 7, O-01 to O-28).
- Usage at the end: about 88% of the weekly limit. The build loop, its tools (plan_next.py, mkbatch.py, guard.py,
  claimcheck.py, leadcheck.sh, regcheck.py, close.py, mk_report.py, mk_decisions_doc.py) and the unit specs stay in
  the programme plan folder `C:/dev/mp-w345-plan`.
