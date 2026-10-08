# Recording readiness: Terra route, broker hardening and the pre-freeze gate

**Release remains blocked. Nothing in this record is deployed.** This record follows [pass 6](PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md). It reconstructs where the Pass 6 time went, records the product owner's route and latency decisions, and lists the broker and gateway changes built since then. The changes exist only in the working tree of `fix/ui-premium-restore`. The last measured build is still the Pass 6 gateway image `sha256:d5c2ceba6f568d1c018984032cda28cd4bed0a77668830ea0f3883bce91ed9c6`. The stack must be rebuilt through the launcher and validated live before a build freeze. Qualification streaks (Q16) start only after that freeze. All four scenarios remain at **0/5** qualified release runs, and all 20 public media fields remain null. The manifest gates are unchanged. No live run, post-change latency or qualification result is claimed here. Every post-change number below is marked as an **estimate**.

**Date:** 2026-09-24

**Decision owners:** Product owner for the route, the quality targets and any permanent deletion; Model Plane and Integration Core (Ingestion Plane) for the changes below; Frontend Plane for the recording harness. Existing plane authority remains unchanged.

## Product-owner decisions

| Date | Decision | Consequence |
| --- | --- | --- |
| 2026-09-23 | Keep **`gpt-5.6-terra` via `openai-codex-subscription`** as the product and recording route. No model switch. | Latency work stays inside the route. The Q14 plan target must be restated (see below). |
| 2026-09-23 | Fix Voice mode on the governed speech path; do not remove it. | Voice mode, voice input and “Les høyt” are not part of the four recorded journeys. The dictation clean-up call defaults to `gpt-4o-mini` (model-gateway `http_routes.rs`), so a recorded take must not use voice input. |
| 2026-09-24 | The broker's direct ChatGPT-OAuth Responses transport is built but **opt-in**. The hardened Codex app-server stays the default until OpenAI confirms hosted multi-user use. | OpenAI's public statements cover individuals using their own account in a client. A 2026-08-21 statement from OpenAI's Codex team said re-serving a subscription as API traffic is not supported and gets flagged. Recording and qualification runs use the app-server. |
| 2026-09-24 | The four model-visible latency levers ship **on**, each behind an env kill-switch: `MODEL_GATEWAY_SOLE_NOTE_PREFETCH`, `MODEL_GATEWAY_RECHECK_PROPOSES_EDITS`, `MODEL_GATEWAY_BALANCED_REVIEW_BATCHES`, `CODEX_APP_SERVER_THREAD_EFFORT`. | They change what the model is sent, so the 104/104 matrix and 10/10 recheck controls must be re-established on the rebuilt build. |
| 2026-09-24 | Subscription tokens are excluded from org token caps. | This restores the earlier behaviour. The subscription route used to ledger 0 tokens; it now records real counts at cost 0. `/budget/check` in cost-core now compares only metered tokens with `max_tokens`. The older question of whether `max_tokens_per_run` should compare against an all-time total predates this work and is not decided here. |

## Pass 6 phase timing (measured)

Reconstructed from the retained gateway and inference-core logs and the private v53–v57 raw reports: 10 successful turns on the final Pass 6 image. No code change was needed to measure it.

- **Provider wait was 98.8–99.7% of server time.** Orchestration took 0.16–0.51 s per turn. Browser, BFF and harness overhead was 0.34–0.73 s. “Provider” means the whole broker path, including per-call Codex setup.
- The time goes to two provider calls in sequence. First the **author call** writes the whole document inside the `create_artifact` arguments (19.9–25.7 s for plans, low effort). Then the **high-effort source review** checks it. Review took 54–77% of plan time (23.3–89.5 s).
- **Each rejected review added roughly 25–31 s**: another complete review pass, plus a 7.9 s author repair on v54.
- **v54 used 89.5 of the 90 s artifact deadline** and passed with about 0.45 s to spare. The earlier v34/v40/v45/v48 timeouts fit the same pattern.
- The fastest plan (v55) already spent 43.2 s in provider calls: author 19.9 s plus review 23.3 s.
- Two of five note turns spent an extra 7.4 s and 8.7 s on a model round that only decided to read the plan.
- First reviews accepted 3/5 plans; rechecks accepted 2/3; note reviews accepted 5/5. Every rejection was a reviewer “unsupported” verdict. There were no local-check or protocol rejections.

| Take | Plan, browser (s) | Author (s) | Source review passes (s) | Note, browser (s) |
| --- | --- | --- | --- | --- |
| v53 | 60.29 | 24.29 | 34.82 accepted (2 batches) | 45.46 (incl. 7.39 read round) |
| v54 | 115.96 | 25.72 | 30.80 rejected, 25.37 rejected, 25.42 accepted; repair 7.88 | 29.28 |
| v55 | 44.04 | 19.92 | 23.30 accepted (3 batches) | 29.58 |
| v56 | 78.67 | 21.18 | 31.07 rejected (reviewer edits applied), 25.70 accepted | 36.48 (incl. 8.70 read round) |
| v57 | 52.55 | 22.72 | 29.06 accepted | 35.31 |

Limits: the retained window held no failed turn, so the timeouts cannot be broken down. Browser and container clocks differ. The persisted `review_ms` covers only the last accepted pass (v54's receipt shows one 25.4 s attempt, not 89.5 s). Five journeys are too few for a p95.

## Route decision and the Q14 targets

Q14 sets source-attached draft/plan completion at p95 ≤30 s. On Terra, the time is in provider calls, so orchestration or frontend work cannot reach that for plans.

The estimates below come from the design challenge. They assume all changes in this record are in: CPU/memory raise, native binary, broker feature trim, recheck edits, balanced batches and note prefetch. They also assume unchanged fixtures and provider model time, about 80% of local Codex setup removable (inferred from event order), first-pass rejection staying near 40% and no concurrent load.

| Path (Pass 6 example) | Observed (s) | After the changes (**estimate**, s) |
| --- | --- | --- |
| First review accepted, 3 batches (v55) | 44.0 | 33–36 |
| First review accepted (v57) | 52.5 | 43–45 |
| First review accepted, 2 batches (v53) | 60.3 | 49–52 |
| One rejection (v56) | 78.7 | 58–64 |
| Two rejections (v54) | 116.0 | 78–86; artifact check about 65–70 s instead of 89.5 s |

- Per-call Codex setup: about 3.7 s → 1.0–1.5 s for single calls, and about 8 s → 1.5–2.5 s for parallel review batches (**estimate**).
- Plans: p50 about **45–50 s**; p95 about **80–88 s** while 5% or more of plans take the two-rejection path. About 20% do today (**estimate**: 40% first-pass rejection × about half of first rechecks rejecting).
- Notes: p50 about **23–25 s**, p95 about **30–33 s** (**estimate**). This is borderline against 30 s.
- **The plan p95 ≤30 s target is not reachable on Terra with a complete high-effort review.** Even the best estimated plan is about 33 s. Reaching it would need a higher first-pass acceptance rate or a rule change, not more orchestration work. **The owner needs to restate the plan target** before the Q14 cohorts are judged. This record does not change the target or the manifest.
- Qualification risk (**estimate**): six Pass 6 project takes timed out (v34, v36, v40, v42, v45, v48). At a 15–20% failure rate, five consecutive passes happen only about 33–44% of the time. Slack on the two-rejection path is worth more than p50 gains.

## Broker findings (Pass 6 build, measured)

Read from copies of the Codex log database and the `integration-api` cgroup counters. The copies were deleted after reading.

- **Per-call Codex setup.** The broker spawned a new Codex app-server process for every call. Setup before the model request took p50 **3.74 s** for single calls (3.45–6.09 s, n=15) and p50 **7.99 s** for review batches started three at a time (4.58–10.32 s, n=23). Review calls spent 85–95% of their time before the first output delta.
- **Connectors, plugins and MCP were loaded with approval policy `never`.** Each session loaded the ChatGPT account's connectors and plugins (github, teams, vercel, cloudflare, convex and others) and launched plugin MCP servers through npx, with about 7.8 MCP startup notifications per call. It also fetched a remote plugin catalog, opened a remote-control websocket and posted analytics. The only guard was a prompt instruction. This was a security finding as well as a latency cost.
- **Prompts were logged to `logs_2.sqlite`.** Each call's full prompt, including attachments and candidate segments, was kept in the connection home: 181 MB, 556 turn-input rows of up to 33 KB since 21 September, and the attachment marker on 33 of the 38 retained call rows. This conflicts with the rule of no shadow archive of drafts or source text ([next steps](PRODUCT_RECORDING_NEXT_STEPS_2026-09-20.md#contract-and-authority-boundaries)).
- **CPU and memory caps.** The container was capped at 2 CPUs / 2 GiB on a 20-CPU VM. The cgroup was CPU-throttled in 9.3% of periods, and memory peaked at the 2 GiB cap (two limit hits, no OOM kill). Parallel calls stretched every local setup phase evenly.

## Broker fixes (built, not deployed)

In `apps/Ingestion Plane/integration-corev2` and the `integration-api` service in `apps/Ingestion Plane/docker-compose.yml`:

- **Hardened app-server as the default.** `CODEX_SUBSCRIPTION_TRANSPORT` defaults to `app-server` and `CODEX_APP_SERVER_HARDENED` to `true`. Process-level `-c` flags turn off plugins, remote plugins, apps and connectors, skills, hooks, tool suggestion, shell and exec tools, web search, memories, analytics, feedback and history. Every MCP server in the connection's `config.toml` is turned off by name (`appserver_hardening.go`). A fallback turn is refused when the file cannot be decoded or declares a server the broker cannot turn off. Codex processes no longer inherit `DATABASE_URL` or secret-named variables. `CODEX_APP_SERVER_HARDENED=false` restores the old arguments without a rebuild. The trim removes Codex-injected tools and skills, so it changes what the model sees on every journey.
- **Ephemeral SQLite home.** `CODEX_SQLITE_HOME` and `-c sqlite_home` point at a per-process directory that is deleted when the process closes. Startup removes any leftover `.ephemeral` directories. No new `logs_2.sqlite` prompt log should survive a call. The legacy files are left alone (see the owner action below).
- **Native binary.** The image links the vendored Codex binary at `/usr/local/bin/codex-native`, and compose points `CODEX_APP_SERVER_COMMAND` there, so a call starts no Node process. An offline measurement saved about 62 ms per idle spawn (p50 75.0 ms through the launcher, 12.8 ms native, n=15). Contended spawns were not measured. The broker falls back to `codex` if the path is missing. Rollback: `CODEX_APP_SERVER_COMMAND=codex`.
- **6 CPU / 4 GiB.** The compose defaults are now `INTEGRATION_API_CPU_LIMIT` 6.0 and `INTEGRATION_API_MEMORY_LIMIT` 4G. `init: true` reaps Codex process trees. The limits apply only when the container is recreated.
- **Token custody.** The broker is the only refresher of each connection's `auth.json`. It uses a per-connection lock plus a file lock, refreshes 10 minutes before expiry and once after a 401, and writes back atomically with mode 0600. It refreshes before spawning, so Codex never refreshes during a call. No process starts while a rotated grant exists only in memory. Run one broker replica per volume, and never run Codex by hand against a live connection home. Tokens never leave Integration Core.
- **Direct transport, opt-in.** It sends plain Responses API requests over SSE to `https://chatgpt.com/backend-api/codex/responses` with the connection's own token, `originator: verevon` and `store:false`. Switching needs `CODEX_SUBSCRIPTION_TRANSPORT=direct` and a container recreate, not a rebuild. No live call has been made on it.

Broker `go build`, `go vet` and `go test ./...` pass on Windows, and the module passes `-race` in an offline Linux container. These are unit results, not live evidence.

The rebuilt stack also adds content-free timing: broker phases (`spawn`, `initialize`, `thread_start`, `turn_start`, `first_delta`, `turn_total`), inference-core `ProviderTiming` and a per-turn `turn_timing` event from model-gateway. These replace the log reconstruction above as the source for Q14 cohorts.

## Latency levers shipped on

Owner decision 2026-09-24. Each lever changes what the model is sent on the recorded route, so all four must land before the freeze and be covered by the validation below. The three gateway switches are read once per process (`latency_levers.rs`): a lever is on unless its variable is `0`, `off`, `false`, `no` or `disabled`, and off restores the earlier request after a restart. The broker switch defaults to `true`; `false` restores the earlier `thread/start` parameters.

| Lever | What changes | Kill-switch | Effect (**estimate**) |
| --- | --- | --- | --- |
| Note-turn plan prefetch | On a new sourced note in a thread with exactly one artifact, the gateway reads that artifact through the audited `read_artifact` path (`gateway-sole-note-source-read`) before the first round. The round then offers only `create_artifact`, and the note instruction drops its read invitation. Covers the project “intern status” and sales “ledernotat” follow-ups. | `MODEL_GATEWAY_SOLE_NOTE_PREFETCH` | p50 0; about −6.6 s on notes that used to read first. The other notes gain about 3 KB of plan text. Every such note now shows a read step in the Work panel; film captions must describe it truthfully. |
| Recheck proposes edits | The second review after a rejection may propose edits. A complete review still follows, and the author repair stays as the fallback. `REPAIRS_PER_STAGE=2` and the maximum of three source reviews are unchanged. | `MODEL_GATEWAY_RECHECK_PROPOSES_EDITS` | p50 0; about −4 to −7 s on the two-rejection path, which is where timeouts break a streak. |
| Balanced review batches | Review batches use the lightest contiguous split by bytes (`balanced_ranges`) at the same batch count. Every segment is still reviewed once, with the full document and sources. | `MODEL_GATEWAY_BALANCED_REVIEW_BATCHES` | About −1 to −3 s on passes where the heaviest batch is slowest. Across 26 recorded plans the heaviest batch drops by 338 bytes on average; the fit is weak (R² about 0.3). |
| `thread/start` effort | `thread/start` sends the turn's `model_reasoning_effort`, so Codex's startup prewarm can match the turn. | `CODEX_APP_SERVER_THREAD_EFFORT` | About 0 on the recorded route, because its calls carry output schemas that the prewarm cannot match. Helps calls without a schema. |

## Levers rejected

| Lever | Reason |
| --- | --- |
| Lower review effort | Low effort failed the labelled matrix at 103/104 by falsely rejecting a valid conditional date ([pass 4](PRODUCT_RECORDING_CAPTURE_PASS4_2026-09-23.md)). The broker accepts only low or high. A quicker but less accurate checker is not a pass. |
| Narrow recheck (changed and dependent segments only) | It conflicts with the independent complete recheck rule ([recording plan](../README.md), item 1; Q13 in the [next steps](PRODUCT_RECORDING_NEXT_STEPS_2026-09-20.md)). In v54 the recheck flagged a segment in a batch whose bytes had not changed since pass 1 approved it. Two earlier fast-recheck experiments were removed. It would need an owner rule change and is not a pre-freeze lever. |
| Provisional “being checked” preview | It improves perceived time only; completion time is unchanged. It would show unchecked claims that later edits may change, while the film must promise only what the app did. It overrides the guard that keeps staged candidates out of tool progress, and it depends on a labelled-draft failure path that does not exist yet. |
| Warm pool of pre-started Codex processes | After the trim and CPU raise, the saving is about 0.5–1 s per call (**estimate**, unmeasured). Memory was at the 2 GiB peak, refills compete for CPU, and prewarmed sockets and sign-ins can go stale. It is non-trivial concurrency to qualify before a freeze. Revisit after the freeze only if measured setup stays at 1.5 s or more per call. |
| Early stop when the budget runs low | It only makes doomed turns fail sooner, and a failed take resets the streak however fast it fails. At v54's second rejection, 33.8 s remained and repair plus recheck needed 33.3 s; a p95-based stop would have failed a take that passed. |

The 90 s artifact deadline and the 150 s tool-phase deadline also stay. Raising them would only turn timeouts into slow passes.

## Validation after the rebuild, before the freeze

Rebuild through the launcher only: `./build-verevon-services.sh --from model` from the Verevon v3 root, which covers the Model, Application, Ingestion and Frontend planes. A hand-assembled `docker compose build` misses the env chain. Then run, in order:

1. **Broker and image checks.** The build log shows Codex 0.153.4. `/usr/local/bin/codex-native` resolves to the vendored binary. The broker's ready line shows `transport=app-server`, `app_server_hardened=true` and `app_server_launcher=native-npm`, with no `app_server_command_missing` warning. The container limits read 6 CPUs and 4 GiB. During a call, Codex runs as a direct child with no Node process. `thread_effort_differs` stays at 0.
2. **Offline hardening check** on the rebuilt image with the network off. Root-level `--strict-config` is accepted by `app-server`, and the `mcp_servers.<name>.enabled=false` and `sqlite_home` overrides take effect. On live calls, confirm that no MCP server, plugin or connector starts, and that no new `logs_2.sqlite` appears outside the ephemeral directories.
3. **104-case labelled review matrix** (`source_review_live`). It must stay at 104/104.
4. **10 recheck controls** (`source_recheck_live`). They must stay at 10/10. Every recheck in them now carries the edit schema.
5. **Note-turn runs.** Run `Q05 04-prosjektplan` and `Q05 02-salgsrapport` with `--repeat-each=20 --workers=1`. Check first-pass acceptance and the oracle. Each note must show one `read_artifact` before `create_artifact` and no tool-decision round. The initial plan turns in the same runs exercise the balanced batches and recheck edits.
6. **One smoke turn** on the existing connection: route `gpt-5.6-terra` via `openai-codex-subscription`, served model, usage with cost basis `subscription`, and the `turn_timing` event with broker phases present.
7. **Configuration record.** `CODEX_SUBSCRIPTION_TRANSPORT=app-server`. All four kill-switches at their default (on). `MODEL_GATEWAY_ARTIFACT_CHECK_TIMEOUT_SECS` and `MODEL_GATEWAY_TOOL_PHASE_TIMEOUT_SECS` unset, so the 90 s and 150 s defaults apply. Record the values with the run.
8. **Freeze prerequisites** from the cross-review. Every new build input must be tracked before any freeze commit; several broker, gateway, inference-core and session-core files are still untracked, and the launcher builds from the working tree. Add a runbook step for rolling session-core back past migration 0040.

A failure in any step is fixed and rebuilt before the freeze. Turning off one lever by its switch is also a build configuration change, so it must happen before the freeze, not during a streak. A pass in this validation is not a qualified release run.

## Live validation results (2026-09-24/25)

All live calls ran on the owner's ChatGPT subscription: 357 inference calls after the rebuild, every one on `openai-codex-subscription` / `gpt-5.6-terra` (inference-core log). No Verevon Balance or API-billed route was used.

**Rebuild.** `build-verevon-services.sh --from model` exited 0.
- Every changed service was recreated and is healthy.
- session-core migration 0040 and the cost-core `cost_basis` column applied.
- The broker runs `transport=app-server`, `app_server_hardened=true`, `app_server_launcher=native-npm`, with 6 CPU / 4 GiB. The native binary needed `CODEX_APP_SERVER_COMMAND=/usr/local/bin/codex-native` in `integration-corev2/.env`, because the old `codex` value there overrode the new compose default.

**Regression found and fixed.** The first live journey drafted and checked its plan but ended in `assistant_persist_failed`.
- Cause: the new `CompleteDirectRunWithMessage` refused the gateway's own terminalize credential (`FAILED_PRECONDITION`). The Control Plane registry marks every model-gateway→session-core token `zdr`, whatever the turn's retention.
- Fix: the retention check now uses the run's own `zdr` flag, which is recorded at start. A zero-retention run is still never written.
- Coverage: regression tests go through the production JWT verifier. Both services now log the real gRPC code.
- After a rebuild the journey passed.

**One project journey (Q05 04-prosjektplan), after the fix.** A single sample; it is not a percentile.

| Turn | Server | Provider | System | Review |
|---|---|---|---|---|
| Plan | 40.0 s | 39.2 s | 0.8 s | accepted on the first pass, 3 batches, first draft at 24.3 s |
| Note | 42.8 s | 42.5 s | 0.3 s | first review rejected; a reviewer edit was accepted on recheck, no author repair |

**Review-quality gates, and the two levers switched off.** Every error below is a false rejection (over-strict). There were no false acceptances.

| Run | Matrix (104) | Recheck controls (10) |
|---|---|---|
| All four levers on | 103/104 (`unapproved-quoted-as-warning`) | 9/10 (`sample-expanded` missed the invalidated dependent claim) |
| Recheck-edits and balanced-batches off | 102/104 (`unapproved-quoted-as-warning`, `majority-eight-of-eleven`) | **10/10** |

Targeted A/B on the failing items:
- `MODEL_GATEWAY_RECHECK_PROPOSES_EDITS`: `sample-expanded` failed in 2 of 4 runs with the lever on and 0 of 3 with it off.
- `MODEL_GATEWAY_BALANCED_REVIEW_BATCHES`: `unapproved-quoted-as-warning` failed in 3 of 6 runs with the lever on and 0 of 5 with it off.

Both levers are now **off by default** in `apps/Model Plane/deploy/docker-compose.yml`, with the evidence in the comments. The note-turn prefetch and the broker thread/start effort stay on.

The two remaining matrix cases are borderline reviewer judgements on this build:
- **With thread effort on (default):** `unapproved-quoted-as-warning` failed 1 of 8 times and `majority-eight-of-eleven` 0 of 8.
- **With thread effort off:** each failed once in 8 runs.
- **Reading:** thread effort makes no measurable difference. A full matrix run therefore currently lands at 102–104/104, and the earlier 104/104 bar is not reliably met. Whether the broker hardening (Codex-injected tools and skills removed) moved these two cases was not tested: that A/B would re-enable the connector exposure.

**Other checks.**
- Governed Voice mode e2e: 4 of 4. The earlier failures were a fake audio element; the product was correct.
- `memory-dedup plan --classifier off` (read-only): 11 clusters, 13 proposed retirements, all in the `review` tier. One proposal looks wrong: it retires `employer_sector`.
- A related SPA gap was fixed: a message sent before the model catalogue loaded dropped its privacy tier.

### 20-run journey series (2026-09-25, levers as shipped)

Q05 runs on the Terra subscription with the harness route guard. The build ran note prefetch on, recheck-edits off, balanced batches off, and thread effort on. Durations are `turn_timing` server times. The Q14 rule of at least 20 samples is met for the plan and report turns; the 04 note turn has 19. Failures are kept.

| Turn | n | p50 | p95 | max | Review passes and repairs |
|---|---|---|---|---|---|
| 04 plan | 20 | 44.1 s | 93.5 s | 101.7 s | 15×1, 1×2, 4×3; 4 author repairs; 1 check timeout |
| 04 note | 19 | 24.7 s | 48.0 s | 51.1 s | 14×1, 5×2; no repairs |
| 02 report | 20 | 53.9 s | 139.1 s | 168.6 s | 11×1, 4×2, 5×3; 2 author repairs |
| 02 ledernotat | 20 | 24.6 s | 31.2 s | 31.9 s | 19×1, 1×2 |

- **Pass rate:** 02 passed 20/20. 04 passed 19/20. The one failure took the two-rejection path (reviewer edit, rejected recheck, author repair, third review) and hit the 90 s check deadline (`response_validation_timeout`, 90.2 s).
- **Orchestration** was about 0.24 s p50 on every turn type. The rest is provider time.
- **Compared with Pass 6 (n=5):** the 04 plan median went from 60.3 s to 44.1 s, and the 04 note median from 35.3 s to 24.7 s.
- **Batch skew:** with the bisected split, batch 0 is the slowest batch (26–29 s against 10–18 s). That is the latency cost of keeping balanced batches off for accuracy.
- **Rejected notes:** 5 of 19 04 notes were rejected once, against 5/5 accepted in Pass 6. The prefetch A/B below could not isolate whether prefetch caused this.

**Note-prefetch A/B (inconclusive for notes).**
- **Run:** 10 × 04 with `MODEL_GATEWAY_SOLE_NOTE_PREFETCH=0` passed 7/10.
- **Cause of the failures:** all 3 were **plan** content-oracle failures (the plan omitted a `stedfortreder|reserve|backup|buffer` mitigation). The plan turn does not use prefetch.
- **Note-turn data:** lost, because the gateway was recreated to restore defaults before its per-turn log was read.
- **Result:** prefetch stays on.
- **What it does show:** a plan-author content miss of about 3 in 30 plans. Together with the roughly 5% timeout path, that puts the chance of five consecutive 04 passes at roughly 45–60%.

### Plan-author guidance v2 A/B (2026-09-25)

**Findings that changed how the baseline is read.**
- The mitigation oracle `/stedfortred|reserve|backup|buffer/i` measures restatement, not mitigation. 37 of the 38 saved v1 plans met it only by restating a source gap (for example "uten avtalt stedfortreder"). **None proposed a mitigation.**
- inference-core's 60 s response cache replayed author drafts between back-to-back runs: 11 of 31 plan turns were replays. The "3 of 30" content misses were one draft served three times.
- The resample draws in the verification pass had the same problem, so their consistency check could compare a copy with itself.
- **Fix:** tool rounds and resample draws now send `cache-control: no-store`, with a regression test that fails without the fix. Titles still use the cache. In the v2 series: 0 cache hits.

**Guidance v2.**
- **Where:** `model-gateway/src/author_guidance.rs`, switch `MODEL_GATEWAY_AUTHOR_GUIDANCE=v1|v2`, compose default **v1**.
- **What it changes:**
  - one labelled proposal for each stated risk or conflict;
  - `ikke oppgitt; forslag: …` cells;
  - conditional dependency wording;
  - a four-bullet structure;
  - a character budget.
- **Leakage guard:** a test fails if v2 contains fixture names, dates or oracle words.
- **Local guards:** under v2, a qualifier exempts only its own cell or sentence, not the whole line.

**Live A/B.** Terra only. 20 × journey 04 with v2, compared with the v1 fresh-draft baseline.

| | v1 (fresh drafts) | v2 |
|---|---|---|
| Plans with a labelled mitigation proposal | **0 / 38** | **15 / 15** saved plans |
| Journey passes | 19 / 20 | 14 / 16 valid runs (1 check timeout, 1 plan missing the "24" schedule anchor) |
| Plans needing 2–3 review passes | about 5 / 20 | **9 / 16** |
| Slowest plan | 101.7 s | 117.7 s (timeout); several at 70–104 s |

- **Outage exclusion:** 4 of the 20 v2 runs, and all 10 planned journey 02 runs, failed at about 8.7 s from 10:24:30Z onward. The broker returned 502 `subscription_broker_failed` on every call, most likely a usage limit on the ChatGPT subscription after the day's test volume. They are excluded as an outage, not counted as guidance failures.
- **Verdict:** v2 fixes the content gap but roughly doubles reviewer rejections, most likely because the reviewer treats the new proposals as unsupported claims. It **fails** the pre-set non-inferiority rules, and v1 stays the default.
- **Next iteration:** align the reviewer so a clearly labelled proposal that asserts no new fact is not judged as a factual claim. That changes the reviewer, so the 104-case matrix and the 10 recheck controls must be rerun afterwards.

### Reviewer proposal rule and subscription refusal reasons (2026-09-26)

**Reviewer proposal rule (`proposal-rule-2`, `MODEL_GATEWAY_REVIEW_PROPOSAL_RULE`, default off).** The rule lets the source reviewer judge a clearly labelled proposal that adds no new fact as a proposal. It was built with paired accept/reject controls, and a loophole review closed 10 gaps before any live run. Live matrix run with the rule **on** (guidance v2, levers as shipped, Terra only):
- **Confirmed cases:** 4 failed. With the rule off, 1–2 fail (`unapproved-quoted-as-warning`, `majority-eight-of-eleven`). The three extra were false rejections of valid documents: `four-batch-dependency-valid`, `sales-corrected-whole-pass7`, `project-conditional-pass8`.
- **New proposed controls:** 2 reject controls were **falsely accepted** (`pr2-header-follow-up-unlabelled`, `pr2-request-responsible-role`), and several valid-proposal twins were still rejected.
- **Verdict:** the rule **fails**. It adds false rejections and introduces false acceptances. It stays off. The recheck and journey stages were not run, because a coordinator script bug (ANSI colour codes in broker logs) falsely reported an outage. Every broker call in that window returned 200.
- **Next design, not yet built:** v2 puts all proposals inside segments that also state facts. The four closing bullets form one review segment, so a proposal can never receive `no_assertions`. A v3 that gives each proposal its own fact-free segment would let the unchanged reviewer judge it without relaxing any rule.

**Guidance decision for the freeze: v1.** v1 has the better journey pass rate (19/20 against 14/16) and the lower review load. v2 is kept behind its switch as evidence for the next iteration.

**Subscription refusal reasons (built and deployed).**
- **The problem:** the broker used to collapse every Codex refusal into `subscription_broker_failed`, so the chat said "the model did not answer".
- **Now:** a content-free reason code travels broker → inference-core → gateway → chat on both transports. The codes are `subscription_usage_limit` (with the reset time where known), `subscription_reauth_required`, `subscription_upstream_unavailable`, `subscription_context_too_long`, `subscription_bad_request` and `rate_limited`.
- **In the chat:** it shows, for example, that the plan's Codex usage limit is reached and when it resets. It never retries before the reset and never switches to another model.
- **Retries:** an untyped broker failure is now a single final attempt. It was briefly up to three Codex turns.
- **Owner confirmation needed:** the broker now returns the reset time and window length, which are account metadata and contain no content. This reverses its earlier rule of returning no reset information.

**Journey 03 live smoke on this build.** It failed: post 2 counted 59 words against the 60–90 requirement, although the gateway had passed its own word checks. The cause is under investigation as a piece-boundary mismatch between the gateway's check and the requirement. It must be fixed before the freeze.

### Journey 03 fixes and the build freeze (2026-09-26)

**Two journey-03 defects were found live and fixed before the freeze.**

1. **Word counts included metadata.** `document_contract.rs` counted each piece's `**Status:** Utkast til gjennomgang` line as post words.
   - The gateway therefore published a post whose copy had 59 words against the brief's 60–90.
   - Whole metadata label lines (status, date, channel and similar) are now excluded from the counted copy.
   - The document guidance now states the body targets in plain words.
2. **Same-turn rewrite after acceptance.** After a new campaign document was accepted and staged, the tool loop asked for one more decision round.
   - The model rewrote the whole document, which triggered a second full review and hit the 150 s tool-phase deadline. It timed out in 2 of 2 runs.
   - A complete campaign now ends the tool phase.
   - A repeat write of an accepted new document is dropped before any check.
   - A queued user correction still gets its round.

After the fixes, journey 03 passed 2/2, in about 58–93 s with one model round. Journey 02 passed 1/1. Journey 04 failed 1/1 on the plan mitigation-keyword oracle, the known v1 content variance.

**Freeze record: 2026-09-26T06:36:27Z.** Branch `fix/ui-premium-restore` at `21d636a5`, plus an uncommitted working tree. Nothing is committed, at the owner's request.

- **Working-tree fingerprint:**
  - tracked diff sha256 `19414df367b80cde…`;
  - 158 untracked files under `apps/`, sha256 of their hashes `351decda93325d6d…`.
- **Image IDs (sha256 prefixes):**

  | Service | Image |
  |---|---|
  | model-gateway | `7ea2ea318efd` |
  | inference-core | `165b0a3b458c` |
  | session-core | `a79bb3873919` |
  | capability-core | `b81af8a6af0f` |
  | cost-core | `530f1a361371` |
  | letta-bridge | `2fea3dec2db1` |
  | execution-core | `0c50b84662bf` |
  | integration-api | `aba8b80474cc` |
  | verevon-gateway-rs | `a98974324e09` |
  | frontend | `89de71a26c38` |

- **Switches:**
  - author guidance `v1`;
  - review proposal rule `off`;
  - sole-note prefetch on;
  - recheck edits off;
  - balanced batches off;
  - broker `transport=app-server`, hardened, native launcher, thread effort on, 6 CPU / 4 GiB.
- **Fixture hashes:** all 13 `manifest.json` source hashes match.
- **Q16 series:** starts on this build with `scripts/capture-product.mjs --mode rehearsal`, five consecutive passes per journey, no automatic retry, and at most 8 attempts per journey. Every attempt is kept under `C:/private/verevon-recordings/q16-*`. Any change to code or switches after this point resets every streak.

### Q16 qualification result (2026-09-26)

All four scenarios reached **five consecutive passes on the frozen build**. The runs used `capture-product.mjs --mode rehearsal` with no automatic retry, and every attempt is kept under `C:/private/verevon-recordings/q16-20260926T0636Z`. Each take's `.run.json` records the real image IDs.

| Scenario | Consecutive passes | Attempts | Failed attempts |
|---|---|---|---|
| 01-kundesvar | 5 | 5 | none |
| 02-salgsrapport | 5 | 7 | #2: **a harness false failure, corrected 2026-09-26.** The report was correct (total +1 680, Skrivebord −18 720, both match the CSV). The oracle's `grossProfitChangeClaims` (`tests/e2e/product-sales-oracle.ts`) attributed Skrivebord's figure to the total. |
| 03-kampanje | 5 | 10 | #5: `response_validation_timeout` |
| 04-prosjektplan | 5 | 5 | none |

Qualification per-turn server times. Turns were paired first/follow-up by order within each scenario window, so treat this as approximate, and n is small (5–8 per cell):

| Scenario | First turn p50 / p95 / max | Follow-up turn p50 / p95 / max |
|---|---|---|
| 01 | 28.7 / 30.5 / 30.6 s | 43.8 / 52.6 / 54.7 s |
| 02 | 27.8 / 50.2 / 51.4 s | 53.8 / 67.5 / 70.0 s |
| 03 | 74.5 / 109.4 / 119.2 s | 61.8 / 122.7 / 129.3 s |
| 04 | 75.3 / 105.5 / 105.5 s | 32.8 / 42.4 / 44.5 s |

**Manifest:** every task now records `consecutivePassesOnReleaseBuild: 5` with this record. `recordingReadiness.gates.repeatability` is `passed`. Status stays **blocked**, and the other gates are unchanged:
- **correctness:** the missing deterministic CSV-figure check and the harness oracle bug in 02;
- **performance:** no owner-restated latency target;
- **durability and recovery:** restart and new-browser checks are not evidenced;
- **trust and experience:** mobile, keyboard, captions and reduced-motion review are not evidenced.

`--mode release` therefore still refuses to run, as designed.

**Top open items (corrected 2026-09-26).**
- **Harness bug:** the 02 attempt-2 failure came from a scope bug in the harness oracle, not from the product; the oracle must be fixed.
- **Gateway gap:** the gateway has no deterministic check that binds a stated monetary change to the attached CSV, so a genuine wrong figure could pass. That check is being added.
- **Streaks:** both changes alter the build or the harness, so they reset the streaks and belong in the next freeze.

**Also for the next freeze: rewrite-guard coverage.**
- **What the guard catches:** a second write in the same turn only when it reuses the accepted document's id, a near-miss of it, or its title.
- **What it misses:** a same-turn rewrite under a completely new id and title is still reviewed and can still use up the tool-phase budget.
- **Why it doesn't hit journey 03:** the phase now ends before any second round.
- **Journey 02 in the last runs:** in 4 of 20 runs it rewrote in the same turn.
  - If the log line "same-turn rewrite of an accepted document not dispatched" never appears while such rewrites continue, this gap is the reason.
  - Extend the early phase end to every accepted new source-bound document.

### Performance target set by the owner (2026-09-26)

These targets hold for the Terra subscription route, per journey:

| Measure | Target |
|---|---|
| First turn, server time | p95 ≤ 120 s |
| Follow-up turn, server time | p95 ≤ 70 s |
| Timeouts in a qualification series | none |

**Measurement rules:**
- Measure from `turn_timing` with at least 20 samples per journey turn, failures kept.
- Five-run qualification streaks alone are too small for a p95.

**Against the Q16 series (small n):**
- Every first turn is within target (03 p95 109.4 s, 04 p95 105.5 s).
- The follow-ups of 01, 02 and 04 are within target (52.6 / 67.5 / 42.4 s).
- The **03 follow-up misses:** p95 122.7 s against 70 s.
- 03 also had **one timeout**.

So the performance gate cannot pass until the 03 revision path is faster and the timeout path is closed.

### Owner ruling: receipt-bound reuse on revision turns (2026-09-26)

**The ruling:** a revision turn is a new user request to change an already accepted document. On such a turn, the source review may **reuse the earlier approval** for segments that are byte-identical to the accepted version.
- **Bound to:** that version's receipt, the same sources and the same checker.
- **Still reviewed:** changed segments and the segments that depend on them.
- **Still run on the whole document:** the deterministic local checks.

**Basis:** the plan's own permission to "validate only changed inputs where the version-bound receipt permits reuse" (NEXT_STEPS:122, 174).

**What it does not change:** the "independent complete recheck" rule (README.md:12) still applies to repairs **within** a turn. Reuse never applies there.

**Why:** the 03 revision turn misses the follow-up target (p95 ≈ 123 s against 70 s), because every revision re-reviews all 27 segments of which only one post changes.

### Revision reuse: built, found inert live, fixed (2026-09-26)

**What was built.** The switch `MODEL_GATEWAY_REVISION_REVIEW_REUSE` (default on; `0`/`off`/`false` restores the full review byte for byte). Reuse runs only on a cross-turn revision whose instruction names one dated post; any other instruction reviews the whole document. A segment's earlier approval is reused only when all of these hold:
- the segment is byte-identical to the accepted version;
- the source hash and checker are the same;
- the accepted text's content hash matches the receipt;
- the segment is outside the targeted post and its section;
- it shares no date, amount or proper name with a changed segment;
- it contains no reference word ("det", "denne", "samme", "nevnt", "above"…).

Within-turn repairs always get the complete recheck. The receipt records `reusedSegments` and `reusedFromReceipt` (id and version). The gateway's own dated-post preservation already copies every other piece byte for byte from the accepted version.

**Two reviews (loophole and rules lens) found four issues, all fixed with tests:**
- **Reference words:** a segment depending on a changed one only through a reference word could be reused. The reference-word guard now applies to reuse too, and "same"/"samme", "mentioned"/"nevnt", "previously"/"tidligere" and "unchanged"/"uendret" were added. The word list is shared with the within-turn recheck, which becomes slightly more cautious.
- **Duplicate documents:** `create_artifact` under a near-miss id forked a second document; it now resolves the id the way `update_artifact` does.
- **Dead code:** removed.

**Live smoke 1 (journey 03, Terra): passed, but reuse never ran.** The revision receipt showed `reusedSegments: 0`, `reviewedSegments: 28`. Its 55.1 s came from an ordinary single clean review pass. The first turn took 120.0 s with one repair.
- **Cause:** a turn checks and records its review in its private run workspace. The durable commit published the text but not the review, so the next turn never found one.
- **Fix:** the commit now publishes the review with its text, only when the committed version and text match what the review accepted.
- **Test:** a two-turn router test proves reuse on the second turn. It fails when the fix is removed, and a switch-off twin reviews everything.

**The entity rule was then tightened.** On the two recorded journey-03 revisions, most "dependencies" were noise: every date's year, the "000" of every amount, and words capitalized only at a sentence, cell, label or heading start (`Utkast`, `Kanal`, `LinkedIn`). The rule now treats:
- one calendar day as one entity in any spelling ("23. september 2026", "23.09.2026", "2026-09-23");
- one amount as one number in any grouping ("3 000", "3.000", "3,000");
- proper names only as words capitalized mid-sentence somewhere in the document, never heading words.

Measured offline with the real code on both recorded revisions:

| Recorded revision | Segments reviewed again (old rule) | Segments reviewed again (tightened rule) |
|---|---|---|
| Journey-03 revision A | 17 of 26 | 9 of 26 |
| Journey-03 revision B | 20 of 28 | 10 of 28 |

Under 12 segments, the revision's review is one call instead of four batches. The genuine links are still reviewed: the overview row and the email that name 23 September, a repeated amount, the product name.

**Adversarial review of the tightened rule.** It found two gaps, both now fixed with tests: dates written in ISO versus day-month form, and "5.000"/"5,000" versus "5 000". It also found one known limit, left as is. A person's name that only ever starts a sentence or bullet is not treated as a name. That affects consistency inside the document, not a segment's support against the unchanged sources. Counting sentence-start words would bring back most of the noise.

**Live smoke 2 (journey 03, Terra), after the commit fix: passed, and reuse ran.** The revision re-reviewed 9 of 29 segments and reused 20 from the version-1 receipt, in one review call. The revision turn took 66.7 s, the first turn 69.8 s.

**Mini-series (journey 03, 6 runs, same build): 5 passed.**
- **The failure:** attempt 2 failed on its first turn (`response_validation_failed`). Each of three review passes rejected a different single segment (16, then 24, then 10). First turns never reuse. The old build showed the same variance (Q16: 1 failure in 10 journey-03 attempts).
- **Revision turns** (together with smoke 2):

  | Server time | Segments reused |
  |---|---|
  | 55.0 s | 16 |
  | 60.0 s | 20 |
  | 65.1 s | 17 |
  | 66.7 s | 20 |
  | 78.0 s | 10 of 22 |
  | 112.8 s | see below |

  In the 112.8 s turn, reuse ran on the first pass: 11 segments in one call, taking 53 s with 4,033 output tokens. That pass rejected the edited post itself, and the ruling's complete recheck (28 segments, 4 batches) then accepted it. The receipt reports the final complete pass, so it shows `reusedSegments: 0`, which is correct.
- **First turns:** 56.5, 60.7, 69.8, 92.0, 126.3 and 134.7 s. The two slowest had repair passes.
- **Batching:** over 210 logged review calls, a call's time does not grow between 5 and 10 assigned segments (p50 13.0–13.9 s, p90 19–24 s). So one reused call is no slower than the slowest of four parallel batches, and batching is unchanged.

**Round-3 review (four lenses, each finding verified by a refuting agent): 5 confirmed, 0 refuted, all fixed with tests.**
- **Ruling violation:** a queued correction's second write in the SAME turn could reuse that turn's own, never-saved review. The lookup now reads a snapshot of the saved store taken at the start of the turn. A router test covers it.
- **Entity gaps:**
  - "kr 1.680,-" and "kr 1 680,00" are now the same amount;
  - a sentence-final "punkt 3.2." also counts as the number 3.2;
  - "21.–25. september" and "fra 21. til 25. september" keep both days.
- **Id lookup:** `update_artifact`'s dispatch falls back to the title, as `create_artifact`'s dispatch and the check already do.
- **Tests:** two cheap missing ones were added. Low-severity items are left open, all in the over-inclusive (safe) direction:
  - Markdown-link names;
  - month-first English dates;
  - hyphenated versus spaced names;
  - an isolated bold-label test.
- **Effect on reuse:** none of the new patterns occur in the recorded journey-03 documents, so the measured reuse (9–11 reviewed) is unchanged.

**Tests:** model-gateway 1470 unit tests and 79 end-to-end tests pass, as does every other integration suite. The live reviewer matrix was not rerun: first-pass review requests are unchanged, and the only change to within-turn rechecks is the more cautious reference-word guard.

**What the ruling means for the 03 follow-up target.** Reuse removes the full re-review from a clean revision, which now takes about 55–78 s. When the reviewer rejects the edited post, the ruling requires a complete recheck within the turn, and that path takes about 110 s. So the 03 follow-up p95 depends on how often the edited post is rejected. The 20-run series measures it.

### Second freeze: revision reuse build (2026-09-26T17:42:54Z)

Branch `fix/ui-premium-restore` at `21d636a5`, plus an uncommitted working tree. Nothing is committed, at the owner's request.

- **Working-tree fingerprint:** tracked diff sha256 `7a20d36662854177…`; 164 untracked files under `apps/`, sha256 of their hashes `baeba617f17f821f…`.
- **Image IDs (sha256 prefixes):**

  | Service | Image |
  |---|---|
  | model-gateway | `3e064480320c` |
  | inference-core | `9760f9d2deff` |
  | session-core | `e101544b1829` |
  | capability-core | `c7d6a49906a4` |
  | cost-core | `970b55da4a90` |
  | letta-bridge | `c7df70693c9d` |
  | execution-core | `a791dcb41050` |
  | integration-api | `aba8b80474cc` (unchanged from the first freeze) |
  | verevon-gateway-rs | `a3c7e080e65c` |
  | frontend | `526d7b5b56d5` |

- **Effective switches (the gateway's startup log):**
  - author guidance `v1`;
  - review proposal rule `off`;
  - sole-note prefetch on;
  - recheck edits off;
  - balanced batches off;
  - **revision review reuse on**;
  - broker `transport=app-server`, hardened, native launcher, thread effort on, 6 CPU / 4 GiB.
- **Fixture hashes:** 13 of 13 `manifest.json` source hashes match.
- **Recovery gate on this build:** `product-recovery.spec.ts` passed 7 of 7, with evidence in `C:/private/verevon-recordings/gates-20260926/recovery`:
  - a dropped connection auto-resumes into one turn, with no new invocation;
  - each of `subscription_usage_limit`, `subscription_reauth_required`, `subscription_upstream_unavailable`, `subscription_route_unavailable`, `subscription_bad_request` and `rate_limited` is actionable, keeps the partial answer, and a retry succeeds.
- **Series:** `qualify_series.sh` runs 20 attempts per journey through `capture-product.mjs --mode rehearsal`, with no automatic retry.
  - The first five consecutive passes requalify each journey.
  - All 20 attempts, failures included, form the `turn_timing` sample for the owner's p95 targets.
  - Every attempt is kept under `C:/private/verevon-recordings/q16b-*`, with its UTC window logged.
  - The series stops on a subscription usage limit or refusal.
  - Any change to code or switches after this point resets every streak.

### Journey-02 harness oracle: five false failures, fixed during the series (2026-09-26)

The product build stayed frozen; only the test oracle changed (`tests/e2e/product-sales-oracle.ts`, `product-scenarios.spec.ts`). Every failed journey-02 report was checked against the CSV and was correct.

| Take | Oracle in use | What the report said (correct) | What the oracle did |
|---|---|---|---|
| 7 | Original | "Belysning økte med 30 000 kr i omsetning og 12 000 kr i bruttofortjeneste, og Oppbevaring økte med 24 000 kr og 8 400 kr" | Missed the amount before the metric and took Oppbevaring's revenue figure (24 000) as its gross-profit change |
| 9 | Intermediate | "… = 124 680 kr i bruttofortjeneste" (a worked level) | My first fix read this level as a change |
| 16 | Intermediate | Channel figures: Nettbutikken +7 360 kr, Bedriftssalg −5 680 kr | Knew only product groups, so treated a channel's figure as the total's |
| 17 | Intermediate | "fra 59 000 kr til 53 320 kr i bruttofortjeneste" (−5 680) | Took the end level as the change |
| 20 | Intermediate | "Oppbevaring (+24 000 kr; +8 400 kr)" | Took the revenue figure again |

**The final oracle:**
- **Amount before the metric:** it reads "N kr i bruttofortjeneste" as a change only with a change verb, a sign, or a "fra A til B" span. A span counts as the difference.
- **Clauses:** it stops at a new clause only when that clause names another scope.
- **Scopes:** it knows both sales channels and Norwegian definite forms, and binds a comparison ("mer enn Oppbevaring") to its subject.
- **More verbs and formats:** it reads "reduserte", "gikk ned" and "minsket", period-grouped thousands, and a percentage after ";".
- **Elliptical amounts:** an amount with no metric word ("og 8 400 kr") is still not read as a claim.

An independent review tried to make it accept wrong figures and found the scope, verb, sign and clause gaps, which are now fixed. The oracle unit tests pass 73 of 73.

**Replay:** all 20 journey-02 takes, rerun through the final oracle, have zero rejected claims. Journey 02 requalified at attempt 5 (attempts 1–6 passed).

**Sample gap:** the five false failures stopped their takes before the follow-up turn, so journey 02 has 15 follow-up samples. The series is being topped up to 20 on the same frozen build.

### Q16b series result on the reuse build (2026-09-26, 17:44–21:10 UTC)

80 attempts, 20 per journey, on the second freeze. Every attempt is kept under `C:/private/verevon-recordings/q16b-20260926T1744Z`. Times are server `turn_timing`, nearest-rank p95, with failures kept.

| Journey | Passed | Five consecutive at | First turn p50 / p95 / max | Follow-up p50 / p95 / max | Timeouts |
|---|---|---|---|---|---|
| 01 | 20/20 | attempt 5 | 32.0 / 55.5 / 57.5 s | 35.8 / 51.9 / 54.5 s | 0 |
| 02 | 15/20 (5 harness false failures) | attempt 5 | 48.9 / 67.9 / 73.6 s | 23.5 / 42.7 / 42.7 s (n=15) | 0 |
| 03 | 18/20 | attempt 5 | 69.1 / 102.3 / 108.3 s | 69.6 / 117.4 / 124.6 s | 1 |
| 04 | 18/20 | attempt 8 | 68.7 / 107.3 / 113.4 s | 23.6 / 70.5 / 70.5 s (n=18) | 0 |

**Requalification:** all four journeys reached five consecutive passes on one frozen build.

**Against the owner's targets:**
- Every first turn is within p95 ≤ 120 s.
- The follow-ups of 01 and 02 are within p95 ≤ 70 s.
- 04's follow-up p95 is 70.5 s: one note turn needed two repairs, and n=18.
- **03's follow-up misses:** p95 117.4 s, 9 of 20 over 70 s, and one timeout.

**Failures:**
- **02:** takes 7, 9, 16, 17 and 20 were all harness false failures (see above). The reports were correct.
- **03 take 13, a product miss (lost framing):**
  - The reviewer rejected a suitability clause in the revised post and applied a private edit, which the complete recheck accepted.
  - The final post opens "På et delt kontor har Lys bordlampe en fotdiameter på 16 cm." It lost the requested "flere team" framing, so the journey check failed.
  - The edit instruction lists what an edit must preserve (facts, details, notes, word limits) but not user-requested framing, and the edit is applied mechanically.
- **03 take 17, a timeout:**
  - The reused single-call pass took 45 s and rejected the edited post.
  - The ruling's complete recheck (4 batches) found another unsupported segment, and the repair ran past the 90 s check budget (`response_validation_timeout` at 124.6 s).
- **04 take 3:** a first-turn `response_validation_failed`. Each of three review passes rejected a different single segment. This is reviewer variance on a path revision reuse does not touch.
- **04 take 15, a product miss:** the plan names the absence risk (21–23 September) but proposes no mitigation (the known author-guidance v1 variance).

**What limits 03's revision.** The breakdown of its 20 revision turns:
- **Drafting** takes 35 s at p50 (41 s at p95) over two model rounds: reading the document, then rewriting it.
- **The review** takes 35 s at p50 (83 s at p95).
  - Reuse held on the accepted pass in 15 of 19 revisions. The other 4 were rejections that forced the complete recheck.
  - With reuse, one call reviews about 10 segments of a 28-segment document and takes 13–40 s even when it accepts. A call that rejects and writes edits takes 45–56 s.
  - Across 463 logged batches, a batch that accepts has a p50 of 12.3 s, and one that rejects has a p50 of 19.3 s (p90 29.7 s).
- So reuse cut the work but not the wall-clock time. The one-call design loses the parallelism of the full review.

**Top-ups on the same frozen build** (21:10–21:33 UTC; journey 02 on the final oracle):
- journey 02: 5/5 passed;
- journey 04: 3/3 passed.

With them, every journey turn has at least 20 samples:

| Journey | First turn p50 / p95 (n) | Follow-up p50 / p95 (n) | Timeouts |
|---|---|---|---|
| 01 | 32.0 / 55.5 s (20) | 35.8 / 51.9 s (20) | 0 |
| 02 | 48.9 / 72.2 s (25) | 23.5 / 41.6 s (20) | 0 |
| 03 | 69.1 / 102.3 s (20) | 69.6 / 117.4 s (20) | 1 |
| 04 | 67.2 / 107.3 s (23) | 25.1 / 56.8 s (21) | 0 |

So 01, 02 and 04 meet every owner target. **03's follow-up is the only miss.**

### Durability, recovery and experience gates on the second freeze (2026-09-26)

Evidence is kept under `C:/private/verevon-recordings/gates-20260926/`.

- **Durability: passed.**
  - `product-durability.spec.ts` (`Q16_CAPTURE_ROOT` = the Q16b root) passed 21/21: a completeness check and 20 qualifying threads, 5 per journey, each reopened in a fresh browser context. Artifacts, versions, receipts and turn roles matched the server, with no console errors.
  - Then Model Plane Postgres, session-core, model-gateway and verevon-gateway-rs were restarted (21:36:39 UTC, same images). The spec passed 21/21 again.
  - All 20 evidence sets are identical before and after, ignoring timestamps.
- **Recovery: passed.** `product-recovery.spec.ts` passed 9/9. Each failure shows an actionable notice with no other model used, and "Prøv igjen" or a new message then succeeds. The cases:
  - a dropped connection auto-resumes into one turn, with no new invocation;
  - six subscription and rate-limit codes;
  - two new cases added for the live timeout: a draft `response_validation_timeout` and `response_validation_failed` show the gateway's message and publish nothing unchecked.
  - The live Q08 check "reload resumes the same run without another invocation" also passed on the restarted system, on the Terra subscription.
- **Experience: partial.**
  - `product-journeys-viewport.spec.ts` passed 9/9: mobile and desktop, keyboard-only and reduced motion on the four journeys.
  - The live Q08 "mobile markdown, keyboard and reduced motion" check passed.
  - Caption tracks and the editorial/media review belong to the product films and are not done.
- **Trust: partial.** No cross-tenant leakage, no memory promotion, temporary-chat policy and no side effects are not re-evidenced on this build.

**Manifest:**
- `repeatability`, `durability` and `recovery` are now `passed`;
- `correctness`, `trust`, `experience` and `performance` stay `partial`;
- every task records five consecutive passes on build `sha256:3e064480320c…`.

Status stays **blocked**. Policy tests pass 22/22.

### Round 4: revision latency changes, measured (not frozen, 2026-09-26 22:12–22:54 UTC)

**The build:** model-gateway `9da50466918c`, with the same switches as the second freeze plus three changes. It is a measurement build, not a freeze: nothing here counts toward qualification.
- **Batched reuse review:** a revision's reused review that still assigns 6 or more segments is split into batches. Each batch states only its own assignment.
- **Revision prefetch:** the gateway reads the thread's sole document before round 1 of a recognized scoped revision, under the existing `MODEL_GATEWAY_SOLE_NOTE_PREFETCH`. The next rounds do not offer `read_artifact` again.
- **Tests:** the full crate passes (1472 unit tests, 84 end-to-end tests). One review finding was fixed: a repeated read after the prefetch.

**Journey 03, 12 runs on Terra: 9 passed.**
- **Revision turns (n=10):** p50 52.4 s against 69.6 s on the freeze; p95 109.7 s; 3 over 70 s.
  - Drafting now takes one model round, 26.5–34.9 s.
  - Clean revisions take 44–55 s, with a 2-batch review of 11.5–22.6 s.
- **First turns:** two timed out (139.2 s and 131.8 s). Their review path is unchanged by this round; both were rejection cascades.
- **Rejections during this window:** 36–38% of review passes rejected, against 24–26% for journey 03 in the Q16b series, at similar pass times (p50 18–20 s). The difference sits on an unchanged code path, so the reviewer's behaviour varied over time. Comparisons across time windows are therefore confounded.

**Finding: rejected preserved segments cannot converge.**
- **The mechanism:** on a scoped dated-post revision, `Preservation::apply` copies every byte outside the targeted post from the accepted version, at the top of every check attempt. When the reviewer rejects such a segment, its edit or repair is reverted, and the next pass reviews the same bytes again. The turn then passes only if the reviewer changes its mind.
- **Round-4 attempt 2:**
  - The "### Kilder" list was rejected and its edit was reverted. The recheck saw the identical candidate hash and accepted it.
  - The turn passed at 73.7 s. The receipt and turn timing show 1 pass and 4 batches, because they count distinct candidates.
- **Round-4 attempt 6:**
  - The status note ("Datoene nedenfor er forslag; …") was reviewed fresh only because of the reference word "nedenfor".
  - It had been accepted in version 1. It was rejected in all three passes on the same candidate hash.
  - The result was `response_validation_failed` at 109.7 s.
- **Scope:** this predates reuse. A full review exposes every preserved segment to the same trap, and reuse lowers the exposure by reviewing fewer of them fresh.

### Owner rulings: scoped revisions, recheck scope, framing guard, guidance (2026-09-27)

A scoped revision is one whose instruction names one dated post, so `Preservation::apply` copies everything else byte for byte.

1. **Preserved segments keep their approval.**
   - A segment outside the targeted post that is byte-identical to the accepted version keeps its receipt-bound approval.
   - It is reviewed fresh only when it shares a changed date, amount or proper name with the edited post. Reference words alone ("det", "nedenfor") no longer make it a dependent.
   - If a genuinely dependent preserved segment is rejected, the repair edits only the targeted post, or the turn fails at once instead of looping on repairs that preservation undoes.
2. **Recheck scope on scoped revisions.**
   - After an edit within the turn, whether a reviewer edit or a model repair, the recheck covers the targeted post and its dependents. Preserved segments keep their approval, and the deterministic local checks still cover the whole document.
   - Every other repair, including every first turn and whole-document revision, keeps the independent complete recheck (README.md:12).
3. **Framing guard.**
   - A reviewer edit that deletes requested framing from the post this turn is revising is refused.
   - The check falls back to a model repair told to keep the requested audience or framing and remove only the unsupported claim.
4. **Author guidance stays `v1`.**

**Why:**
- **Preserved segments and recheck scope (rulings 1–2):**
  - Q16b's journey-03 follow-up p95 was 117.4 s with one timeout; round 4's was 109.7 s.
  - The rejection path, a complete recheck after an in-turn edit, takes about 110 s.
  - Rejections of preserved segments can never be repaired.
- **Framing guard (ruling 3):** Q16b take 13 lost the requested "flere team" framing.

**Implementation (uncommitted, model-gateway `src/source_validation.rs`, `turn_timing.rs`, `latency_levers.rs`, compose):**
- **Preserved segments (ruling 1):** a segment outside the targeted post is reviewed fresh only when it shares a changed date, amount or proper name.
  - "Changed" means the edit added or removed it, or mentions it a different number of times: replaced paragraphs are compared pairwise, so a swap between paragraphs counts.
  - A run that inserts or deletes paragraphs counts all its entities.
  - A bare 1–2 digit number is now an entity too, so a changed count such as "20 → 30" chairs is caught.
  - **Measured on five recorded journey-03 revisions:** 3–8 of 26–34 segments are reviewed, against 9–12 before. Three of the five have no preserved dependents.
- **Scoped recheck (ruling 2):**
  - **Which attempts:** on a scoped revision with a prior review, every later attempt recomputes the assignment against the accepted version, so only the targeted post and its dependents are reviewed. The local deterministic checks still cover the whole document.
  - **Other turns:** first turns, whole-document revisions and revisions without a prior review keep the complete recheck.
- **Fail fast (ruling 1):**
  - **First rejection:** a rejected preserved dependent gets one repair confined to the targeted post. It is told the preserved passage's conflict, and its range excludes the post's trailing blank line.
  - **Why the range matters:** a first version consumed that line, merged the next heading into the post and duplicated the preserved section. A content test now asserts the repaired text.
  - **Repeat rejection:** the check fails at once.
- **Framing guard (ruling 3):**
  - **The rule:** a reviewer edit inside the targeted post is refused when it removes a content word (letters only, at least 4 letters) of the revision instruction.
  - **The fallback:** the model repair is then told which terms to keep.
  - **Switch:** `MODEL_GATEWAY_REVISION_FRAMING_GUARD` (default on).
  - **Scope:** first turns and whole-document revisions are unaffected, and tests assert that.
- **Truthful counts:** `reviewPasses` and the turn-timing pass/batch counts now count a re-review of an identical candidate. Only scoped revisions mark their review ids with the attempt, so other requests are byte for byte unchanged.
- **Switches:** rulings 1–2 live under `MODEL_GATEWAY_REVISION_REVIEW_REUSE`. Off restores the full review and complete recheck.
- **Review:** a workflow review (three lenses, each finding verified by a refuting agent) confirmed 4 findings and refuted none; all are fixed. A separate review of the count rule and the boundary fix then found the swap gap, which is now fixed.
- **Known limit:** an edit that changes only a qualifier attached to an unchanged amount or name ("1 000 kr" becoming "1 000 kr etter rabatt") does not make a preserved segment dependent.
- **Tests:** model-gateway 1488 unit tests and 84 end-to-end tests pass, as does every integration suite, with no warnings.

### Third freeze: scoped-revision rulings build (2026-09-27T04:30:56Z)

Branch `fix/ui-premium-restore` at `21d636a5`, plus an uncommitted working tree. Nothing is committed, at the owner's request.

- **Working-tree fingerprint:** tracked diff sha256 `143968d6923ef4a6…`; 164 untracked files under `apps/`, sha256 of their hashes `dc72517f7b25da49…`.
- **Image IDs (sha256 prefixes):**

  | Service | Image |
  |---|---|
  | model-gateway | `e90d485701eb` |
  | inference-core | `44f5affaf4b7` |
  | session-core | `8dff4de75e53` |
  | capability-core | `2c82b6904594` |
  | cost-core | `c0c299fc169a` |
  | letta-bridge | `d2c2d785f4f6` |
  | execution-core | `341c7acc1681` |
  | integration-api | `aba8b80474cc` (unchanged) |
  | verevon-gateway-rs | `27cfb4596a8d` |
  | frontend | `38567be4a5b9` |

- **Effective switches (the gateway's startup log):**
  - author guidance `v1`;
  - review proposal rule `off`;
  - sole-artifact prefetch on (a new note's source, or a scoped revision's document);
  - recheck edits off;
  - balanced batches off;
  - revision review reuse on (with rulings 1–2, reused-review batching from 6 segments);
  - revision framing guard on;
  - broker as before.
- **Fixture hashes:** 13 of 13 match.
- **Smoke (journey 03, Terra): passed.** The revision took 51.3 s, with the prefetch and one reused review call. The first turn took 96.9 s, with one reviewer edit and the complete recheck.
- **Recovery gate on this build:** `product-recovery.spec.ts` passed 9/9, with evidence in `C:/private/verevon-recordings/gates-20260927/recovery`.
- **Series:** 20 attempts per journey, with the same procedure as Q16b and the final journey-02 oracle, under `C:/private/verevon-recordings/q16c-*`.

### Q16c series result on the third freeze (2026-09-27, 04:32–08:16 UTC)

80 attempts, plus top-ups on the same image (01 ×2, 03 ×3, 04 ×3, all passed), so every journey turn has at least 20 samples. Every attempt is kept under `C:/private/verevon-recordings/q16c-20260927T0432Z` and `q16c0{1,3,4}top-*`.

| Journey | Passed | Five consecutive at | First turn p50 / p95 (n) | Follow-up p50 / p95 (n) | Timeouts |
|---|---|---|---|---|---|
| 01 | 18/20 + 2/2 | attempt 8 | 28.6 / 64.8 s (22) | 33.9 / 54.5 s (21) | 0 |
| 02 | 20/20 | attempt 5 | 47.6 / 81.4 s (20) | 24.0 / 47.5 s (20) | 0 |
| 03 | 18/20 + 3/3 | attempt 5 | 69.0 / 113.0 s (23) | 44.1 / 74.7 s (21) | 1 |
| 04 | 16/20 + 3/3 | attempt 6 | 46.6 / 92.5 s (23) | 27.3 / 63.3 s (21) | 0 |

**Requalification:** all four journeys reached five consecutive passes on one frozen build, `sha256:e90d485701eb…`.

**Against the owner's targets:**
- Every first turn is within p95 ≤ 120 s.
- The follow-ups of 01, 02 and 04 are within p95 ≤ 70 s.
- **03's follow-up:** p95 74.7 s, against 117.4 s on the second freeze.
  - 2 of its 21 revisions are over 70 s: 78.2 s and 74.7 s.
  - In each, the reused review call rejected once: 39 s as a single call, 22 s as a batch. The scoped recheck that followed took 12 s and 11 s.
- **03 had one first-turn timeout.**

**The rulings at work, journey 03's 18 revision turns in the series:**
- **Reuse:** reuse held on the accepted pass in 18 of 18, reviewing 3–8 of 26–34 segments.
- **Rejections:** per-pass rejection fell to 14%, from 24%, with no rejection cascades.
- **Rechecks:** they reviewed only the targeted post and its dependents.
- **Fail-fast repair:** in attempt 17, a rejected preserved list was fixed by the one repair of the targeted post, and the turn passed.

**Failures (8 of 80 attempts):**
- **Rejection cascades on turns that keep the complete recheck:**
  - 03 first turn, attempt 6: `response_validation_failed` at 113.0 s;
  - 03 first turn, attempt 10: `response_validation_timeout` at 125.4 s;
  - 04 new note, attempts 1 and 12: each full recheck rejected a different segment.
- **Document-stage repairs exhausted:** 01 attempts 3 (first turn) and 10 (follow-up). Two whole-document repairs did not bring the draft inside its local check, most likely the customer-body word limit. The check is content-free and does not log which one.
- **Plan wording (guidance v1), 04:**
  - attempt 8 has no "earliest possible" date wording;
  - attempt 20 failed the same oracle.

**Rejection behaviour per turn type (share of passes rejecting, then cascades and failures):**

| Turn | Q16b | Q16c |
|---|---|---|
| 03 revision | 24%, 1 cascade, 1 failure | 14%, 0, 0 |
| 03 first turn | 26%, 0, 0 | 36%, 2, 2 |
| 04 note | 25%, 1, 0 | 30%, 2, 2 |
| 04 first turn | 50%, 5, 1 | 29%, 1, 0 |

The scoped path is now the most stable. The remaining failures sit on the complete-recheck path, where each full recheck re-assesses every segment and the reviewer rejects a different one at random.

### Gates on the third freeze (2026-09-27)

Evidence is kept under `C:/private/verevon-recordings/gates-20260927/`.

- **Durability: passed.**
  - `product-durability.spec.ts` (`Q16_CAPTURE_ROOT` = the Q16c root) passed 21/21 before a controlled restart of Model Plane Postgres, session-core, model-gateway and verevon-gateway-rs (08:17:56 UTC, same images), and 21/21 after it.
  - All 20 evidence sets are identical, ignoring timestamps.
- **Recovery: passed.**
  - `product-recovery.spec.ts` passed 9/9 on this build.
  - The live Q08 "reload resumes the same run without another invocation" check passed on the restarted system.
- **Experience: partial.**
  - `product-journeys-viewport.spec.ts` passed 9/9, and the live Q08 mobile, keyboard and reduced-motion check passed.
  - Captions and the editorial/media review remain.
- **Trust: partial.** Live on Terra, on this build, `product-memory.spec.ts` and `product-connected.spec.ts` passed 3/3:
  - an isolated task survives a reload, rejects widening and creates no personal memory;
  - memory correction and forgetting persist;
  - authenticated read contracts and quote provenance hold.
  
  That evidences no memory promotion and the temporary-chat policy. Cross-tenant leakage and expired-policy precedence are not evidenced yet.

### Owner ruling: in-turn recheck scope on every turn (2026-09-27)

**The ruling:** after an edit or repair within a turn, on ANY turn (first turns and new notes included), the recheck covers the edited segments and the segments that depend on them, under the same dependency rule as scoped revisions.
- **Kept approvals:** a segment the same checker approved earlier in the same turn, and that is still byte-identical, keeps that approval.
- **Still whole-document:** the deterministic local checks.

This replaces the "independent complete recheck" rule (README.md:12) for in-turn repairs everywhere.

**Why:** on the third freeze, rejection cascades on complete rechecks (each full recheck rejecting a different segment) caused 4 of the 8 failed attempts, including the only timeout: 03 first turns ×2 and 04 new notes ×2. The scoped path on revisions had 14% rejecting passes and no cascades.

**Implementation (uncommitted, model-gateway):**
- **The in-turn scoped recheck.** After a rejected pass in the same check, the next review is assigned against the previous reviewed candidate, and the local checks still cover the whole document.
  - **Always reviewed:** changed segments, their dependents (the same section, or a shared changed date, amount or name) and every segment not yet approved.
  - **Carried:** everything else, with its evidence.
  - **Offered prior approvals:** none. Assigned segments are reviewed fresh and are never offered an earlier approval to confirm.
  - **Accumulation:** approvals accumulate across attempts.
  - **Fallback:** a source or binding mismatch falls back to the complete recheck.
- **Composed with scoped revisions:** the in-turn layer may narrow only a revision's preserved dependents. A review found that a targeted post with an internal sub-heading could otherwise be partly carried away; this is fixed with a regression test.
- **Switch:** `MODEL_GATEWAY_INTURN_SCOPED_RECHECK` (default on); off restores the complete recheck.
- **Batch threshold:** the reused-review batch threshold is now 3 segments. A rejecting single call took 39 s, against 22 s for a rejecting batch.
- **Diagnostics:** every document-stage local-check failure now logs a content-free line with a category and numbers (words, minimum, maximum, piece), so that journey 01's repair exhaustion can be diagnosed.
- **Receipts:** a new `carriedInTurnSegments` field keeps reviewed + reused + carried equal to the segment count.
- **Review:** a workflow review (three lenses, refuting verifiers) confirmed 3 findings and refuted 1; all 3 are fixed. Tests: model-gateway 1499 unit tests and 85 end-to-end tests pass, as does every integration suite.
- **Smoke on that build:** journey 03 passed, with its revision at 40.9 s and the reused review split into 2 batches of 3. Journey 04 failed: every in-turn recheck of its table-heavy plan still covered all 13 segments.
  - **Cause:** "same section" used `section_context`, which hashes the section's whole content, so one edited row made every row dependent.
  - **Fix:** the in-turn dependency now uses `heading_context`, meaning a segment's heading chain plus, for a table row, its table's header row.
  - **Test:** a test with a mutation check asserts that one edited row rechecks only that row, while an edited header or heading rechecks what it governs.
- **A second review of that rule found two dependencies the entity rule misses, now added (both only add reviews):**
  - a total row ("Totalt", "Sum", "Samlet", "I alt") is rechecked whenever another row of its table changes;
  - a passage that directly follows an edited one under the same heading and opens with a conclusion ("Derfor …", "Dette betyr at …") is rechecked.
  
  A pipe-leading paragraph after a table no longer counts as a row of it.
- **Tests:** a flaky log-capture test now rebuilds the tracing interest cache. model-gateway 1501 unit tests and 85 end-to-end tests pass, with no warnings.

### Fourth freeze: in-turn scoped recheck (2026-09-27T23:12:35Z)

model-gateway `82fe303492e7`, all switches on, including `in_turn_scoped_recheck`.

- **q16d, first attempt:** stopped by a host reboot at 23:21:36Z after one passed take. The reboot also emptied the in-memory session store, so the stored test sign-in stopped working. It was refreshed with the owner's authorisation (localhost fixture account).
- **q16d, second attempt:** 01-kundesvar passed 20/20 (qualified at attempt 5). The series was then stopped by hand during 02 (attempts 1–3 passed).
  - The first recorded reason, that frontend edits were live in the app, was wrong. The frontend container runs Vite dev with watching disabled, so the served modules held none of the edits. The corrected record is in `freeze4b.txt` in the session scratchpad.
  - 02 attempt 4 failed because the chat page stayed "Loading…" for more than 10 s after the durability reload. That was while Wave-1 agents were running heavy local builds and tests: CPU contention, not a product defect.
- Freeze 4 was superseded by freeze 5, and no further samples were taken on it.

### Fifth freeze: chat Wave 1 (2026-09-28T02:01:25Z)

Chat Wave 1 is live:
- Office export from answers and from the Resultat panel;
- highlighted code blocks;
- per-thread rename and delete;
- .xlsx and .pptx uploads;
- Outlook, Gmail and OneDrive read tools.

Images:
- model-gateway `d5291be06e53` (a workspace-layer rebuild; model-gateway source is unchanged);
- execution-core `4dd474a46002`;
- imports-api `5fa4b1fd08c4`;
- frontend `e731f4ec62a3`.

Every switch is unchanged; fixtures 13/13; tracked diff `1f22140d6970d086`.

**Q16e series, 02:02–05:17 UTC**, 20 attempts per journey on gpt-5.6-terra via openai-codex-subscription:

| Journey | Passes | First turn p50 / p95 | Follow-up p50 / p95 | Timeouts |
|---|---|---|---|---|
| 01 kundesvar | 20/20, qualified at 5 | 29.8 / **39.3 s** | 28.7 / **47.2 s** | 0 |
| 02 salgsrapport | 20/20, qualified at 5 | 47.8 / **70.2 s** | 23.1 / **33.6 s** | 0 |
| 03 kampanje | 20/20, qualified at 5 | 72.4 / **105.4 s** | 43.0 / **76.3 s** | 0 |
| 04 prosjektplan | 18/20, qualified at 5 | 43.7 / **75.8 s** | 26.2 / **74.6 s** (n=18) | 0 |

- **Every first turn is inside its 120 s target, and there were no timeouts.** First-turn p95 against freeze 3: 01 64.8 → 39.3 s, 02 81.4 → 70.2 s, 03 113.0 → 105.4 s, 04 92.5 → 75.8 s.
- **Follow-up p95 is over its 70 s target on 03 (76.3 s) and 04 (74.6 s).**
  - All three follow-ups over 70 s are repair turns. The draft is ready at 11–32 s, the first source review rejects a segment, and the repair plus re-review takes another 45–60 s. Two of them are in 03 (76.3 s and 90.9 s, six provider calls each) and one in 04 (74.6 s, three review passes).
  - The tail is review-repair time, not drafting.
- **04 attempts 18 and 19** failed the plan oracle: no stand-in, reserve or buffer concept was named. This is the known plan-wording class under author guidance v1, not a timing failure. 04 therefore has 18 follow-up samples and needs two top-ups.
- **03 revision reuse** applied on all 20 accepted passes: 3–8 segments reviewed and 18–26 reused.

Status is still **blocked** on follow-up p95 (03, 04). Chat Wave 2 is being built next, so the full Q16 procedure (series, top-ups, durability, recovery, viewport, Q08, trust) will run on the freeze that follows it rather than on freeze 5.

### Q16f: short series with the Wave-2 build and three review-latency switches (2026-09-28, 18:14–19:10 UTC)

This is a partial series (10 takes each of 03 and 04), run to check the latency switches before more work went on top. It is not a qualification.

**Build:**
- model-gateway `981d0dca282e`;
- session-core `607562c1491e`;
- execution-core `3258f3e55555`;
- integration-api `057a6e8dab9b`;
- imports-api `c224ac4f5883`;
- verevon-gateway-rs `1c349a72f093`;
- frontend `8e71a4a03094`.

**Switches (all on):**
- `sole_note_prefetch`;
- `revision_review_reuse`;
- `revision_framing_guard`;
- `in_turn_scoped_recheck`;
- `tiny_candidate_skips_propose_edits` (a candidate of fewer than 3 segments goes straight to repair);
- `targeted_source_repair` (the repair request carries only the failed segments);
- `list_item_segments` (bullet lists are reviewed item by item, like table rows).

`recheck_proposes_edits` and `balanced_review_batches` stay off.

| Journey | Passes | First turn p50 / p95 | Follow-up p50 / p95 | Follow-ups over 70 s |
|---|---|---|---|---|
| 03 kampanje | 10/10 | 75.4 / 92.2 s | 47.5 / **75.6 s** | 2 of 10 (72.0 s, 75.6 s) |
| 04 prosjektplan | 9/10 | 46.6 / 74.9 s | 30.4 / 47.0 s (n=9) | 0 |

At n=10, p95 is the maximum. No timeouts. The one failure in 04 was the known plan-wording class (no stand-in or buffer named).

**Repairs:**
- 03: none of the ten follow-ups needed a repair, against all three slow ones in the freeze-5 series.
- The samples are small and this is not a controlled A/B, so read it as consistent with the switches helping, not as proof.

**What the two slow 03 follow-ups were.** Neither was a repair.
- **Take 7 (75.6 s), a wasted retry.**
  - The first draft attempt ran 32 s and failed in inference-core with `invalid subscription tool arguments`.
  - The fallback retried the same provider and needed another 32 s.
  - Cause: on the subscription route tools are emulated. The model returns an envelope whose `arguments` field is itself a JSON string, and one bad character in a large Norwegian document body fails the whole attempt.
  - Rate: one failed attempt in about 40 tool-emitting calls in this window. Freeze-5 container logs were replaced by the rebuild, so that rate is unknown.
- **Take 2 (72.0 s), a heavy review batch.**
  - The draft was ready at 32 s. One of two parallel review batches, four segments and 966 bytes, spent about 3,000 reasoning tokens (39.5 s), against 283 tokens (8.3 s) in the other batch.
  - The review's thinking budget is deliberately 4,096 tokens; it is not being lowered without an A/B on the saved 104-case matrix.

With 20 samples, p95 by nearest rank is the second-largest value, so one slow turn in twenty is tolerable and two is not. The wasted retry is therefore the first thing to fix (tolerant parsing of unambiguous JSON damage, in progress). Splitting review batches finer is the second candidate, to be A/B-tested for correctness first.

## Q16h and Q16i: the model sends only the revised post (2026-09-29)

**What changed.** On a recognised revision of one dated post ("Gjør innlegget for 23. september ..."), the model is told to send `update_artifact` with only that post's section, and the gateway splices it into the accepted document. Stage 1 of the [patch-revision plan](../../system-audits/patch-revision-2026-09-29/00-critic-verdict-and-staged-plan.md). Before this, the follow-up rewrote about 3.8 KB to change about 0.4 KB.

**Q16h: 20 takes of journey 03, switch on, all other levers as in freeze 5.** Same Terra subscription route, no automatic retry, corrected oracle (see below).

| Measure | Q16h | Baseline (whole document) | Owner target |
|---|---|---|---|
| Passes | 20/20 | 20/20 (q16e) | |
| Follow-ups that came back as a section | **20/20** | not applicable | kill criterion: under 90% |
| Follow-up p50 / p95 / max | **26.7 / 41.6 / 43.8 s** | 47.5 / 75.6 s (q16f, n=10); 76.3 s p95 (q16e, n=20) | p95 ≤ 70 s |
| Follow-ups over 70 s | **0** | 2 of 10; 2 of 20 | |
| First turn p50 / p95 / max | 73.5 / 92.4 / 93.1 s | 105.4 s p95 (q16e) | p95 ≤ 120 s |
| Timeouts, repairs, `invalid subscription tool arguments` | none | | none |

The model wrote 474 to 623 characters (median 530) against a stored document of about 3.6 KB; the draft was ready at a median of 14.3 s, against about 32 s. Most follow-ups needed one review pass and a few two; none needed a repair. These are the numbers the [patch-revision analysis](../../system-audits/patch-revision-2026-09-29/00-critic-verdict-and-staged-plan.md) projected (follow-up p50 about 24 s, p95 57 to 60 s); the measured p95 is lower.

**Oracle correction, made before this series.** The first attempt (folder `q16g-20260929T1458Z`, kept as evidence) failed take 1 with "piece 1: 7 words" although the stored document was complete. The model had put a metadata block, including the "Produktinformasjon teksten bygger på" note, above each post, and the oracle cut the piece at the first source note. `tests/e2e/product-campaign-oracle.ts` now removes a leading metadata block first (8 tests). Replayed over all 62 recorded journey-03 documents: none that passed now fails, the failing one now measures 63, 63, 61 and 80 words. The oracle no longer counts the "Kanal" and "Foreslått dato" lines as copy, so most posts count 2 to 7 words fewer than in q16e and q16f: **q16h is judged slightly stricter than those series**, and several posts sit at 60 to 62 words against the 60-word minimum.

**Defect found by reading the documents, not by any check.** When the accepted post has a `### Kilder` sub-section (the per-post source note the brief asks for), the splice replaced the post's whole section with the model's, and the model had sent only the heading and text. In q16h that happened in **3 of 3 takes that had the layout** (takes 1, 7 and 9 of 20); with whole-document output the note survived in 6 of 6 earlier takes. The copy checks passed all three, because they measure the customer copy. The source note is exactly what the brief asks the drafts to show. Fixed in three parts:
- **Told:** the section-only notice is followed by one message naming the post's sub-heading lines.
- **Refused:** with the switch on, a section that comes back with none of the accepted post's sub-sections is rejected and repaired, and the repair is shown the note it lost. `apply` itself is unchanged.
- **Judged:** the journey-03 oracle fails a take in which a post that showed a note no longer shows one (`lostPostNotes`; 13 tests in all). Replayed over the recorded documents it flags exactly the three q16h takes and none of the 47 others.

**Offline evidence for the fix** (gateway lib 1591 passed, integration 93 passed):
- unit tests for the two helpers and the check, with the lost note quoted in the repair;
- two mocked end-to-end tests: the author call names the sub-heading and a section with the note is spliced; a section without it is never emitted or saved, and with the switch off it is spliced as before;
- a corpus test over the 50 recorded revisions: 9 had a note, and the refusal fires on exactly the 3 that lost it;
- mutation checks: disabling the check, and separately the notice, fails the tests.

**Q16i: not finished. The live proof of the fix is outstanding.** The compose default is now `MODEL_GATEWAY_DATED_POST_SECTION_ONLY=1`, the images were rebuilt through the launcher, and 20 takes were started on that configuration (`q16i-20260929T1706Z`). Takes 1 and 2 passed (follow-ups 25.9 s and 23.7 s), but neither had the per-post note layout, so the fix was not exercised. **Take 3 stopped with `subscription_usage_limit`: the connected ChatGPT plan has reached its Codex usage limit and resets 2026-10-03 17:45 UTC.** The series stopped by its own guard and nothing was run on another provider. What Q16h proves (adoption, timing) was measured before the fix; the fix only affects turns whose target post has a sub-section.

**Q16j: the live proof of the fix (2026-09-30, after the owner reset the Codex limit).** The host had rebooted in between (containers restarted, the test sign-in was emptied and restored with the authorised local setup), and the owner had committed two marketing-site font changes. Neither touches the chat app, the gateway or the fixtures (13 of 13 hashes match). The images are the same as in q16i. Frozen 13:09 UTC, run 13:10 to 14:09 UTC, 20 takes of journey 03, switch on by the compose default.

| Measure | Q16j | Owner target |
|---|---|---|
| Passes | 20/20 | |
| Follow-ups that came back as a section | 20/20 | kill criterion: under 90% |
| Follow-up p50 / p95 / max | **25.3 / 57.2 / 58.5 s** | p95 ≤ 70 s |
| Follow-ups over 70 s | **0** | |
| First turn p50 / p95 / max | 68.9 / 102.8 / 105.8 s | p95 ≤ 120 s |
| Timeouts, repairs, guard refusals, tool-argument errors, usage-limit events | none | none |

- **Notes:** 4 of the 20 documents had the per-post `### Kilder` layout, and **all 4 kept their note** after the revision (Q16h: 0 of 3). The notice alone was enough; the refusal never fired, so its live path is still exercised only by the offline tests. In those four takes (2, 4, 18 and 19) the model wrote 623 to 818 characters instead of about 500, which is the note travelling with the post. Two of them (2 and 18) also changed the note's text to match the revised post; the other two sent it unchanged.
- **Oracle:** the new note check flags none of the 20 documents; the structural diff shows the headings unchanged in every take.
- **Reuse:** all 20 revisions reused earlier approvals on the accepted pass. They reviewed 3 to 12 segments (median 4) and reused 17 to 32.
- **Tail:** the two slowest follow-ups (58.5 s and 57.2 s) were review time after a normal 14 to 16 s draft, one with four review batches and two passes. That is the review's reasoning budget again, not the draft; it is within target and not touched.
- **Against the baseline:** follow-up p95 57.2 s against 76.3 s (q16e) and 75.6 s (q16f); the first-turn p95 of 102.8 s is about the same as q16e (105.4 s).

Journey 03 of this pass is this series. Journeys 01, 02 and 04 ran next on the same build (`q16k`).

## Q16k: journeys 01, 02 and 04 on the same build, and two timeouts (2026-09-30)

Frozen 13:09 UTC (the Q16j freeze), run 14:09 to 16:17 UTC, 20 takes each.

| Journey | Passes | First turn p50 / p95 / max | Follow-up p50 / p95 / max | Timeouts |
|---|---|---|---|---|
| 01 kundesvar | 19/20 | 30.7 / 39.3 / 39.9 s | 30.4 / 43.5 / 43.5 s (n=19) | 0 |
| 02 salgsrapport | 19/20 | 58.5 / 68.2 / 68.2 s | 29.6 / 50.2 / 50.2 s (n=19) | 0 |
| 04 prosjektplan | 17/20 | 43.3 / 109.5 / 111.6 s | 31.1 / 42.9 / 42.9 s (n=17) | **2** |

All timing targets hold. Two targets do not: **journey 04 had two timeouts** (the series must have none), and three journeys are short of 20 follow-up samples because a failed first turn has no follow-up.

**The five failures:**
- **01, take 16: `customer_body_sections_missing`.** The draft was refused twice by the local check that wants one customer body and one internal note section, and the second repair came back unchanged. A model-output failure at 31 s; the draft is not in any log, so the cause is unknown. A content-free diagnostic now records how many internal-note headings the draft had (none, or two or more, both make the check say "missing").
- **02, take 19: the chat page never rendered.** The route sat on "Loading…" for the 10 s wait for the composer, before anything reached the model. The wait is now 30 s. It is test code only.
- **04, take 17: plan wording** (no stand-in or buffer named), the known content failure.
- **04, takes 7 and 9: `response_validation_timeout`, both at about 110 s.** The two had the same shape. The draft was ready at 19 to 22 s. The first source review of 15 segments ran in 3 batches of 4, 4 and 7 segments. The 7-segment batch (1,404 to 1,474 bytes, table rows) spent 4,051 to 4,580 reasoning tokens and took 52.5 and 60.7 s, while the other two finished in 10 to 18 s. Three table rows were unsupported, so a repair (14 to 15 s) and a second review (17 s) followed, and the 90 s the source check gets from the moment the draft is ready ran out in the next repair. The same tail was there in freeze 5 (a 109.7 s first turn that just finished).

**What drives the tail.** Across 258 review batches of the q16k window: median 12.1 s, p95 28.9 s, 3 over 40 s, and the two slowest (53 and 61 s) were the two 4,000-plus-token calls. Batches of 7 or more segments had a 33.0 s p95 and 8 of 137 over 30 s; smaller ones 21 s and 3 of 121. Journeys 02 and 04 also review more than in freeze 5 (second review pass in 40 to 47% of first turns against 25 to 30%), which fits `list_item_segments` splitting bullet lists into items.

**Finer review batches, A/B on real plans (2026-09-30).** 10 real journey-04 plans (whose batching the new rule changes) and a copy of each with one unsupported statement inserted, 3 runs per arm, the production configuration, Terra subscription only:

| | Old rule (3 batches) | Finer rule (4 batches) |
|---|---|---|
| Real plans accepted | 20/30 | 20/30 |
| Seeded defects rejected | 30/30 | 30/30 |
| Protocol errors | 0 | 0 |
| Review time p50 / p90 / max | 26.0 / 33.1 / 48.5 s | 25.6 / 32.7 / 48.2 s |
| Batch calls with 2,000 or more output tokens | 10 of 180 (5.6%), max 3,716 | 1 of 240 (0.4%), max 2,745 |
| Median output tokens per call | 995 | 688 |

The two arms judge alike. Wall-clock review time did not differ in this sample, because provider latency varies independently of tokens (the same plan took 18, 19 and 27 s in one arm and 48 s once in the other), and one-pass reviews rarely meet the tail. The reasoning-token tail did shrink 14-fold. That is the mechanism behind the two timeouts, so the rule is now on; the next series is its real test. The low acceptance of real plans by a single complete pass (20 of 30 in both arms) is the same review variance that causes the repair loops.

**Changes made from this, all in the gateway and compose (tests: lib 1600, integration 93):**
- `MODEL_GATEWAY_FINER_REVIEW_BATCHES` on by default (one batch per four segments or 1,400 bytes of weight, at most four at once).
- `MODEL_GATEWAY_ARTIFACT_CHECK_TIMEOUT_SECS` 120 instead of 90, because take 7 needed about 110 s of checking even with its heavy batch halved. The 150 s limit on the whole tool phase still applies, and an independent timeout audit found no upstream limit that a 120 s check would trip.
- Hardening from an independent three-lens review: the sub-section guard now counts only `#` headings (a bold hook line in a post was counted as a sub-section), a request that also removes the note or the sources stays on the whole-document path, and the section-only notice no longer says not to send "the sources or the notes" while another message says to keep the post's note.

## Q16L: the final pass on the new configuration (2026-09-30)

Frozen 18:06 UTC on the model-gateway image `1ff7c6a268d6` (finer review batches on, section-only on, 120 s source check, the two review levers off, guidance v1, 13 of 13 fixture hashes). Journeys 04, 03, 02 and 01, 20 takes each, run 18:07 to 21:16 UTC, plus a four-take top-up of journey 02 (`q16Lb`, 21:16 to 21:26). The frontend container was deliberately not restarted: another session had uncommitted frontend edits, which a launcher rebuild would have baked in.

| Journey | Passes | First turn p50 / p95 / max | Follow-up p50 / p95 / max | Samples (first / follow-up) | Timeouts |
|---|---|---|---|---|---|
| 04 prosjektplan | 20/20 | 46.3 / 67.1 / 121.1 s | 28.0 / 47.0 / 106.5 s | 20 / 20 | **0** |
| 03 kampanje | 19/20 | 79.2 / 105.7 / 108.9 s | 28.6 / 57.7 / 61.8 s | 20 / 20 | 0 |
| 02 salgsrapport | 17/20, plus 4/4 top-up | 53.4 / 65.0 / 73.0 s | 23.5 / 46.0 / 46.0 s | 20 / 21 | 0 |
| 01 kundesvar | 20/20 | 32.7 / 43.0 / 55.5 s | 28.8 / 45.3 / 73.2 s | 20 / 20 | 0 |

Against the owner's targets: first-turn p95 ≤ 120 s holds on all four, follow-up p95 ≤ 70 s holds on all four, **no timeouts in 84 takes**, and at least 20 samples per journey turn. Nearest-rank p95 at n=20 is the second-largest value, so one slow turn in twenty is tolerated: journey 04 has one first turn at 121.1 s (three review passes and two repairs, the loop that timed out at 110 s in q16k) and one follow-up at 106.5 s (three passes, one repair), and journey 01 has one follow-up at 73.2 s.

**Journey 04, the point of the changes:** 20/20 with no timeout, against two timeouts and a 109.5 s p95 in q16k. Across 80 review batches the largest took 32.5 s (none over 40 s), the largest batch held 4 segments, and the most reasoning in one call was 2,401 output tokens, against 4,580 and 60.7 s before. The 121 s turn completes because the source check now has 120 s; it would have been a timeout at 90 s.

**The four failed takes, all model behaviour, none a timeout, none a gateway fault** (when I checked mid-series, the Codex route had returned 200 for all 410 inference calls in the integration-api log, and inference-core had logged no warnings):
- **03, take 3 (revision turn):** after one repair the recheck still found unsupported paragraphs and the check gave up within its attempt limit, at 62 s.
- **02, take 6 (first turn):** a local document check failed on one segment and two short repairs did not clear it; no review pass ran.
- **02, take 8 (first turn):** the model's first `create_artifact` call carried only an `id`; the gateway returned a schema error, the model recovered and the turn completed at 73 s, but the recording oracle counts any tool error as a failure.
- **02, take 12 (first turn):** the model wrote the report as chat text and made no artifact call.

Journey 02 had one model failure in the previous 40 takes and three here, clustered within 15 minutes. The earlier series also show occasional 02 failures (q16b kept several), and 01 and 04 ran clean on the same build. Nothing in these classes is touched by this window's changes (they occur before any review or splice), so I read this as route variance and will say so to the owner, not as a regression.

**The sub-section fix held again:** 11 of the 20 journey-03 documents had a per-post `### Kilder` note, all 10 that completed the revision kept it, and the refusal never fired (the notice was enough); the oracle flagged none.

**The gates** (`run_gates.sh`, on the Q16L capture root, 21:26 to 21:32 UTC, Terra subscription only, `--no-deps`):

| Gate | Result |
|---|---|
| Durability before the restart | 21 passed |
| Controlled restart of Model Plane Postgres, session-core, model-gateway and verevon-gateway-rs | healthy after 10 s |
| Durability after the restart | 21 passed; evidence identical in 20 of 20 files (timestamps ignored) |
| Recovery (`product-recovery.spec.ts`) | 9 passed |
| Viewport (`product-journeys-viewport.spec.ts`) | 9 passed |
| Live Q08 reload-resume and mobile markdown | 2 passed |
| Trust (`product-memory.spec.ts`, `product-connected.spec.ts`) | 3 passed |

**Where this leaves the four gates that were open.** Performance now meets every owner target on one build; the manifest says so (`performance: passed`). Correctness stays partial: 4 of 80 final-pass takes failed on model behaviour, and the 04 plan-wording class under author guidance v1 is not addressed. Trust stays partial because the cross-tenant spec (`real-authority-knowledge.spec.ts`) needs password fixtures only the owner can run. Experience stays partial until the captions and the editorial and media review are done. The overall status stays `blocked` on those owner-side items, and no public media is approved.

**Still to do (owner):** captions, the editorial and media review, the cross-tenant spec, and whether to widen section-only beyond the "Gjør innlegget for D. måned" phrasing (decision D2 in the plan).

## Build drift after the final pass (2026-10-01)

The final-pass build (model-gateway image `1ff7c6a268d6`) is no longer the running build. The model plane was redeployed twice on 2026-10-01 from the working tree for the chat gap-closure plan (batch 1 at about 09:00 UTC, batch 2 at 12:05 UTC): typed subscription failures, the run-level refusal latch, agentic stream follow, server-side run budgets, Do mode on the Terra route (all behind `AGENTIC_SUBSCRIPTION_ENABLED`, off), and two switches that turn off background features which called paid models (capability-core learning review, session-core Dreaming extraction). None of this touches the document journeys' path by design, and every new behaviour is off by default, but the Q16 numbers above were measured on the earlier build. **Before any release or public recording, re-run the journeys on the build that will ship** (a short series is enough to catch a regression; the full five-pass rule still applies for a release freeze).

**Smoke check on the final build of 2026-10-01 (23:20 to 23:30 UTC, label q17smoke):** one rehearsal take per journey on Terra after all four planes had been redeployed (Control, Model, Ingestion, Frontend), no retry. All four journeys passed (4 of 4). First turns took 29.1 s (01), 53.8 s (02), 81.3 s (03) and 48.5 s (04); follow-ups took 47.8 s (01, one repair), 27.2 s (02), 38.6 s (03) and 22.7 s (04). No timeout and no turn over the owner targets (first turn at most 120 s, follow-up at most 70 s). This is one take per journey, so it is a regression check and not a qualification: the five-pass rule and the 20-sample p95 still have to be re-run on the build that ships.

## Owner action after the rebuild: delete the legacy prompt logs

After the rebuild, and once the smoke turn shows no new `logs_2.sqlite` is written, the **product owner** deletes the legacy `logs_2.sqlite` files (and the legacy Codex session transcripts) in the connection homes on the `integration-codex-subscription-home` volume. They hold full prompts and attachment content back to 21 September. The new startup sweep removes only `.ephemeral` directories, not these files. This is permanent deletion. **It is never done by an agent.**

## Next gates

- After the freeze, start Q16: five consecutive complete passes per scenario on the frozen build, the same Terra subscription route and unchanged fixtures, with no automatic retry. A failure resets that scenario's streak. Earlier private films and this validation do not count.
- Measure Q14 cohorts from `turn_timing` with at least 20 samples per workload, keeping failures, timeouts and repair frequency. Judge plans against the target the owner restates.
- Before any switch to the direct transport, obtain OpenAI's confirmation for hosted multi-user use and run its live checks.
- Recovery, restart/new-browser durability, device/accessibility and editorial/media review remain open, as in [pass 6](PRODUCT_RECORDING_CAPTURE_PASS6_2026-09-23.md).

See the [next-steps plan](PRODUCT_RECORDING_NEXT_STEPS_2026-09-20.md), [maintained recording plan](../README.md), [workflow](../RECORDING-WORKFLOW.md) and [manifest](../manifest.json).
