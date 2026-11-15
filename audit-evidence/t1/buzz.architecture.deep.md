# Buzz architecture deep dive (facet: architecture)

Audit 1 for Verevon "Space". Source: Buzz, the open-source Rust workspace where humans and agents work together on a Nostr relay (Apache-2.0, Block, Inc.). Repo: `C:/dev/buzz`, HEAD `799cfd705` ("Start the ACP idle interval when channel turns complete (#8149)"), desktop 0.5.28. Read-only audit: nothing was built, run or modified.

Companion file: `buzz.architecture.features.json` (116 records, ids `buzz-architecture-001` to `-116`). Record ids are cited below as `[A-nnn]`. Evidence paths in the JSON are absolute (`C:/dev/buzz/...`); in this narrative they are repo-relative.

Provenance tags: **SRC** read in source or config. **DOC** read in documentation or specs (intent, not proof). **INF** my inference, basis stated. **REA** was not used: Buzz is open source and was read directly. Code is ground truth; where docs and code disagree, the section "Doc-versus-code drift" lists it.

Scope notes. Slack, Teams and Grok Bot were not analyzed in this pass (other agents cover them with REA). Every comparison to them below is **INF** from the two inventories' headings and general knowledge, and is marked. Buzz-vs-Verevon statements use the scout baseline `v-space-backend.baseline.md` (DOC, written by another auditor) and are marked as such.

---

## 1. Verdict in ten lines

1. **Buzz is a signed-event log with a thick relay around it.** Every action is a Nostr event identified by a `kind`; a relay per community is the only source of truth, with Postgres 17 (monthly partitions), Redis (pub/sub, presence, rate limits, fences), and S3-compatible storage for media and git [A-001, A-011] (SRC).
2. **Agents are ordinary principals, not a bot API.** An agent has its own keypair, a membership, presence and typing, and passes the same admission pipeline as a human; ownership is proven offline by a signed attestation (NIP-OA) and access can be derived from the owner's membership (NIP-AA) [A-035, A-036, A-037] (SRC).
3. **The agent runtime is a harness, not an engine.** `buzz-acp` bridges relay events to any ACP agent over stdio with per-scope queues, steer/interrupt modes, an author gate and owner control commands; it is the most reusable part for Verevon [A-038 to A-043, A-040 unique] (SRC).
4. **Supervision is owner-private by design.** Tool-level telemetry (NIP-AO) and usage (NIP-AM) are NIP-44 encrypted to the owner and invisible to other room members; the room sees only what the agent chooses to post [A-047, A-049] (SRC). There is no per-tool human approval: the harness auto-approves every ACP permission request and the default mode bypasses permissions [A-045] (SRC).
5. **Tenant isolation is the strongest part.** Host binding before any handler, typed `AdmittedTx`, community-leading keys, write-fence triggers, TLA+/Tamarin models with mutation tests and a runtime trace-replay checker [A-065, A-066, A-068] (SRC). One gap: the proof assumes Postgres row-level security that the checked-in schema does not install [A-067] (INF, medium).
6. **Realtime is conventional but unusually hardened**: three-tier subscription index, a single delivery-time access chokepoint, per-community refcounted Redis topics, ephemeral kinds that never touch storage, and a replica-fence proof so keyset pages can come from read replicas [A-004, A-006, A-012] (SRC).
7. **The price of pure Nostr is visible.** No server-assigned sequence means time-lookback resume (about 31 minutes overlap), bounded-skew triggers and heartbeat proofs; read state split into two non-synchronizing systems (NIP-RS blobs and the `/buzz/v1` accessory API) [A-012, A-015, A-019] (SRC/DOC).
8. **Push is the best-designed notification piece**: a blind, capability-gated gateway where APNs only ever carries a fixed "reconnect" wake, with encrypted, expiring, self-scoped leases [A-031] (SRC). It is experimental and Apple-only; desktop-active suppression is a draft spec only [A-033] (DOC).
9. **What Buzz lacks is exactly what enterprises ask for**: no semantic search, no DLP, no residency, no SCIM, no eDiscovery export, no video or screen share, no i18n, no command palette, no agent browser or desktop, no spend caps, no agent-to-agent hop limit [A-044, A-054, A-064, A-078, A-102] (SRC/INF).
10. **For Verevon the transferable value is in patterns, not code**: subscribe-first/backfill-second, delivery-time re-authorization, exact-key mentions, mid-turn steer, author gate per binding, owner-reviewed drafts, audience-bound agent instances, self-reaping bodies, typed admission, deny-by-default plus formal replay (section 10).

---

## 2. System map

```
 Clients                                   One deployment (N stateless relay pods)
 ───────                                   ─────────────────────────────────────────────
 Desktop (Tauri 2 + React 19)  ─ WS NIP-42 ─┐   ┌────────────────────────────────────────┐
   Rust-owned socket, SQLite caches         ├──►│ buzz-relay (Axum, 158K lines incl.      │
   sidecars: buzz-acp/agent/dev-mcp/CLI    │   │ tests): WS, HTTP bridge, media, git,    │──► Postgres 17 (events partitioned
 Mobile (Flutter)              ─ WS / push ─┤   │ huddle audio, workflows, push runtime,  │     monthly; channels, members, audit,
 Web (repo browser) / admin-web ─ NIP-98 ───┤   │ admin, operator listener, mesh          │     outboxes, read-state, fences)
 CLI `buzz` (agents, CI)       ─ NIP-98 ────┘   └───────┬──────────────┬─────────────────┘──► Redis (per-community pub/sub,
                                                          │              │                     presence, typing, rate limits,
 Agent harness (buzz-acp ──ACP/stdio──► goose |           │              │                     NIP-98 replay, mesh leases)
  claude | codex | buzz-agent | BYO)  ◄─ WS NIP-42 ───────┘              │               ──► S3/Silo (media, git packs)
                                                                          │
 Side services: buzz-push-gateway (own Postgres, APNs, App Attest)       │  inter-pod QUIC mesh (iroh) for huddle
                buzz-pair-relay (loopback sidecar, NIP-AB pairing)        │  and tunnel sessions, Redis-fenced leases
                buzz-admin (one-shot jobs: deletion drain, storage snapshot, roster)
                buzz-backend-kubernetes (provider binary that deploys agent pods)
```

Workspace: 33 crate directories under `crates/` plus `desktop/src-tauri` (excluded from the workspace) (SRC). Sizes (lines include inline tests): `buzz-relay` 158K, `buzz-db` 80K, `buzz-acp` 61K, `buzz-agent` 42K, `buzz-cli` 26K, `buzz-auth` 13K, `buzz-core` 11K, `buzz-media` 10K, `buzz-backend-kubernetes` 7K, `buzz-push-gateway` 6K, `buzz-workflow` 5K, `buzz-relay-mesh` 3K, `buzz-pubsub` 2.5K, `buzz-audit` 1.2K (SRC, second-pass `wc`). `buzz-core` is zero-I/O by rule (types, verification, filter matching, the kind registry); services never call each other, only the relay composes them (ARCHITECTURE.md:148, DOC). The relay is a modular monolith: the biggest maintainability risk in the repo (INF).

Two unrelated things are called "mesh". (a) `buzz-relay-mesh`: an inter-pod QUIC mesh (iroh) that carries huddle audio and tunnel sessions between replicas of one deployment [A-023] (SRC). (b) "Buzz Mesh" in `VISION_MESH.md`: community-pooled LLM compute through the external `mesh-llm` runtime, a non-default cargo feature of the desktop [A-063] (SRC). Neither is federation between independent relays; ARCHITECTURE.md still says there is no gossip or replication, which is true of chat state only.

---

## 3. Data model and protocol

### 3.1 The event is the unit

NIP-01 event: `id` (sha256 of canonical form), `pubkey` (secp256k1), `kind`, `tags`, `content`, Schnorr `sig`. `kind` is the only dispatch switch. Ranges: replaceable 10000-19999, ephemeral 20000-29999 (never stored, audited or searched), parameterized replaceable 30000-39999, Buzz custom 40000-49999. `crates/buzz-core/src/kind.rs` holds 133 `pub const KIND_*` definitions at HEAD (ARCHITECTURE.md quotes 80 and 127 in different places) (SRC). House rule: prefer a new kind over a new HTTP endpoint, because fan-out, scoping and auth come free (AGENTS.md:289, DOC). Agent-facing operations go in `buzz-cli`, not in an MCP server (AGENTS.md:318, DOC); there is no Buzz MCP crate despite VISION.md still listing one [A-056] (SRC).

Notable kinds by role (SRC kind.rs): messages 9 and v2 40002, edit 40003, reactions 7, pin 40004 and stream reminder 40007 (both used: pins in the desktop, 40007 in the harness default subscription and the feed), bookmark 40005 and scheduled send 40006 (defined only; no UI or CLI verb found), canvas 40100, DMs 41010-41012, jobs 43001-43006 (reserved), usage 44200, artifacts 45010, forum 45001-45003, workflows 46001-46012 plus approvals 46030/46031, huddles 48100-48106, observer 24200, presence 20001, typing 20002, read state 30078, reminders 30300, push lease 30350, engrams 30174, personas/teams/managed agents 30175-30178, moderation 9040-9044 and reports 1984, relay-signed projections 39000-39007, 13534, 30622, 40901, 40902.

### 3.2 Tables that matter (schema/tables/public, SRC)

| Table | Key | Notes |
|---|---|---|
| `communities` | `id`, unique `lower(host)` | Operator-global registry: host, per-community `signing_key`, icon, archival and a four-state deletion machine with a fence generation. |
| `events` | `(community_id, created_at, id)` | Monthly range partitions; generated `search_tsv` (NULL for sensitive kinds); GIN on `tags jsonb_path_ops`; `d_tag`, `not_before`, `delivered_at`, `deleted_at`, `channel_id`. Every hot index leads with `community_id`. |
| `channels` | `(community_id, id)` | Type stream/forum/dm/workflow, visibility, topic/purpose, `participant_hash` (DM identity), `ttl_seconds`/`ttl_deadline`. Same UUID may exist in two communities by design. |
| `channel_members` | `(community_id, channel_id, pubkey)` | Roles owner/admin/member/guest/bot, soft delete (`removed_at`), per-viewer `hidden_at`. |
| `users` | `(community_id, pubkey)` | One profile per community; `agent_owner_pubkey` (FK inside the community), `channel_add_policy`, legacy `okta_user_id`. |
| `audit_log` | `(community_id, seq)` | Hash chain per community, `hash_version`. |
| `thread_metadata`, `reactions`, `event_mentions`, `parameterized_event_watermarks` | community-leading | Disposable projections plus the LWW watermark for addressable events. |
| `workflows`, `workflow_runs`, `workflow_approvals`, `scheduled_workflow_fires` | | Definitions, runs, hashed approval tokens, at-most-once cron claims. |
| `push_*`, `operator_listener_*` | | Lease store, match queue, wake outbox, gateway authority tables (own database). |
| `community_deletion_*`, `relay_admin_*`, `relay_operators`, `storage_accounting_snapshots` | | Operator planes. |
| `personal_read_accounts`, `personal_read_frontiers` | | Server-derived read state (`/buzz/v1`). |

DMs are channels with `participant_hash` (up to 9 members) plus a relay-signed per-viewer hide snapshot (kind 30622, NIP-DV) [A-029] (SRC). Threads are NIP-10 marked replies with materialized `reply_count`/`descendant_count` on roots (AGENTS.md:324) and a depth cap of 100 [A-028] (DOC). Search is Postgres FTS over a generated column, results re-authorized after the query [A-030] (SRC).

### 3.3 Ingest pipeline (SRC `handlers/ingest.rs`, ARCHITECTURE.md:272-295)

auth and scope, pubkey equals authenticated key, reject kind 22242, ephemeral sub-pipeline, `spawn_blocking` signature verify, channel membership, idempotent insert (`ON CONFLICT DO NOTHING`, returns whether inserted), Redis publish, local fan-out, then audit append and workflow trigger as fire-and-forget tasks. The relay replies `OK` only after the whole pipeline. WebSocket `EVENT`, HTTP `POST /events`, `/query` and `/count` share it [A-002]. Ephemeral events skip DB, audit and search; presence additionally sets a Redis key with a 180 s TTL (3 times the 60 s heartbeat) and **refuses ACK and fan-out if the Redis write fails**, so a later bulk presence snapshot (kind 40902) can never contradict a delta a client already saw [A-008] (SRC).

### 3.4 Subscription and delivery (SRC)

`SubscriptionRegistry` (`subscription.rs:379 fan_out_scoped`) keeps indexes keyed by community: (channel, kind), channel-wildcard, and a linear scan for global subs; global subs never receive channel-scoped events. The REQ handler checks channel access before registering (no race), applies a scoped-token upper bound and repairs a negative cache entry from the DB (`req.rs:625`). Delivery goes through one chokepoint, `filter_fanout_by_access` (`handlers/event.rs:115`), shared by local ingest and the Redis cross-node path: receiver-side tenant label equality, author-only kinds, shared-gated kinds, owner-only kinds (30622, 44200), then private-channel membership via a 10 s cache that is invalidated cross-pod, failing closed on any lookup error [A-004, A-005, A-072]. The client contract, learned from production bugs, is subscribe-first, backfill-second, dedupe by id, and release client fences on a terminal `CLOSED` (ARCHITECTURE.md:380, DOC).

### 3.5 Relay-computed views and accessory APIs

- **NIP-CW windows** (HTTP `/query` extension): `top_level`, `include_summaries`, `include_aux`, composite cursor `(until, before_id)` required together; the relay probes `limit+1` for exhaustion and signs overlays (39005 summary, 39006/39007 bounds) with its own key; inaccessible equals nonexistent [A-014] (DOC, kind constants SRC).
- **`/buzz/v1` accessory API** (opt-in, NIP-11 `buzz_v1`): `GET /me/sidebar` returns per-channel and per-thread `{unread, mentions, read_through_id, latest_id}`; `no-store`; NIP-98 per request; absence means "unavailable", never zero [A-019] (DOC). It does not synchronize with NIP-RS (kind 30078 encrypted frontier blobs, still served) (DOC).
- **Relay-signed projections**: group metadata/admins/members (39000-39002), roster (13534), DM visibility (30622), identity archival (8002/8003/13535), presence snapshot (40902). Thin clients stay protocol-pure [A-105] (SRC).
- **Replica routing**: reads that influence a write or a permission read the writer; display reads may use replicas gated by a soundness predicate; clients can force the writer with `consistency: "strong"` (never the reverse) [A-013] (DOC, `bridge.rs:438` SRC).

---

## 4. Identity, authentication, keys

| Mechanism | Used for | Evidence |
|---|---|---|
| NIP-42 (challenge, `kind:22242`, ±60 s) | WebSocket sessions for humans and agents; 5 s AUTH timeout | `connection.rs:30`, ARCHITECTURE.md:234 (SRC/DOC) |
| NIP-98 (`kind:27235`, URL+method+payload hash) | HTTP bridge, media, git, invites, admin, `/buzz/v1`; replay guard `SET NX EX` in Redis scoped by community | `buzz-pubsub/src/nip98_replay.rs` [A-074] (SRC) |
| NIP-OA owner attestation | `["auth", owner, conditions, sig]` over `nostr:agent-auth:<agent>:<conditions>`; offline-verifiable; reusable capability; not an identity override | NIP-OA.md [A-036] (SRC/DOC) |
| NIP-AA virtual membership | Agent admitted by owner's membership at AUTH; no persistent row; no inherited roles; owner stays in session state | NIP-AA.md:25-150, `handlers/auth.rs:208` [A-037] (SRC) |
| NIP-FI / NIP-FA | Enterprise IdP assertion (JWS, `aud` = community host, JWKS offline) plus NIP-42 proof of key; modes off, shadow, enforce, deny_protected; issuer→relay disconnect API | `nip_fi_config.rs:490-515` [A-073] (SRC) |
| Per-community relay key | Signs projections and system events | `communities.signing_key` [A-065] (SRC) |
| NIP-AB pairing | SAS-confirmed device link through a loopback sidecar relay | `crates/buzz-core/src/pairing/NIP-AB.md` [A-021] (SRC) |

Code goes beyond the NIP-AA text in the safe direction: it stores `users.agent_owner_pubkey` first-write-wins, cascades a community ban from owner to agents (an agent ban stays agent-only), and closes live sockets admitted under a revoked owner, cluster-wide over Redis (`handlers/auth.rs:69,208`, `handlers/ingest.rs:2248`, `state.rs:319,855`) [A-037] (SRC). The spec says active sessions are not force-terminated; the code terminates them.

Key custody (SRC): one OS-keyring blob per process (`secret_store.rs`), 0600 file fallback; NIP-49 `ncryptsec` backups and HPKE-wrapped files; an **egress guard** that refuses `ncryptsec` at 13 relay-bound boundaries and a test that fails the build when a new `/events` URL site skips it (`egress_guard.rs:1-25`); agent keys are injected into child env, stripped again by the dev-MCP shim into a 0600 keyfile, and handed to provider binaries only inside the one-way deploy payload [A-075]. Key loss is unrecoverable by design (VISION_SOVEREIGN.md "What You Give Up", DOC).

---

## 5. Agents in the room (category 7)

### 5.1 Agent as principal

`users` row per community with `agent_owner_pubkey` and `channel_add_policy` (owner_only, nobody, anyone: the agent decides who may add it to channels), channel role `bot`, kind 10100 profile, and a kind 0 profile carrying the NIP-OA tag so anyone can prove ownership [A-035]. Same key may join other communities but state (profile, DMs, memory, jobs, audit) is community-local [A-080] (VISION_AGENT.md, DOC). Identity is exact: a persona or an owner-signed ownership claim is never an alias for another key, and "not managed on this device" is a provenance marker, not a hosting claim [A-110] (DOC).

### 5.2 The harness (`buzz-acp`, 61K lines) [A-038 to A-043]

- Connects as the agent (NIP-42, optional auth tag), discovers channels over REST, auto-subscribes on new membership (kind 44100), replays unprocessed mentions at startup (SRC README).
- **Scope key** derived once at admission: channel (default) or thread policy; DMs stay one conversation; tasks are fresh sessions. The same key partitions queues, in-flight tracking and `!cancel`/`!rotate` (`scope.rs`) [A-039]. The agent is told it is one of many sessions that share core memory and workspace but not context (`session_model_*.md`) (SRC).
- **Queue**: 500 pending per scope and per channel, at most one prompt in flight per scope, queued events batched into one prompt.
- **Mid-turn handling** (`config.rs:62-90`): queue, **steer** (default; cancel and re-dispatch a merged prompt framed as "arrived while you were working"), interrupt (supersede), owner-interrupt. Steer is cancel plus merge, not injection into the live turn [A-040] (SRC, unique among references as far as the scout notes show: INF).
- **Author gate** `respond_to`: owner-only (default), allowlist, anyone, nobody. Same-owner siblings are admitted by verifying a NIP-OA tag with a fail-closed `Indeterminate` verdict; workflow messages carry owner authority only with authenticated provenance and an explicit mention [A-041] (SRC).
- **Owner control**: kind 9 message from the owner, body exactly `!cancel`, `!rotate` or `!shutdown`, with a separate `p` mention; consumed before the gate [A-042].
- **Pool and lifecycle**: 1 to 32 subprocesses, per-slot circuit breaker (3 crashes in 60 s, 5 min cooldown), lazy pool, heartbeat prompts, `--respond-to nobody` for broadcast-only agents [A-043].
- **Prompt framing**: paired semantic sections for event, history, routing context and merges; relay-backed skills are untrusted unless a human names them [A-111] (SRC).
- **BYO harness**: tier 1 goose/claude/codex/buzz-agent, tier 2 presets (Cursor, Pi, OpenCode, Kimi, Amp, Hermes, OpenClaw...), tier 3 user JSON; identity keys are stripped from definition env [A-058]. `buzz-acp run --task` is a headless one-shot runner with a versioned JSON contract and exit codes [A-061].
- `buzz-agent` (42K lines) is a minimal ACP agent: up to 8 sessions, handoff-on-full-context ladder, bounded loops, Anthropic/OpenAI/Databricks/OpenRouter [A-057]; `buzz-dev-mcp` gives shell, file, todo tools and underscore-prefixed advisory lifecycle hooks (`_Stop`, `_PostCompact`) [A-056].

### 5.3 Visibility, approvals and human hand-off

| Concern | What Buzz does | Verdict |
|---|---|---|
| Work visible to the room | Only what the agent posts. Base prompt: tool calls and reasoning are invisible, so narrate, never go dark, publish-or-silence (`base_prompt.md:71`) | Weak for non-owners (SRC) |
| Work visible to the owner | NIP-AO frames, kind 24200: ephemeral, NIP-44 to owner, `seq` for gap detection, 100/s per agent, 1,000-frame ring in the harness; desktop validates, renders 15 card classes (verb, object, outcome), can archive locally with 30-day retention [A-047, A-048] | Strong, but owner-only and best-effort (SRC) |
| Cancel | `cancel_turn` control frame or `!cancel` message | Good (SRC) |
| Cost | NIP-AM per-turn records (durable, encrypted to owner, `null` never summed as zero, `deltaReliable`, `pricingIdentity` only when provable) [A-049]; no budget, cap or per-room allowance anywhere [A-064] | Visibility yes, control no (SRC/INF medium) |
| Tool-call approval | `buzz-agent` has a rigorous permission broker (global admission semaphore, at-most-once, one absolute deadline, fail closed), but `buzz-acp` answers `allow_once` automatically and defaults to `bypass-permissions` (`acp.rs:2005-2070`, `config.rs:464-476`) [A-045] | **No per-tool HITL** (SRC) |
| Privileged agent actions | `buzz agents draft-create/update` and `projects add-channel` publish an owner-addressed encrypted request (a NIP-AO frame); owner saves in the app; the prompt forbids claiming the thing exists before that [A-046] | Good pattern; ephemeral so an offline owner loses it (SRC, medium) |
| Workflow approvals | Schema, REST reads, kinds 46010/46011/46012/46030/46031 exist; executor returns `Suspended` but the relay marks the run failed (WF-08); `send_dm` and `set_channel_topic` return NotImplemented (WF-07) [A-087] | Still unwired at HEAD (SRC) |
| Mid-turn human steer | See 5.2 | Best-in-class here (SRC) |

### 5.4 Memory, personas, identity of the instance

- **Memory (NIP-AE)**: kind 30174 addressable events signed by the agent, content NIP-44 under the agent-owner conversation key, `d` tag = HMAC-SHA256 of the slug so slugs are hidden, `core` plus `mem/...`, tombstones, strict head selection, 65,535-byte plaintext limit. The harness fetches `core` per session and injects it every turn; on a transport error it injects nothing, so an outage is never mistaken for empty memory [A-050] (SRC). Scope is **per (agent, owner)**, not per room: the same core goes into every channel the agent serves, which the repo's own information-flow proposal calls a leak path (DOC).
- **Personas and teams (NIP-AP)**: kinds 30175 persona, 30176 team, 30177 managed agent (allowlist projection: never nsec, auth tag, env or runtime), 30178 team catalog, 44300 instruction versions; `SHARED_GATED_KINDS` are author-only unless `["shared","true"]` [A-051]. NIP-PMA (kind 30179, private aggregate with CAS generations) is reserved and rejected by the relay today (DOC).
- **Portable snapshots**: `.agent.json`/`.agent.png`, secrets excluded by construction, memory opt-in at three levels, locked NIP-44 envelope decryptable only by owner or exact agent key [A-109] (SRC).
- **Nest** (`~/.buzz`): AGENTS.md, GUIDES, PLANS, RESEARCH, WORK_LOGS, OUTBOX, REPOS, `.scratch`, versioned templates refreshed on upgrade; Bestie (a personal-agent designation) is internal-build only [A-062, A-099] (SRC).

### 5.5 Hosting, isolation and secrets

- **Local-first**: the desktop bundles `buzz-acp`, `buzz-agent`, `buzz-dev-mcp`, `buzz-backend-kubernetes`, `git-credential-nostr`, `buzz` as external binaries (`tauri.conf.json:55`), supervises them, keeps nsec in the keyring, shows Running/Recent in the tray, holds a macOS power assertion while agents run [A-062, A-094] (SRC). Agents die when the laptop sleeps: hence remote agents.
- **Remote agents** (DOC spec plus SRC): provider binary `buzz-backend-<id>` discovered by name, JSON on stdin/stdout, output treated as hostile and redacted. Axiom M1: after deploy the desktop keeps **no management channel**; presence (kind 20001, 180 s TTL) is the status, `!shutdown` the stop. Invariants I1 identity fail-closed, I2 no secrets in config (key-name lint), I3 presence is status, I4 at most one live instance per key per scope (deterministic pod name plus annotation compare, per-attempt immutable Secrets), I5 intentional termination is final. The agent bounds its own life with `BUZZ_ACP_EXIT_AFTER_INACTIVITY` (default 0, Kubernetes default 2 h) on its own timer independent of pool readiness [A-053]. At HEAD the reaper and launch block exist; the pod is non-root, all capabilities dropped, seccomp, no service-account token, `restartPolicy: Never`, 60 s grace (`pod.rs:131-168`) (SRC). The Known Defects list is pinned at `28ae6cd21` and is partly dated.
- **Isolation**: process-level only. Shell runs as an ephemeral process with process-group kill, 8 KB tail to the model; `BUZZ_ACP_LAUNCH_PREFIX` lets a trusted host wrap workers (Unix only, no fallback); `buzz-acp run` says plainly it is not a sandbox; the IFC document says ACP sessions are not security boundaries. **No browser or desktop automation exists in agent crates** [A-054] (SRC).
- **Information-flow proposal** (draft, 27 Aug 2026): a trusted broker holds the agent key; each agent instance is bound to one audience and never switches; memory entries carry audience and membership epoch; closed action set with deliberately no `sign(bytes)`; five enforcement points; human declassification is exact content to exact destination, single-use. `ifc-core` implements reader-set labels but nothing depends on it yet [A-055] (DOC; no-dependents is a grep, SRC medium).
- **Mesh compute**: relay membership gates `mesh-llm` nodes (status notes refreshed every 45 s, ignored after 120 s); agents see a local OpenAI-compatible endpoint; non-default feature [A-063] (SRC).

### 5.6 Multi-agent behaviour and its known failure

Coordination is conversational: delegate by mention, report back with a callback mention, no mention in narrative, no bare acknowledgements, threads preserved [A-059]. Job kinds 43001-43006 exist only as constants and feed queries (no harness dispatch found). A post-mortem records three Codex agents exchanging 21+ acknowledgements; the fix was prompt wording. **There is no reply-depth counter, hop limit, cooldown or agent-to-agent budget at HEAD** (grep of `buzz-acp/src`) [A-044] (SRC, high). Any multi-agent Space needs that breaker in code.

### 5.7 Evaluation

`benchmarks/buzz-dataset` (10 Harbor tasks, regression k=1 and workflow k=3) grades product behaviour: reply placed in the user's thread, `p`-tag callback, non-waking narrative names, exact channel membership and TTL, cross-thread isolation, ambiguous-name mentions, cold memory retrieval. The orchestra adapter runs the real `buzz-acp -> buzz-agent -> buzz-dev-mcp` tree in the task container with fresh keys and a private channel per trial, snapshots relay state for graders, and fails the trial if the export fails [A-060] (SRC). Some graded behaviour is deliberately absent from the task prompt so it must come from the production base prompt.

---

## 6. Realtime, sync and scale (category 9)

**Transport and lifecycle (SRC)**: WS at `/`, NIP-11 on `GET /`, 5 s AUTH timeout, ping every 30 s with three missed pongs, bounded send queue with a grace counter (3), separate control and terminal-control channels drained before cancel so ban and NIP-FI denial frames always arrive, 5 s restart-close ack; limits 64 KiB frame, 1,024 subs per connection, 500 rows per historical filter, filter-query concurrency 4, 128 explicit `#h` values per request (`handlers/req.rs:26-49`) [A-009].

**Cross-pod fan-out (SRC)**: Redis topics `buzz:{community}:channel:{id}` and `buzz:{community}:global`, `SUBSCRIBE` driven by refcounted `retain_topic`/`release_topic` with a debounced unsubscribe, dedicated pub/sub connection, 4,096-slot broadcast buffer, local-echo dedup; the bus harness measured a 64x ingress reduction at 64 communities (bus only, not end to end) [A-006]. Two side channels: `cache-invalidate` (pure idempotent key drops; loss is harmless) and `conn-control` (imperative disconnect commands; DB ban row is the durable backstop), kept separate on purpose [A-007]. Topics are a routing boundary, not an authorization boundary: the relay re-checks access before delivery (`topic.rs` header).

**Ordering and resume**: no server-assigned monotonic sequence. Ingest accepts bounded future skew (900 s); a deferred commit-time trigger rejects channel events older than a floor (960 s) when the writer session has the GUC; the replica fence proves completeness for keyset pages with an ordered heartbeat (exclusive advisory lock, sample `S`, oldest open transaction from `pg_stat_activity`, token committed last); clients resume with a **1,870 s lookback**, paged 500 rows with 4 concurrent pages, REQ replay in batches of 8 spaced 50 ms, a passive stall watchdog that never writes to the socket, event-driven resume triggers (online, focus, visibility) because WKWebView throttles timers, and a terminal latch after AUTH denial [A-012, A-015] (SRC). Cost of the model, paid in engineering.

**Rate limiting (SRC)**: Redis Lua `INCR`+`EXPIRE`, human and agent tiers (agent detected by `agent_owner_pubkey`), WS flood budget with a 5 s burst window, per-minute message quota for persistable events only, community-scoped keys, fail closed to `Unavailable`; fixed windows allow 2x bursts at boundaries (code comment); observer frames capped at 100/s per agent in process. ARCHITECTURE.md:949 saying no limiter is enforced is stale [A-010]. Whether quotas are aggregated per owner across agent keys (NIP-AA recommends it) was not confirmed.

**Presence and typing (SRC)**: leases with TTL; typing as a Redis ZSET with a 5 s window; ephemeral kinds never stored; availability is tri-state with an explicit unknown (failed or pending read is unknown, never offline) [A-008, A-052] (DOC).

**Reads and consistency**: see 3.5; reads that gate writes hit the writer; canvas saves set `consistency: strong` [A-013, A-090].

**Push (SRC/DOC)**: relay enqueues matches transactionally (`push_match_queue`, claim 30 s, batches of 64, 8 attempts, poison reaper), writes `push_wake_outbox` with a unique `(community, endpoint_hash, event_id)`, delivers through a separate gateway that verifies Apple App Attest, custodies tokens under independent AEAD keyrings, applies replay and endpoint quotas in its own database, and sends a fixed reconnect wake; the app reconnects and fetches signed content [A-022, A-031]. NIP-PL leases are kind 30350 with a random per-origin installation id, mandatory expiry (at most 30 days), narrowed filters only (`#p` self, `#h`, `authors`; no `since`/`limit`/`search`), classes silent < default < time_sensitive < urgent (urgent reserved for approvals), `suppress.p_tags_max` hellthread gate. Relays advertise `buzz-push-v1`, not `nip-pl` (docs/buzz-push.md). Experimental, APNs only. Suppression while the desktop is active is a draft spec: 10-minute inactivity, renewals every 10 s, eligibility expiring 30 s after the last confirmation, fail toward sending [A-033] (DOC; no code found).

**Voice (SRC)**: `WS /huddle/{id}/audio`, Opus v2 frames with an 8-byte header, no SFU, soft cap 25 peers, 255 peer-index pool, bounded drop-on-full audio and a never-dropping control channel, protocol versions 1-3; cross-pod rooms go through the owner pod via fenced mesh [A-083]. A huddle is an ephemeral private channel (kind 9007 with `ttl=3600`) linked by creator-signed 48100/48103 events, so chat, reactions and transcript inherit room authorization and expiry [A-084, A-082]. Media is relay-readable Opus (no encryption code found in `audio/`; a comment in the mesh wire file claims otherwise: INF) [A-083 notes].

**Encryption**: none for channels and DMs (server-managed so admin, search and eDiscovery work); NIP-44 for owner-private side data (read state, engrams, metrics, observer frames, push leases, reminders); NIP-17 gift wraps accepted but not indexed [A-020] (SRC).

**Scale and deploy**: stateless pods; Helm with HPA (min 5, max 15, CPU 65%, custom `buzz_ws_connections_active` target 5,000 per pod), PDB, 60 s grace; separate operator listener for `/_liveness`, `/_readiness`, `/_status`, `/_mesh`; storage accounting by an external snapshot worker, not the relay; DB telemetry leader-only and replica-only, paired with availability gauges so absence is never read as zero [A-024, A-077, A-114]. Stated targets are 10K humans, 50K agents, about 600K events/day, fan-out under 50 ms p99; **no evidence of load tests at those numbers** (VISION.md:198, DOC; absence is INF). Client perf budgets exist: typing latency with 8 simulated busy agents, cold/warm switch long tasks, scroll [A-115] (SRC).

**Mobile**: VISION_MOBILE.md is an offline-first intent (derived, discardable store; pending/sent/failed states). The pubspec has no embedded database (only `shared_preferences` and secure storage), so the intent is largely unrealized at HEAD [A-101] (INF, medium; the doc itself says not all intents are realized).

---

## 7. Governance, security, compliance (category 10)

**Strengths**
- **Host binding first**: `resolve_host` before any handler; unknown host fails closed; `h` tags, token stamps and NIP-98 URL hosts are checked against the resolved community; `TenantContext` has no `Default`, no `Deserialize`, no parse-from-client path (and says honestly it is a lint-and-review fence because constructors are `pub`) [A-065] (SRC `buzz-core/src/tenant.rs:10-30`).
- **Typed admission plus DB fences**: `AdmittedTx` wraps a transaction that took the community admission lock; event-write helpers require it; `compile_fail` doctests prove a raw transaction or another community is rejected. Triggers attach a community write fence to every table that has `community_id` (`schema/community_write_fence.sql`); commit-time checks stay authoritative during migration [A-068] (SRC).
- **Formal models**: TLA+ `MultiTenantRelay.tla` (1,142 lines, invariants such as `Inv_NonInterference`, `Inv_ReadConfinement`, `Inv_AdmissionFence`), Tamarin `MultiTenantAuth.spthy`, `GitOnObjectStore.tla`; every invariant mutation-tested; `buzz-conformance` replays runtime traces with an **independent** checker that shares no helpers with the emitter and fails on coverage breach [A-066] (SRC).
- **Audit**: per-community SHA-256 hash chain (`pg_advisory_lock` single writer, panic-safe), 11 actions in code (docs say 10: `MediaUploaded` added), `verify_chain`; AUTH and ephemeral events never logged [A-069] (SRC). Agent tool calls are outside it.
- **Moderation**: reports are private structural state (never events), actions are signed commands with role guard rails, bans bite at authentication and cascade owner→agents, timeouts are write blocks that still allow reports and unban/untimeout commands (`ingest.rs:2208`), tombstones, best-effort notices, two layers (community vs platform escalation hook) [A-071] (SRC/DOC). No automod, no volunteer tier.
- **Kind-level read gates**: `AUTHOR_ONLY_KINDS`, `SHARED_GATED_KINDS`, `RESULT_GATED_KINDS`, `P_GATED_KINDS` live next to the registry and are enforced by one predicate on REQ, COUNT, fan-out and search [A-072] (SRC `kind.rs:129-215`).
- **Deletion**: whole-community deletion is a durable staged state machine (submitted, inventoried, approved, fenced, drained...), idempotent by request UUID, abortable until destruction may have begun, executed by a one-shot job under a generation lease with Kubernetes `Forbid` and zero retries because durable rows own retry. Owner consent is asserted by the operator, not signed by the owner (docs say so plainly) [A-070] (SRC/DOC).
- **Enterprise identity**: NIP-FI with shadow-then-enforce rollout and an issuer-to-relay disconnect API; no SAML, no SCIM [A-073]. The scout note "SSO not found" is wrong: OIDC-style assertions exist; only SAML/SCIM are absent.
- **Operator plane**: separate admin host with NIP-98, Operator vs Moderator tiers, reports/feedback/communities/members/restrictions, outbox-based admin actions [A-113]; hosted-community provisioning, archive, transfer, delete over operator-signed NIP-98 [A-079].
- **Hardening and supply chain**: enumerated-deny SSRF policy over IPv4/IPv6 special-use ranges (including embedded IPv4), constant-time webhook secrets, Redis replay guard, hashed single-use approval tokens, SLSA provenance plus a deployment-eligibility predicate, cargo-deny, pinned toolchains, security CI, closed-label metrics [A-074, A-076, A-077] (SRC).

**Gaps and risks**
- **RLS is an axiom, not an installed control**: no `ENABLE ROW LEVEL SECURITY` anywhere in `schema/`, `migrations/` or Rust; only the proof document mentions it [A-067] (SRC absence, INF). Tenant safety rests on application discipline plus fences.
- **Media blob reads** need a host-scoped Blossom grant but are not tied to the message's channel ACL: membership plus a known hash suffices (conformance table, DOC) [A-078].
- **Not present** (grep, INF medium): DLP, data residency, SCIM, compliance export / eDiscovery tooling (VISION claims eDiscovery "works" because content is server-readable), legal hold, per-room retention policies (retention exists as TTL channels, ephemeral kinds, local observer-frame retention).
- **Agent authority is ambient**: sessions share credentials, workspace, caches and network with the owner (Appendix C of the IFC doc, DOC); auto-approved permissions [A-045]; no budgets [A-064]; no hop limit [A-044].
- **Relay signing key** custody: a compromised community key forges roster, DM-visibility and window overlays.
- **Operator and tenant planes share a process**; separation is host qualification and middleware ordering.
- **NIP-OA credential** is a reusable capability; `created_at` conditions are agent-controlled unless AUTH freshness is enforced; `kind=` clauses are not evaluated at admission (NIP-AA says so) [A-036].

---

## 8. Client shell (category 11)

- **Desktop** = Tauri 2 + React 19, TanStack Router/Query/Virtual, Tiptap, Radix, Tailwind; desktop Rust crate holds 377 unique Tauri commands (inventory count, not re-verified). The relay socket lives in Rust: frames are coalesced in 8 ms windows capped at 7,680 serialized bytes to stay under Tauri's 8,192-byte direct-eval threshold; one shared session per `(relay, pubkey)` multiplexes archive sync, persona catalog and unread catch-up [A-016] (SRC).
- **Caches**: `channel_head_cache.rs` (a paint accelerator only, 32 channels, 1 MiB per row) and a user-driven SQLite archive that also keeps ephemeral observer frames the relay will never store (30-day default retention); not a general offline store [A-017] (SRC).
- **Media**: `buzz-media:` custom scheme plus loopback proxy that mints per-request auth, retries once on 60 s token expiry for range streams, forwards immutable cache headers, refuses non-range bodies over 20 MiB [A-112] (SRC).
- **Deep links** `buzz://` with de-duplicated pending queues (community, navigation, entity) so cold-start links are not lost; entity-host allowlist; invite code and policy receipt in the URL [A-095].
- **Tray and power**: Running/Recent agent items fed by a small projection from the webview; macOS IOKit assertion while agents run [A-062, A-102].
- **Auto-update**: `tauri-plugin-updater` registered only in configured release builds; endpoint and key from build env (`tauri.conf.json:43` has an empty endpoints list); release lanes for desktop, relay and mobile are independent, with immutable candidate PRs where merge is the authorization event, signed macOS canary then promote; Windows is unsigned and agents need Git Bash [A-096].
- **Multi-community**: rail with drag sort, per-community theme, cross-community unread observer, N sockets for N communities, identity archive and shared identity [A-097].
- **Flags and overlays**: `preview-features.json` (workflows, projects, pulse, forum, agent-managed profiles), `protectedFeatures/internal.ts` vs empty `public.ts` (Bestie is internal-only), build-time enterprise relay allowlist and adapter URL, LaunchDarkly evaluator on the relay [A-099].
- **Accessibility and i18n**: aria in 416 desktop files, reduced motion in 74, WCAG 2.1 AA target in VISION; review-proven rules require one owner per actionable label and every input modality tested; **no i18n library anywhere and no command palette** (Cmd+K is search) [A-102] (SRC counts from scout; INF absence).
- **Mobile** (Flutter, hooks_riverpod): push NSE, biometrics gate for identity actions (`sensitive_action_authorizer.dart`), secure storage, deep links, age gate, huddles MVP (foreground only) [A-101].
- **Testing**: mock-IPC e2e bridge, 189 files in `desktop/tests/e2e`, perf project under agent load, nextest per crate, "tests must bind the production seam and be falsifiable" as written rule [A-098, A-115].

---

## 9. Best in class, weaknesses, drift

### 9.1 Best in class (relative to what the audit set can show; comparison to Slack/Teams is INF)

| Area | Why | Records |
|---|---|---|
| Tenant isolation engineering | Host binding, typed admission, fences, TLA+/Tamarin with mutation gates and runtime trace replay. Slack and Teams are closed and publish no equivalent (INF) | A-065, A-066, A-068 |
| Vendor-blind mobile push | APNs carries only a fixed wake; leases are encrypted, self-scoped, expiring | A-031 |
| Owner-private agent telemetry and usage | Encrypted, ephemeral frames plus durable encrypted metrics with delta-reliability semantics | A-047, A-049 |
| Agent identity and delegation | Offline-verifiable owner attestation, membership derived from owner, revocation cascades to live sockets | A-036, A-037 |
| Mid-turn human-agent hand-off | Steer / supersede / owner-interrupt with explicit prompt framing | A-040 |
| Exact-identity mentions and invitations | Label bound to one key, ambiguity blocks, publish-time re-authorization | A-107, A-108 |
| Replica-safe pagination | Commit-time floor plus heartbeat fence proof | A-012 |
| Remote-agent lifecycle rigor | M1 no control channel, five invariants, self-reaping | A-053 |
| Agent behaviour benchmarks as product tests | Real stack in the container, relay snapshot grading | A-060 |
| Documentation honesty | Specs name their trust boundaries and known defects | whole repo (DOC) |

### 9.2 Weaknesses and limitations

1. **Autonomy over supervision**: auto-approved permissions, ambient authority, host-level shell, no sandbox, no browser (Grok-style "agent PC" is absent) [A-045, A-054].
2. **Unbounded agent chatter**: no hop limit in code [A-044].
3. **Cost control absent**: no budgets or allowances [A-064].
4. **Observability is owner-only**; room members cannot see tool cards [A-047].
5. **Ephemeral approvals**: owner-reviewed drafts ride ephemeral frames; if the owner's client is offline the request is lost [A-046].
6. **Workflow approvals unwired** (WF-07/08) [A-087].
7. **Two read-state systems that do not synchronize** [A-019].
8. **Pure-protocol cost**: client-signed timestamps, time-based resume overlap, replica-fence machinery [A-012, A-015].
9. **Single large relay binary** (158K lines) carrying chat, media, git, audio, push, admin, workflows.
10. **No semantic search, DLP, residency, SCIM, eDiscovery export, video, screen share, recording, i18n, command palette** [A-030, A-078, A-083, A-102].
11. **Mobile offline-first not realized**; push is Apple-only and experimental; suppression unimplemented [A-033, A-101].
12. **Scale unproven**: targets only; no load evidence [A-009].
13. **Key loss is unrecoverable**; Nostr onboarding friction is an admitted cost (VISION_SOVEREIGN.md, DOC).
14. **Spec debt**: some drafts lead the code (NIP-PMA rejected, scheduled send, polls, saved items, job kinds, mesh tunnel without a product consumer: `api/mesh_demo.rs` says nothing in the product calls `ReliableStreamRouter::join` yet) (SRC).

### 9.3 Doc-versus-code drift (code wins)

| Claim in docs | Code at HEAD | Evidence |
|---|---|---|
| ARCHITECTURE.md:949 "no rate limiting implementation" | Redis limiter wired for WS, HTTP and GIF paths; agent tier | `state.rs:1290`, `rejection.rs:68`, `admission.rs` (SRC) |
| ARCHITECTURE.md:531-545 `PSUBSCRIBE buzz:channel:*` global bus | Per-community refcounted `SUBSCRIBE` topics | `topic.rs:43`, `lib.rs:217-240`, `perf/RELAY_BUS_SCALING.md` (SRC) |
| ARCHITECTURE.md:300-306 presence fan-out local-only | Presence now falls through to the shared global ephemeral path | `handlers/event.rs:986-990` (SRC) |
| ARCHITECTURE.md:598 ten audit actions | Eleven (adds `MediaUploaded`) | `buzz-audit/src/action.rs` (SRC) |
| ARCHITECTURE.md ALL_KINDS "127" / "80" | 133 `KIND_` consts | `kind.rs` (SRC) |
| ARCHITECTURE.md:7 "no gossip, no replication" | True for chat state; the inter-pod mesh has gossip membership | `buzz-relay-mesh/src/lib.rs` (SRC) |
| VISION.md status "MCP server - full feature surface" | No Buzz MCP crate; CLI-first; only `buzz-dev-mcp` | `crates/`, AGENTS.md:318 (SRC) |
| VISION.md "desktop supports all seven surfaces" | Pulse, Projects, Forum, Workflows preview-gated | `preview-features.json` (SRC) |
| VISION_ACTIVITY.md "twelve render classes" | 15 in code | `agentSessionTypes.ts:24-39` (SRC) |
| NIP-AA "active sessions not forcibly terminated" | Sockets admitted under a revoked owner are closed, cluster-wide | `state.rs:855-905` (SRC) |
| multi-tenant-relay.md NIP-98 seen-set per pod | Redis `SET NX EX` | `nip98_replay.rs` (SRC) |
| remote-agents.md Known Defects 3 and 4 | Launch block and reaper exist at HEAD | `agents_deploy.rs:44-49`, `config.rs:512` (SRC) |
| mesh wire comment "client frame NIP-44 encrypted end to end" | No encryption code in huddle audio paths | grep (INF) |
| Scout inventory "human approval of tool calls via permission broker" | Harness auto-approves | `acp.rs:2005` (SRC) |
| Scout inventory "isolated_execution = agent sandbox" | One-shot task session, not isolation | `isolated_execution.rs`, `TASKS.md` (SRC) |
| Scout inventory "no NIP-AB spec" | `crates/buzz-core/src/pairing/NIP-AB.md` | file (SRC) |
| Scout inventory "SSO not found" | NIP-FI/NIP-FA exist; SAML and SCIM absent | `nip_fi_config.rs` (SRC) |
| Scout inventory "38 crates" | 33 directories under `crates/` | `ls crates` (SRC) |

---

## 10. What Verevon Space should copy, adapt or avoid

Grounding (DOC, `v-space-backend.baseline.md`): Space is authority-first with Control-signed short-lived decisions and receipts, but every post is a model turn (no human-only message), the room polls every 6 s (no push), non-owners cannot reply into another member's thread, no reactions, edits, attachments, DMs, notifications, personal agents, message search, offline cache, per-room memory or cost, approvals recorded but not resumed, and agent-to-agent delegation absent. Architecture rules: no cross-plane DB access; Space access never widens an owner-plane ACL; ZDR must propagate.

### 10.1 Copy (adopt nearly as-is)

| # | Pattern | Buzz source | Verevon change |
|---|---|---|---|
| 1 | Subscribe-first, backfill-second, dedupe by id; terminal-close releases client fences | A-005 | Required for the move from 6 s polling to a push transport |
| 2 | Delivery-time re-authorization at one chokepoint, fail closed, shared by every delivery path | A-004 | Re-check the current Control audience on every push to a socket; decisions live 2 minutes, so re-validate per delivery rather than per subscription |
| 3 | Refcounted per-Space topics with debounced unsubscribe; routing is not authorization | A-006 | Per-Space topics (not an org firehose) on the Application plane |
| 4 | Idempotency by construction (client key per post, `ON CONFLICT DO NOTHING`, pending/sent/failed) | A-003 | Stops retries from creating duplicate billable turns |
| 5 | Exact-key mentions as a structured recipient list; ambiguity blocks send; invite vs reference-only; re-authorize at publish | A-107, A-108 | Prerequisite for mention notifications and spoof-proof agent addressing |
| 6 | Human-only post as a first-class event distinct from a model turn | A-001, A-002 | Closes Verevon's largest functional gap; agents are addressed by mention |
| 7 | Per-binding author gate (owner, listed members, any member, nobody) with fail-closed lookups | A-041 | Extend `trigger_modes` with an author class |
| 8 | Mid-turn modes queue / steer / supersede / owner-interrupt with explicit prompt sections | A-040 | Verevon has a queue route; add the others |
| 9 | Session scope as an explicit key (room, thread, DM, task) carried through authorization | A-039 | Decide before building agent-to-agent delegation |
| 10 | Owner-reviewed drafts for privileged agent actions: agents propose, humans decide, nothing claims to exist before approval | A-046 | Matches "agents cannot bind agents or change roles"; make the proposal durable, not ephemeral |
| 11 | Tri-state liveness (online, offline, unknown); presence as a lease; snapshot/delta inclusion contract | A-008, A-052 | Add to Space roster |
| 12 | Typed admission value required by write functions | A-068 | Make "authorized by Control for this Space" a type in the Rust BFF so a missing check fails to compile |
| 13 | Deny-by-default plus invariants modelled and trace-replayed, independent checker, coverage breach fails | A-066 | Template for release gates R-1 to R-5 |
| 14 | Per-kind read policy declared beside the event registry and enforced by one predicate | A-072 | Keep new Space event types from leaking across audiences |
| 15 | Capability document per Space (what org policy enables) so clients do not discover denials by 403 | A-026 | Pair with deny-by-default `space_effect_policies` |
| 16 | Shadow mode before enforcing any new auth rule | A-073 | Roll out new decision requirements without breaking rooms |
| 17 | Remote body rules: self-reaping inactivity timer on its own clock, deterministic name for single-instance, secrets only in the launch payload, key-name lint on config | A-053 | Sandbox lifecycle in Model Plane |
| 18 | Hash-chain audit per Space plus abort-window deletion with digest-bound approval | A-069, A-070 | A Space-scoped audit view and non-personal Space deletion |
| 19 | Agent behaviour evals as release gates (thread placement, mention hygiene, cross-thread isolation) | A-060 | Alongside R-1 to R-5 |
| 20 | Perf budget under agent load (typing latency with N busy agents) | A-115 | Before enabling push realtime |

### 10.2 Adapt (idea is right, implementation differs for Verevon)

- **Owner-private plus room-visible activity.** Buzz gives owner-only encrypted telemetry; Verevon needs a second, **room-visible semantic stream** (verb, object, outcome cards: A-048) fed by Model Plane run events, with the raw rail owner-only. Keep Buzz's `seq` gap detection and cancel control frame [A-047].
- **Audience-bound agent instances plus a keyless broker** [A-055]. Maps onto Verevon recipient-audience snapshots (hash and revision): bind an agent run to one audience revision, label memory with it, and require an exact human declassification grant to publish wider. Strongest fit with "Space access never widens an owner-plane ACL".
- **Server-signed projections** [A-105] as versioned Space roster, audience-revision and presence snapshots; use a server-assigned sequence instead of Nostr timestamps to avoid Buzz's skew machinery.
- **Server-derived read state** [A-019]: a read frontier per (user, scope) with `mark_through` anchors computed by the owning plane; never show zero when unavailable. Skip Buzz's second encrypted-blob system.
- **Reminders as an encrypted due-time primitive** [A-106] for scheduled send and agent wake-ups under ZDR.
- **Push**: copy the blind constant-wake design and relay-side suppression with renewal leases [A-031, A-033] if mobile appears; keep content out of any vendor (Novu).
- **Space memory with an audience label from day one** [A-050]: Buzz's per-(agent, owner) scope is the cautionary tale.
- **Ephemeral child Space with TTL** for huddles and agent task rooms [A-082, A-084].
- **Fenced generation leases** for one owner per live session when Verevon runs multiple realtime replicas [A-023]; the fenced-header rule (membership is a hint, the lease is the arbiter) is the reusable part.
- **Replica rules** [A-012, A-013] only if Verevon adds read replicas or projections over Control/Convex data: reads that influence writes or permissions read the authority.
- **ACP harness as a Space runtime option** [A-038, A-058]: ACP-speaking external agents could sit in Spaces as service members under Control roster and effect policies.
- **Snapshots** [A-109] for portable, secret-free agent definitions.
- **Authenticated media through the BFF** [A-112] with immutable content-addressed caching.

### 10.3 Avoid

- Host-level shell as the "agent PC" and ambient owner authority [A-054]; Verevon's credential-free sandbox leases and Quarry-mediated browser are stronger.
- Auto-approving permissions and bypass-by-default [A-045]; keep `require_confirmation` and finish approval continuation before exposing approval steps (Buzz's WF-08 shows the failure mode: do not ship suspend without persist and resume as one tested path).
- Prompt-only loop prevention [A-044]; implement a per-thread agent-hop budget and cooldown in orchestration.
- Per-owner memory shared across rooms [A-050].
- Ephemeral-only transport for approvals and privileged drafts [A-046].
- Two parallel read-state systems [A-019] and client-clock-based resume [A-015].
- Proof axioms the schema does not install [A-067]: make backstops executable or say they are unimplemented.
- Stale architecture docs: the drift table above is the cost; Verevon has its own stale-doc register, keep it enforced.
- No-budget agents [A-064]: add per-Space and per-agent allowances.
- Blob reads decoupled from room ACL [A-078].
- Retrofitting i18n and a command palette [A-102].

### 10.4 Verevon-specific mapping (Space gaps to Buzz patterns, DOC baseline)

| Verevon gap (baseline) | Closest Buzz pattern | Notes |
|---|---|---|
| Human-only message absent | Events by kind, ingest pipeline [A-001, A-002] | Separate "human post" event from "invoke agent" |
| 6 s poll, no push | Subscribe-first, delivery chokepoint, refcounted topics [A-004, A-005, A-006] | Server sequence instead of time lookback |
| Non-owner reply refused | Scope key and NIP-10 reply tags [A-028, A-039] | Authorization on the append decision, not owner-bound threads |
| Person mentions are text | Exact-key mentions [A-107] | Also enables notifications |
| No Space notifications | Zero-default classes, blind push, due-time reminders [A-031, A-032, A-106] | Respect ZDR |
| Personal agents impossible | NIP-OA/NIP-AA derived access [A-036, A-037] | Owner membership grants agent access, never owner roles |
| Approval granted but run not resumed | Permission broker invariants, owner-reviewed drafts [A-045, A-046] | At-most-once, fail-closed, one absolute deadline |
| No per-room memory | NIP-AE plus IFC labels [A-050, A-055] | Audience-labelled |
| No cost per Space | NIP-AM records [A-049] | Add enforcement Buzz lacks |
| No agent-to-agent delegation | Coordination by mention [A-059] | Add hop budget [A-044] |
| No unread aggregate | `/buzz/v1` frontiers [A-019] | Owner-plane projection |
| No Space audit view | Hash-chain audit [A-069] | Per-Space chain |
| Rooms cannot be deleted | Deletion state machine [A-070] | Reuse existing per-plane receipts |
| No search in rooms | Candidates then authorize [A-030] | Ensure ZDR on any index |
| No calls | Huddle as ephemeral child channel [A-083, A-084] | Probably buy an SFU; Buzz caps at 25 peers, no video |

---

## 11. Open questions

1. Is huddle media meant to be end-to-end encrypted? A mesh comment says so; no code does (A-083). Is any E2EE planned beyond DMs (VISION.md:102 says only "future consideration")?
2. Will NIP-RS and `/buzz/v1` converge, and is the pivot a signal that protocol-pure read state cannot carry counts? (A-019)
3. What is the plan for a code-level agent-to-agent breaker, and for a human approval UI for tool calls? (A-044, A-045)
4. Does the relay aggregate rate limits per owner across virtual agent members as NIP-AA recommends? (A-010; not confirmed in code.)
5. Does a production deployment add row-level security out of tree? (A-067; this checkout has none.)
6. How far has the inter-pod mesh tunnel gone toward a product consumer (`berd`/goose sessions)? (A-023; demo route only in this repo.)
7. What load has the relay actually been tested at, against the 10K human / 50K agent target? (A-009.)
8. How much of Buzz Desktop's enterprise behaviour lives in the separate `block/buzz-app` repo (federated identity client, internal-only features)? (A-099; docs say the desktop client for NIP-FA is implemented there and the in-repo guide lists most steps as planned.)
9. Will push leave Apple-only experimental status, and will suppression ship? (A-031, A-033.)
10. Will Buzz Mesh ship in public builds (today a non-default cargo feature) (A-063)?

---

## 12. Verification log (second pass, this session)

Re-verified directly in source or docs (SRC/DOC): ingest pipeline and fan-out chokepoint (`handlers/event.rs:115-215`, `subscription.rs:379`), connection constants (`connection.rs:30,520,1075`, `handlers/req.rs:26-49`), Redis topics, side channels and presence TTL (`topic.rs`, `lib.rs:217-240`, `conn_control.rs`, `cache_invalidation.rs`, `presence.rs:16`), rate limiter wiring (`state.rs:1290,1487`, `rejection.rs:50-100`, `admission.rs`, `handlers/event.rs:1140`), NIP-OA/NIP-AA code paths (`handlers/auth.rs:69,208`, `handlers/ingest.rs:2208-2290`), harness config (`config.rs:62-100,355-383,464-512`), permission auto-approval (`acp.rs:1265,2005-2070`), absence of hop limits (grep), workflow stubs (`executor.rs:657-664,738`, `lib.rs:230-247`), AdmittedTx (`runtime/admitted_tx.rs:1-80`), replica fence (`runtime/replica_fence.rs:1-60`), absence of RLS (repo grep), no Buzz MCP crate, Kubernetes pod hardening (`pod.rs:131-168`), reaper and launch block, desktop socket batching and caches (`native_websocket_batch.rs:9-22`, `native_relay_client.rs`, `channel_head_cache.rs`), media proxy (`media_proxy.rs`), mobile pubspec (no embedded DB), crate sizes, HPA values (`deploy/charts/buzz/values.yaml:40-70`), 15 render classes (`agentSessionTypes.ts:24-39`), evidence file existence and line bounds for all 485 evidence strings in the JSON (automated check: 0 missing, 0 out of range).

Not re-verified (inherited from the first pass or the scout, medium confidence): the Tauri command count, accessibility counts, i18n absence, Windows/Linux packaging details, desktop onboarding beyond the kickoff module, the 218-file projects feature, the mobile app beyond `pubspec.yaml` and folder layout, the formal NIP-PL/NIP-RS Python models, huddle STT/TTS internals, and all Slack/Teams/Grok Bot comparisons (INF).

Secret-bearing files skipped: `desktop/.env.e2e`, `.env.example` (BUZZ_* names only), `desktop/src-tauri/testdata/hpke_nsec_backup_v1.json`, `.intersect/sadscan.yaml`, `.release/desktop-candidate.json`. No per-user data directories were touched.

---

## Appendix A. Record index by category

(See `buzz.architecture.features.json`; ids are `buzz-architecture-NNN`.)

- **Realtime & sync architecture (30)**: A-001 to A-027, A-105, A-114, A-115.
- **Messaging core (4)**: A-028, A-029, A-107, A-108.
- **Search & discovery (1)**: A-030.
- **Notifications & attention (5)**: A-031 to A-034, A-106.
- **Agents in the room (33)**: A-035 to A-064, A-109, A-110, A-111.
- **Governance, security & compliance (15)**: A-065 to A-078, A-113.
- **Org & membership (4)**: A-079 to A-082.
- **Calls & meetings (3)**: A-083 to A-085.
- **Apps & extensibility (3)**: A-086 to A-088.
- **Collaborative artifacts (5)**: A-089 to A-093.
- **Client shell (10)**: A-094 to A-102, A-112.
- **UX patterns & layout (3)**: A-103, A-104, A-116.
