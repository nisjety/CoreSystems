# QM architecture deep dive (Verevon Space audit 1, facet: architecture)

Source: QM, open source TypeScript, "a multiplayer agent harness for work, in Slack and on the web" (C:/dev/qm). Snapshot: branch main, HEAD 1e975255 dated 2026-10-12; 894 commits, 821 of them after 2026-08-13 (the date Verevon's comparison baseline d719f540 was taken). Companion file: qm.architecture.features.json (159 records, ids qm-architecture-001 to 159, cited below as [#n]).

Provenance tags: SRC = read in source or config; DOC = documentation or a git commit subject; INF = my inference with stated basis; REA = not used (this is a source repository, no shipped binary was analysed). Confidence is high unless stated.

## 0. Method and verification

- Starting point was the scout inventory (qm.inventory.md). Everything below was re-read in source or docs. All 663 file:line citations in the JSON were machine-checked for file existence and line range (0 problems). About 60 numeric or mechanism claims were spot-checked against code (limits, timeouts, defaults, table keys); all held except one that I corrected (see 0.1).
- Read-only: nothing in C:/dev/qm was built, run or modified; git was used for log, show and rev-list only. No credential or user-data stores were read; only .env.example was seen by the scout (names only).
- No REA analysis was applicable. Negative results (no calls, no push, no E2EE, no service worker, no SCIM, no slash commands) are grep-based and can miss renamed code; each negative record says so.

### 0.1 Corrections to the scout inventory and to my own earlier draft

| Item | Scout or draft said | Verified state |
|---|---|---|
| AG-UI events | named in a commit, not found by grep | SRC: RUN_STARTED, TOOL_CALL_START, TOOL_CALL_RESULT, RUN_FINISHED are in src/api/routes/run-events.ts; the string "AG-UI" is absent but the envelope is AG-UI-compatible [#56] |
| Retention | retention module exists, UI tabs removed | SRC: src/admin/retention.ts is a DAU/WAU/MAU cohort report, not data retention; QM has no data-retention or legal-hold controls [#75] |
| Calendar tab | listed as shipped (git 03a793fa) | SRC: plugins/web-ui/src/calendar.ts renders "Coming soon"; there is no calendar feature [#156] |
| Skill sync | docs say tracked sync is a later phase | SRC: createSkillSyncEngine is implemented and wired (src/wiring.ts:2586); docs are stale [#29] |
| Slack transport default | unknown | SRC: Socket Mode by default, HTTP events opt-in [#157] |
| Tool ledger | my first draft: "exactly-once across retries" | SRC: the ledger key includes the claim attempt, which increments on every claim, so it only protects replays inside one claim; cross-claim duplicates rely on turn.already_answered replay and partial-turn resume [#54] |
| Forced egress | first draft: only E2B and Modal lock to the proxy | SRC: Sprites, E2B, Modal, DigitalOcean, smolmachines and Porter can; AWS, local Docker, Agent37 and Superserve report none [#25, #145] |
| Fastify | scout: core uses Fastify | SRC confirmed: src/api/server.ts uses Fastify as the HTTP shell with a catch-all into a declarative Route table (auth modes either, source, public, or an audience) |
| Sentry "sanitized" | commit 73fea1e1 | SRC/DOC: later commit ccc7bee9 removed scrubbing on purpose; docs say events are sent unredacted [#139] |

## 1. Executive summary

1. QM is a durable, Postgres-centred orchestration layer around swappable agent harnesses (Pi, Claude, Codex, OpenCode), swappable computers (ten backends) and swappable models. Its organizing object is the scope (personal, channel, group, team, org). A single resolver composes everything a turn may see from the scope [#1, #4, #8, #22].
2. The agent has no identity of its own in a room. It acts as the person who spoke, and every authorization is rechecked against that live person. Unattended work (crons, webhooks, loops) re-authorizes at every fire [#2, #43]. This is the opposite of the "agent as member" model Verevon intends.
3. The strongest ideas are not the features but the invariants: tighten-only policy cascade, audience-intersection floor, provenance label on every transcript entry, human-only authority walls, secrets the agent never sees, per-session serialized durable runs with leases and fencing, and a capability profile that lets a backend refuse rather than silently substitute [#3, #4, #5, #12, #14, #21, #51].
4. Chat itself is not a QM subsystem. Slack is the human system of record; QM keeps the agent's conversation log and mirrors Slack history into a neutral cache. The web UI is a second projection of the same sessions [#87, #88, #110]. There are no DMs, channels, presence, typing, read state, calls, calendar, push, mute or DND of QM's own.
5. Realtime is deliberately modest and well engineered where it exists: SSE run streams with byte-offset deltas and client-side recovery, Postgres LISTEN/NOTIFY as wake-up hints, a durable delivery outbox, ack-after-durable-staging for Slack events [#56 to #61]. There is no offline cache, no push gateway, no E2EE, no multi-device sync engine [#67].
6. Security posture is thoughtful but narrow: single-organization by design, regex command policy admitted to be bypassable, fail-open screening on outages, admin reads without consent, no retention controls, browser actions outside approval gates, plaintext credentials while in use [#72, #75, #78, #135]. SECURITY.md lists these honestly, which is itself a pattern worth copying.
7. Deployment is the multi-tenancy answer: each organization runs its own stack from a committed deployment directory, with a CLI (22 commands), Terraform for AWS, Fly, Docker, Helm and Porter runbooks, digest-pinned images, restore-point proof and a live canary gate [#82 to #84, #154].
8. The repo is unusually candid about its own mistakes. docs/SPEC.md has a "wall of shame" of 17 lenses and about 75 stated examples (overengineering, band-aids, duplication, hand-rolling, non-durability, config-matrix growth, legacy preservation, regex, god files). CI fails a PR until independent reviewers have scored each lens [#81, #152].
9. Scale of the code: about 551 source files and 139K lines in src, 4,565 lines in the orchestrator, 3,082 in wiring, 1,733 in config with about 285 env knobs, 62 CREATE TABLE statements, 712 root test files (1,107 including plugins, cli and desktop). The maintainers themselves flag config-matrix and provider-drift growth [#22, #64].
10. New since the August baseline (821 commits): swarms, persistent subagents, a responsive coordinator "spine", goals with an independent governor, team scopes, Inbox Loops, ambient channel judge, Electron desktop, DigitalOcean and Superserve backends, Lambda MicroVM computers, per-person AI accounts, browser model grants, document inputs, runtime handoff, and the dev-instance harness. Most are feature-flagged [#30 to #35, #47, #93, #112, #131 to #156].
11. Best fit for Verevon Spaces: the room-aware agent primitives (explicit surface tool, reach with membership bounds, ambient judge with standing orders, scope-labelled context, approvals as cards, recipient consent), the egress and sandbox capability model, the scheduling contract, and the engineering process. Not a fit: its trust model, regex policy, unredacted telemetry, coarse budgets, Slack-centric chat, no realtime collaboration.
12. Verevon's August comparison covered eight subsystems well. It did not cover swarms, persistent subagents, goals and the governor, team scopes, per-speaker AI accounts, the browser model grant, the explicit surface tool, admission gating, runtime handoff, document inputs, model verification, the session tape and the dev-instance and review process. Section 16 lists these.

## 2. System anatomy

SRC unless stated.

```
Slack (Bolt, Socket Mode by default; HTTP events opt-in)        Browser (Lit + Vite SPA)      Electron shell (remote web app)
        |  in-process plugin, supervised by core                        |                            |
        |                                                  web-ui server (node:http BFF, admin module embedded)
        |                                                               |  HMAC-signed source requests, portal identity header
        |                                                       portal (public front door, SSO, sealed sessions, auth broker module)
        v                                                               v
   +-----------------------------------  core (Fastify shell, declarative route table, /v1/*, 307 path literals) --------------------+
   |  turn admission: rate limit, budget, identity, idempotency, wake routing (steer/engage/drop)                                       |
   |  orchestrator (4.5K lines): resolve scope -> assemble context -> harness router -> tools -> commit to session tape                  |
   |  durable workers (run queue, leases) | scheduler (pg-boss) | loops | monitors | delivery outbox | admin/audit | keychain | deploy   |
   +------------------------- Postgres (runs, tape, DurableMaps, pg-boss, memory revisions, audit) + S3 (blobs, snapshots) -------------+
        |                                  |                                           |
   harness adapters                  sandbox backends (10)  <-- capability tokens --> Envoy egress proxy + ext-authz decision service
   (pi, claude, codex, opencode)     sprites, smolmachines, e2b, modal, porter, agent37, superserve, do-managed-agents, aws microVM, local docker
```

Process topology in production: three application workloads (core, web-ui with admin embedded, portal with the sign-in broker embedded) plus an optional egress proxy [#147]. Slack runs inside core. Two core deployments (blue and green) can share one database, and exactly one runs background work at a time [#63].

## 3. The design decisions that define QM

D1. Scope is the room. A conversation maps to a scope (DM to personal, channel to channel, group DM to group, plus team and org). The resolver returns one Resolution: workspace layers (org and team read-only, scope read-write), composed system prompt, egress floor, command policy, security posture, sharing posture, approval grant modes and granted resource handles. Config is refreshed from the durable store before each resolve [#1, #24]. Weakness: ids are strings of the form kind:ref; Verevon plans typed SpaceRef.

D2. The speaker is the principal. TurnRequest carries an actor assertion; capability tokens carry actorId and liveActor; the agent is never a separate principal. Triggers carry a runAs mode (owner, scopeFloor, scopeShared) and re-check membership and internal status at every fire [#2, #43, #44]. Consequence: a room agent has the reach of whoever spoke, bounded by membership; posting elsewhere is bounded by the acting human's visibility [#36].

D3. The tape is the source of truth; surfaces are projections. Sessions are append-only logs of exactly what the model read and produced, plus explicit context events; context is a deterministic fold of the log for the current audience [#68]. Status: both session_entries and the tape remain, mode default serve, and SPEC records being "stuck for weeks" on the cutover. Slack and web render the same sessions with separate presenters [#110].

D4. Durable execution first. Every turn is a run row; one running run per session via a partial unique index; claim by UPDATE with FOR UPDATE SKIP LOCKED; lease token with heartbeat at ttl/3 and self-cancel after 3 lost beats; crash-loop parking; token-fenced completion; steer and stop signals stored and replayed if they lose a race with completion [#51 to #55]. Postgres LISTEN/NOTIFY wakes workers but is never the record [#58].

D5. Everything below the agent is swappable behind a profile. Harness adapters declare capabilities (abort, steer, images, fast mode, native tape, goal enforcement); sandbox backends declare persistence, process sessions, park-on-teardown and egress enforcement; the router refuses instead of substituting a weaker computer [#8, #21]. The agent can even swap its own runtime mid-task with saved tool results (max 8 handoffs per task) [#153].

D6. The agent should not see secrets. Credential broker (calls made on the agent's behalf with https, host and path binding, redirect refusal, 5 MB cap, audit), secret drop forms (7-day, atomic redeem), asks routed as approval cards, per-turn capability tokens, masking of output. Escape hatches exist and are documented: files on disk for CLI logins [#14 to #18, #73].

D7. Tighten-only. The org floor can only be narrowed by lower scopes; audience-intersection floor for egress and granted resources; every transcript entry carries a scope label and reads are filtered by audience; three actions are deliberately unreachable from the agent (admin grants, impersonation, approval decisions) and parity tests pin the agent-callable route set [#3, #4, #5, #7, #12].

D8. The agent's voice in a room is explicit. In ambient and coordinator turns the only way to speak is the surface tool; post replies here, reach goes elsewhere, react, edit and delete only own messages, read_thread, whats_new, search (mirror or as the asking person), read_members, read_file. Silence is the default [#131, #36, #31, #45].

D9. Multi-tenancy is operational, not logical. One organization per deployment; ORG_ID fixed; per-org stack from a deployment directory pinned to exact CLI and image digests [#78, #84].

D10. Thin harness, delete before you add. SPEC north stars: unhobble, free the brain (state in Postgres), fast is the best feature, agent UX equals human UX, delete before you add. The review process is built to enforce them [#81, #130, #152].

## 4. Data model and protocols

### 4.1 Main stores (names from SRC; groupings mine)

| Area | Stores | Notes |
|---|---|---|
| Conversation | session tape rows (message, context_event, annotation), session_entries (legacy), session metadata, participants windows, session_shares | scope_label on every entry; tenure windows by sequence [#68, #94, #108] |
| Execution | runs, tool_calls (run_id, attempt, call_index), run signals, instance_heartbeats, background_ownership, leases and advisory locks | [#51 to #55, #63] |
| Delivery | deliveries outbox (idempotency key, claim TTL, 6 h expiry, shadow rows), Slack envelope staging, surface cache (channel_messages, channel_state, channel_files, channel_policy and history) | [#60, #61, #88, #32] |
| Authority | principal_links, directory, projects (versioned roster), teams, team_members, team_rooms, grants (ACL), feature flags, admin grants | [#76, #91 to #95, #41] |
| Secrets | keychain credentials, asks, grants, drops; user model credentials; connector OAuth clients and flows | [#14 to #16, #132] |
| Memory and skills | memory_revisions (records JSON, sensitivity, provenance), skills, skill packs, bundles | [#26, #29] |
| Work | crons and cron_fires, loops, loop items, monitors, tasks, swarms | [#42, #46, #47, #35] |
| Artifacts | file artifacts (SHA-addressed), blobs in S3, deployments (apps) with versions | [#106, #107] |
| Governance | audit_log, credential usage and egress audit sinks, error_events, spend ledger, security screenings | [#74, #80, #139] |

62 CREATE TABLE IF NOT EXISTS statements across src; many small records use one generic DurableMap over Postgres JSONB with a memory twin for tests [#64]. Migrations are code-registered with checksums and can run maintenance steps before the migration set [#65].

### 4.2 Protocols and transports

| Boundary | Mechanism | Evidence |
|---|---|---|
| Surface to core | HTTP JSON; HMAC-signed source requests with 5-minute replay window; portal identity header with the principal and impersonation fields | [#62, #77] |
| Agent to core | per-turn capability token (HMAC JSON, audience, scope, actor, run attempt and lease token, egress policy, memory scopes, credential handles) in x-agent-capability | [#18] |
| Browser to core (via BFF) | /v1/turns?async=1 then SSE /api/runs/:id/events with AG-UI-style events, polling fallback; one multiplexed session-state SSE | [#56, #57, #59, #158] |
| Workers | Postgres claim with lease; LISTEN/NOTIFY wake-ups, 7,500-byte payload cap, one listener per process | [#51, #58] |
| Scheduling | pg-boss queue "cron-fire" with singletonKey cronId:scheduledAt, 1 s polling, up to 32 local concurrency | [#42] |
| Slack | Bolt; Socket Mode default; ack within 2.5 s then durable staging | [#61, #157] |
| Sandbox control | per-provider SDKs or REST; Lambda microVM REST signed with SigV4; guest HTTP daemon /health /exec /write /read | [#143] |
| Egress | Envoy forward proxy, Lua filter calling an authorization service with the per-turn token as proxy password | [#25, #145] |
| MCP | client only, to admin-registered servers, shared or per-user credentials | [#100] |
| Desktop | qm-desktop:// deep link, PKCE-style single-use code, sender-validated IPC | [#112 to #115] |
| Models | Anthropic Messages, OpenAI Responses, chat completions through gateways with protocol chosen from provider metadata; per-person subscription logins | [#105, #132] |

No NATS, Kafka, Temporal, WebSocket server, WebRTC or Nostr exists in QM (grep), in contrast to Verevon's planned NATS and Temporal and to buzz's relay.

## 5. Sandbox and runtime model (the "agent computer")

- A computer is a resource the agent can list, create, set as default, restart and retire, with explicit per-scope defaults; refusal is explicit when no default exists. Sandbox resources are always on after the upgrade that retired routing tables and SANDBOX_RESOURCES_ENABLED [#20, #148].
- One contract, ten backends. The profile reports writable persistence (resident_disk, snapshot_to_workspace, provider_managed), process sessions, park-on-teardown and egress enforcement (none, ip_port, domain) [#21, #22]:

| Backend | Persistence | Parks on teardown | Egress enforcement | Notes |
|---|---|---|---|---|
| sprites | resident disk | no | domain when proxy set (policy set and read back) | SDK with WebSocket exec and checkpoints |
| smolmachines | resident disk | no | domain (allowCidrs, verified set) | |
| e2b | provider-managed or snapshot | yes | domain (allowOut proxy host, denyOut all) | template via Template SDK; child template bakes layer tools |
| modal | provider-managed or snapshot | no | domain or CIDR allowlist | baked image id |
| porter | resident disk | no | domain; cluster must enable egress restriction (Cilium) or creation fails | sandboxes cannot reach RFC1918, proxy must be internet-reachable |
| agent37 | resident disk | no | none | third-party hosted; added via an ADR from the vendor |
| superserve | resident disk | no | none (operator allow/deny rules applied, policy hash stamped) | config epochs fence stale instances [#146] |
| do-managed-agents | provider-managed or snapshot | yes | domain | agent none: only the machine is used; port-forward exec |
| aws (Lambda microVM) | snapshot to workspace | yes | none | suspend/resume, 7.5 h rotation, S3 home [#143] |
| local docker | resident disk | yes | none | same guest daemon as AWS; dev default |

- Home persistence is a mix of provider checkpoints and a throttled tar in object storage (2 GiB cap) with refuse-to-adopt-blank rules; processes and RAM are never preserved [#23].
- Isolation of data across rooms: read-only org and team layers mount into every computer, the scope's layer is read-write; Open sharing lets a live trusted human turn read bounded cross-context material (25 shared scopes, 100 sessions, 200 files) with audit, and Isolated always wins [#19, #24].
- Capability tokens bind a computer to scope, actor, run and lease; TTL defaults to 48 h and operators can configure no expiry (verification accepts exp 0 only for four dedicated audiences, including the sandbox control-plane audience); credentials are plaintext inside the sandbox while in use [#18].
- Egress: Envoy + authorization service with DNS pinning (the vetted IP is handed back in a header), private ranges, metadata and loopback always blocked; enforcement is conditional per backend, and runtime-deployment egress is not built (SECURITY.md) [#25, #145].
- Background processes are first class (process sessions, reaper, durable watches with cursor, pattern and expiry) [#46].
- Browser: a skill-based remote CDP browser (Kernel, Anchor or Browserbase) driven by an embedded browser-use runner; the runner calls back to core with a scoped model token; profiles are keychain credentials and DM-only; sign-in walls are handed to the person via a live-view link; no approval gate inside the runner [#133 to #135]. This is the closest QM gets to the "agent with its own PC and browser" idea, and its gating is by prompt, not by a technical executor as in Verevon's Quarry design.

## 6. Secrets and security model

- Posture: strict (every tool call needs approval except two no-effect turn enders), auto (flagger-driven), dangerous; tightened per scope; sharing posture isolated or open [#69].
- Screening: prompt-injection screener labels tool-result provenance, chunks input (16,000 chars cap, 1,600-character chunks, 256 overlap), at most two in flight, writes an audit record without the text; fails open with an untrusted marker on outages; policy simulation replays up to 500 captured screenings [#70, #71].
- Command policy: regex floor with shell canonicalization to depth 8; the docs call it a speed bump [#72].
- Approvals: durable records with once, session or always grants; Slack cards with Run and Deny; the approval pause terminates generation; credential approvals only through the card; quarantined tool output released once [#10, #11, #13].
- Human-only walls: grants, impersonation, approval decisions are not in the agent API [#12].
- Identity: principal links; portal sealed sessions with domain-separated keys; trusted OIDC; CLI-minted five-minute admin login; no SCIM, passkeys or SSO provisioning [#76, #77, #151].
- Audit and retention: audit_log idempotent but log-and-drop on failure by default; admin reads are audited, not consent-gated; exact model requests captured by default; file artifacts never expire [#74, #75].
- Telemetry: optional Sentry with unredacted context; optional PostHog; Postgres error log [#139].
- Supply chain: 7-day dependency cooldown, production npm audit, signed images, provenance-attested CLI, CI rejects AI co-author trailers [#81].
- Stated limits (SECURITY.md:130-185, DOC+SRC): command policy bypassable; open sharing relies on model discretion; browser actions outside gates; sandbox credentials plaintext in use; admins can read sensitive content; durable data outlives expectations; app capability links are bearer; screening can fail open.

## 7. Channel adapters and surfaces

- Slack is the only chat adapter. Events are acknowledged within a 2.5 s cap and staged durably (300 ms stage timeout, 3 min stale, 5 min claim, 5 attempts, 24 h abandon) before handlers run [#61]. message-gating decides addressed versus overheard; unaddressed channel batches go to an ambient judge with per-channel standing orders and a bot ledger (ignore, rollup, action, user) [#31, #32, #122]. Approval cards, task-list reactions, agent-picked acknowledgement emoji, inline reply directives, history mirror with live fallback, external-workspace guests with a private-continuation flow, and manifest generation with live runtime reconciliation (5 s poll) complete the adapter [#50, #88, #120, #121, #125, #126].
- Web is a second projection with its own presenters, a thin BFF, SSE and polling recovery, and a scope-centred context page with a 16-pane dockview layout [#111, #158].
- Desktop is a thin Electron shell around the remote web app [#112].
- Gmail is a loop source (Pub/Sub push, cursor advances after durable import) rather than a chat adapter [#48].
- There is no Teams, Discord, SMS, email-as-room or voice adapter; the adapter contract is surface-neutral destinations plus presenters, but only Slack implements it in full (INF from file layout).

## 8. Scheduling, background work and attention

- Crons: scope-owned, one fresh thread per fire, deterministic fire keys, exclusive fires, deferral when the session is busy (30 s up to 10 min late), one-shot self-disable, fire notes for continuity, mention roster so reminders ping the right person; validation errors that teach the agent the right form [#42, #149].
- Authority: re-authorize at every fire, run as owner, scope floor or scope shared, recipient consent for recurring schedules, no-escalation rule, credentials need explicit grants for unattended use [#43, #44].
- Quiet by design: poll surfaces may answer "no update" and stay silent; per-second tick job removed so idle scheduling causes no writes [#45, #42].
- Loops (Inbox): durable queues with triage, ship gate, trust ladder and governor; event ingestion acknowledged after durable receipt; flagged [#47, #48].
- Monitors: durable watches on background processes [#46].
- Admission control during handover: AdmittedWork gate, background owner record, build-heartbeat drain [#63, #140].
- Attention: there is no notification system. Delivery goes to a destination (Slack DM or thread, web transcript); approvals and asks wake the conversation; ambient judge decides when to speak; the Inbox ranks work. No push, mute, DND or digest apart from a skill-based morning digest [#97].

## 9. Storage, migration and upgrades

- Postgres is "the core's filesystem"; process memory is only a cache. SPEC's non-durability lens catalogues real bugs from process-local state; webhook in-flight tracking remains unfixed on main [#152].
- Pooled and direct connection split for PgBouncer; one LISTEN connection per process; advisory locks and leader leases on direct connections [#58, #65].
- Rolling-upgrade rules: checksums immutable and mismatches stop the upgrade; legacy tables imported once with a marker and kept for old binaries; code rollback does not roll data back, so a database recovery point is required [#148].
- Object storage for blobs and home snapshots; presigned multipart transfers with caps (16 MiB parts, 1 GB, 12 GB staging) [#66].
- Published-app data: AWS uses Litestream to S3 with per-deployment STS-scoped prefixes; Fly uses per-app encrypted volumes (host-local, not replicated) [#144].

## 10. Multi-tenant stance and deployment

QM is explicit: one organization per deployment (SECURITY.md:18-34) [#78]. The answer to multi-tenancy is a per-org stack.

| Target | Services | Computers | Apps (published) | Notes |
|---|---|---|---|---|
| Docker (CLI target) | local containers | local Docker | docker provider | for test drives only |
| Fly (CLI target) | Fly apps | Fly Sprites (INF: the CLI passes resident env as FLY_RESIDENT_ENV_*) or an env-selected backend | Fly Machines with suspend and volumes | the Fly Machines sandbox backend is retired |
| AWS (CLI target) | ECS Fargate, CloudFront, Terraform module, RDS | Lambda microVM | microVM with Litestream | blue-green cohorts, restore-point proof, rollout deadline (20 min service targets, 30 min ownership handoff) [#83] |
| Kubernetes (Helm) | core, web-ui, portal, egress proxy | any supported backend | operator choice | no default image tag; digest refs |
| Porter | four Porter apps | Porter sandboxes (Cilium egress) | in-cluster | runbook for an agent operator |

Other mechanisms: deployment layer (tool descriptors and skills delivered by content hash and checked by conformance) [#99]; typed secret routing [#82]; operator bootstrap login [#151]; agent-operated runbooks with a live canary gate [#154]; combined workloads keep module trust edges [#147]; contributions model uses human-written ADRs, and the two ADRs in the tree are vendor proposals (Agent37 as a sandbox backend; surfacing a refused agent computer), not a decision log [#123].

## 11. Client shell: desktop and web

- Desktop (Electron 44, prototype): sandboxed renderer, no Node, contextIsolation, per-instance cookie partition, all device permissions denied, webviews blocked, off-origin links to the browser, same-origin links in sandboxed preview windows, HTTPS-only instance URL; browser sign-in handoff; single-instance lock; custom protocol; preload ergonomics (drag regions, hold-Command session shortcuts, tab cycling); signed and notarized packaging path; no auto-update, tray or notifications [#112 to #117].
- Web: responsive build with a phone mode at 860 px and an installable manifest but no service worker; Cmd+K search; deep links; themes and density; accessibility partial; no i18n [#111, #118, #119, #159].
- Compared with Teams, Slack and Grok Bot (per earlier inventories): QM lacks native notifications, tray, auto-update, multi-account switching and offline.

## 12. Testing and engineering process

- 712 root test files with node:test and module mocks, fakes for every sandbox provider, Postgres lane (about 50 files), contract tests across memory and Postgres stores, route-table-driven auth parity tests, sharded CI, exact-tree reuse, image boot smoke, certified-tree receipt [#85].
- Live Slack E2E against a digital twin with an event pump and LLM-judged assertions [#86].
- dev-instance: any worktree boots as a production-shaped stack on real model and Postgres with Slack app pool leases, a delivery canary, idle teardown; expected before PRs touching agent or surface behaviour [#142].
- Memory benchmark with six canned conversations and three judged axes with floors [#150].
- Process: fresh-context review for every PR; wall-of-shame lens reports in the PR description enforced by CI; config-matrix and schema changes disclosed; no comments; fix every instance; durable by default [#81, #152]. The repo does not fully follow its own rules (comments remain; screenshots are tracked) [#81].

## 13. Taxonomy mapping (categories 7, 9, 10, 11)

### 13.1 Agents in the room (7): QM is strongest here

| Sub-topic | QM | Records |
|---|---|---|
| Agent identity as member | none; acts as speaker; bot user only in Slack | [#2] |
| Personal agents | personal scope with isolated memory, files, keychain view, crons, apps, computer; per-speaker AI accounts | [#1, #132] |
| @-invoking and ambient | mention routing, addressed vs overheard, ambient judge, standing orders | [#31, #32, #122] |
| Work visibility | tasks list, reaction acknowledgements, subagent cards, SSE run stream | [#37, #56, #59] |
| Approvals and hand-off | durable approvals, cards, asks, recipient consent, hand-off skill | [#10, #15, #44] |
| Agent PC, browser, sandbox | ten backends, typed profile, resources, browser skill and model grant | [#20 to #25, #133, #134] |
| Multi-agent | subagents, swarms, spine, goals with governor | [#30, #33, #34, #35] |
| Memory per person or room | scoped notebook with sensitivity and provenance, context boundary, external providers | [#26 to #28] |
| Scheduled work | crons, loops, monitors, triggers | [#42 to #49] |
| Permissions, secrets | broker, drops, tokens, walls | [#12 to #18] |
| Cost and limits | coarse rate limit and budget; spend ledger and export | [#80, #136] |

### 13.2 Realtime and sync (9): modest but disciplined

Transport is SSE plus HTTP; ordering is per session in the run queue; no optimistic UI beyond streaming; recovery is client-side offset check and polling fallback; no offline cache, multi-device engine, push or E2EE; scalability is horizontal workers on one Postgres [#51 to #67, #124, #140, #141, #157].

### 13.3 Governance, security and compliance (10)

Strong on prompt-injection posture, tighten-only policy, audit of access, release safety and supply chain; weak on retention, DLP, eDiscovery, residency, SSO/SCIM, multi-tenancy [#69 to #86, #135, #137 to #139, #142, #150, #151].

### 13.4 Client shell (11)

Locked-down Electron prototype, PKCE-style handoff, responsive PWA without push [#112 to #119, #158, #159].

## 14. Best in class (candidates to copy)

| Pattern | Why it is the best example read | Records |
|---|---|---|
| Credential broker (secret never seen) | https, host and path binding, redirect refusal, audit, tested | [#14] |
| Tighten-only cascade plus audience-intersection floor | pure functions, tested, applied to egress, grants, memory | [#3, #4, #26] |
| Human-only authority walls pinned by parity tests | a documented invariant, not a convention | [#12] |
| Explicit room I/O tool with post versus reach split | prevents accidental broadcast; silence is default | [#131, #36] |
| Typed backend profile that refuses substitution | explicit capability negotiation; egress capability with readback | [#21, #145] |
| Recipient consent and re-authorization at every fire | unattended work cannot outlive access | [#43, #44] |
| Goal with independent governor | the agent cannot end or pause its own goal | [#30] |
| Ack-after-durable-staging and delivery outbox | at-least-once with idempotency and expiry | [#60, #61] |
| Per-session serialized durable runs with fencing | simple SQL, strong invariants | [#51 to #55] |
| Browser model grant | browser vendor never holds a model key | [#134] |
| Policy simulation by replay | counts only, payloads stay in place | [#71] |
| Live release gate and agent-operable runbooks | done means a canary passed | [#83, #154] |
| Wall-of-shame review lenses enforced in CI | design memory that gates merges | [#81] |
| dev-instance with channel canary | multi-party QA before review | [#142] |

## 15. Weaknesses and limitations

Mine, with evidence: single-org trust model; regex command policy; fail-open screening; log-and-drop audit by default; admin reads without consent; no retention, DLP, residency or SSO/SCIM; browser gating by prompt; unredacted error reporting; coarse input-token budget; Slack-centric chat with no QM-native DMs, channels, presence, read state, calls or push; ten backends are a maintenance burden; 285 env knobs; god files (orchestrator 4,565 lines, wiring 3,082, agent-tools 4,602, AWS CLI backend 4,843, web chat.ts 3,458); tape cutover unfinished; webhook in-flight race; docs drift (CLI README still says Fly Machines for agent computers; skill-registry says sync is a later phase; background ownership docs mention a protocol older images used) [#22, #54, #63, #68, #72, #74, #75, #78, #135, #139].

QM's own (docs/SPEC.md): about 75 stated examples across 17 lenses covering overengineering, band-aids, duplication, hand-rolling (pg-boss versus custom retries, wakeups and delivery claims), non-durability, config-matrix expansion (ten sandbox backends, four overlapping runtime settings), provider drift, god files, regex (1,443 production regex sites in 380 files per the spec), legacy preservation, discarded error evidence, token waste still on main (per-turn environment note, duplicated credential listings).

## 16. What the Verevon August comparison missed, changed or can now sharpen

Baseline docs: apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md and verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md (QM baseline d719f540). Their adapt list (versioned fencing, one scope resolver, audience intersection, interrupted-effect marker, head-and-tail cap, approval pause, durable work fencing, recoverable audit outbox, typed harness adapter, signed chassis, bounded screening, capability profile, layered workspace, provenance-bearing artifacts, durable watches, fresh authorization at fire, skill lifecycle, scoped credential grants, memory revisions, scope-serialized provisioning, deterministic fire keys, scope-centred UI, manifest and digest evidence) still holds for the items I re-read in current code (resolver, audience floor, interrupted-effect marker, head-and-tail cap, approval pause, run fencing and orphan-steer replay, capability profile, layered workspace, durable watches, fire-time authorization, deterministic fire keys, memory revisions, per-scope provisioning lock, signed chassis); the recoverable audit outbox in deployment-layer-store and the skill-store internals were not re-read. The reject list is mostly still right, with these updates:

| Baseline claim or omission | Current QM state | Evidence |
|---|---|---|
| Bearer public app links as the default | changed: publishing is private, no public URL, share separately; external sharing off unless an admin enables a flag | skills-seed/publish/SKILL.md; commits a153974f, 27501bfe [#79, #106] |
| Shared-room facts auto-copy into personal memory | still possible but now carries provenance and sensitivity, read through one disclosure view; carrying memory needs Open policy at both ends | docs/memory-records.md [#26] |
| RAM-only approval projections | fixed in QM (approvals durable); webhook in-flight tracking remains process-local | docs/SPEC.md:87-96 [#152] |
| Fail-open screening | still present on outages and unscreenable content | [#70] |
| Log-and-drop audit | still the default | [#74] |
| Second Postgres queue | pg-boss is used for crons next to the Postgres run store; SPEC calls this hand-rolling | [#42, #51] |
| Stale pointer pi-tools.ts | folded into agent-tools.ts on 2026-09-07 | [#11] |
| Browser runner outside some gates | confirmed and now detailed (skill-based, DM-only profiles, no approval gate, pre-launch consent) | [#133 to #135] |
| Budget and rate infrastructure | now detailed: per-speaker and org rolling windows on input-token estimate | [#136] |

Not covered by either document (new since baseline or under-weighted): swarms [#35]; persistent subagents and the responsive spine [#33, #34]; goals with independent governor [#30]; team scopes [#93]; Inbox Loops with ship gate [#47]; explicit surface tool and reach [#131, #36]; ambient judge and standing orders in detail [#31, #32]; per-speaker AI accounts including subscription logins [#132]; browser model grant [#134]; client-executed tools [#39]; agent-initiated runtime handoff [#153]; admission gate [#140]; document inputs [#138]; model verification [#137]; session tape and fork [#68, #90]; typed egress capability and Porter, Superserve, DigitalOcean, AWS microVM backends [#143, #145, #146]; Litestream for apps [#144]; desktop shell [#112 to #117]; rolling-upgrade rules [#148]; dev-instance and review lenses [#81, #142]; combined services [#147].

## 17. What Verevon should copy, adapt or avoid for Spaces

Copy (mechanism is directly useful):
- Explicit room I/O tool with reach bounded by the acting human's membership and a per-turn outbound cap [#131, #36].
- Teachable errors and a fire contract for scheduled agent work (fresh thread, continuity outside it, mention roster) [#149].
- Typed egress-enforcement capability with provider readback and replace-on-drift; per-turn egress token as proxy credential [#145].
- Browser model grant for any browser runner (scoped, single-model, account-bound) [#134].
- Admission gate for synchronous work during handover [#140].
- Configuration epochs to fence stale cohorts [#146].
- Model verification fingerprint for per-Space model allowlists [#137].
- Document input path: provider-native files, bounded fallback, per-turn re-authorization, references on the tape [#138].
- Durable-by-default rule and wall-of-shame lenses as review gates; dev-instance with a channel canary; memory benchmark with floors [#81, #142, #150, #152].

Adapt (take the shape, change the authority model):
- Scope resolver and audience floor: use typed SpaceRef and Control-signed decisions; never string ids or historical participation as authority [#1, #4].
- Speaker-as-principal: Verevon plans agents as members with their own authority; keep QM's rule that a human's reach bounds what an agent does on that human's behalf, and the recipient-consent rule for triggers [#2, #44].
- Per-speaker AI account: bill and rate-limit the speaker, no silent fallback; skip subscription-login reuse unless vendor terms allow [#132].
- Budgets: charge the speaker and the Space, with real priced usage, not an input-token estimate [#136].
- Ambient judge: cheap observer plus standing orders plus audit; make it Space-policy-gated [#31, #32].
- Goals with a governor and runtime handoff: keep inside the Model Plane with cost attribution [#30, #153].
- Rolling-upgrade rules and agent-operable runbooks for sovereign installs [#148, #154].

Avoid (negative examples):
- Single-organization trust assumption; regex or shell-text command policy as a boundary; fail-open screening; log-and-drop audit; admin reads without consent; no retention; unredacted telemetry; browser actions without a technical executor gate; bearer capability links as sharing; plaintext long-lived credentials; ten-way provider matrix without a drift budget; Slack as the only chat truth; process-local state [#72, #70, #74, #75, #78, #135, #139, #22, #87, #152].

## 18. Open questions and unverified items

- Whether QM's production deployments use Socket Mode or HTTP events (default code path is Socket Mode; deployment configs not read) [#157].
- Real-world behaviour of the swarm, spine, team scope and loops features (all flagged; read from code and docs only) [#33, #35, #47, #93].
- Whether AI-based screening and the ambient judge perform acceptably (no benchmarks in the repo apart from the memory bench).
- Exact load limits: no published numbers; the "million agents" aspiration is a spec statement only (INF) [#124].
- Whether the OAuth client ids used for Claude and ChatGPT subscription logins are permitted for third-party products (INF, medium confidence; terms not examined) [#132].
- Whether uploads, admin pages and the Inbox UI carry additional realtime behaviour not covered by the product-surface audit (not in this facet).
- Provider-side docs for Superserve, DigitalOcean Managed Agents, Porter and Lambda MicroVM were read only through QM's own docs; their current public behaviour was not checked.

## 19. Evidence index (files that carry most of the weight)

Docs: README.md, SECURITY.md, AGENTS.md, CONTRIBUTING.md, docs/SPEC.md, docs/session-tape-spec.md, docs/memory-records.md, docs/deploy-directory.md, docs/background-ownership.md, docs/combined-services.md, docs/upgrading-runtime.md, docs/document-inputs.md, docs/admin-model-verification.md, docs/error-reporting.md, docs/porter.md, docs/superserve.md, docs/swarms.md, docs/persistent-subagent-sessions.md, adrs/*.

Source: src/types.ts, src/resolution/*, src/core/orchestrator.ts and orchestrator/*, src/runs/*, src/harness/{agent-tools,harness,harness-router,goal,context-compaction,runtime-control}.ts, src/sandbox/*, src/auth/capability-token.ts, src/api/{server,app-turn,app-ambient}.ts and routes/*, src/credentials/*, src/security/*, src/cron/*, src/triggers/*, src/loops/*, src/surface-cache/*, src/slack/*, src/persistence/*, src/ratelimit/*, src/deploy/*, plugins/{web-ui,portal,chassis}, desktop/*, cli/src/*, deploy/*, skills-seed/browse/SKILL.md, .codex/skills/dev-instance/SKILL.md.
