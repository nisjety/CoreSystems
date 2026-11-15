# Buzz (block/buzz) deep dive, product facet (Audit 1, Verevon Space)

Auditor: deep-dive analyst, Sonnet. Read-only audit of `C:/dev/buzz` (HEAD `799cfd705`, Rust workspace 0.1.0, desktop 0.5.28). Nothing was built, run, installed or modified. Companion file: `buzz.product.features.json` (180 records, ids `buzz-product-001` to `-180`; below they are cited as `#001` to `#180`).

Provenance tags: **SRC** = read directly in code, schema or manifests. **DOC** = repo documentation (README, VISION*, ARCHITECTURE, NIP drafts, docs/); intent, not proof. **INF** = my inference, basis stated. **REA** was not used: Buzz is open source and was read directly. Confidence is high unless stated. Peer statements (Slack, Teams, Grok Bot, Verevon) come from the sibling audit files in `findings/t1/` (slack, teams and grokbot inventories and deep dive, `v-space-backend.baseline.md`, `v-space-ui.baseline.md`) and are tagged DOC or INF accordingly.

## 0. Method, evidence discipline and honest limits

- **Evidence base.** The scout inventory (`buzz.inventory.md`) was used as a map only. I re-read what I relied on in code, schema and docs, and I found it wrong or stale in about a dozen places (section 9). Where docs and code disagree, code wins.
- **Anchors.** Every record cites `path:line` (repo-relative, forward slashes) or a path. A script checked that every cited path exists and every line number is inside the file (703 evidence entries, 0 failures). I then reviewed the line content of nearly every anchor and corrected about 40 that pointed at the wrong line. Many anchors point at the start of a function, a heading or a type rather than the exact statement; read them as "start here".
- **Not read in depth.** The 38 Rust crates were read selectively (relay ingest, auth, audit, pubsub, workflow, acp, agent, core kind and git permission modules, plus module headers). The Flutter app was skimmed (pubspec, folder layout, home page, age gate, push extension). The very large desktop frontend (hundreds of feature files plus tests) was read by feature, not exhaustively. I did not open `desktop/.env.e2e`, `.env.example` values, the HPKE key-backup fixture, `.intersect/sadscan.yaml` or `.release/desktop-candidate.json` (secret-bearing or release-config files skipped). No per-user data stores were touched.
- **Static only.** I did not run Buzz, so no runtime behaviour, latency or UX feel is claimed. "Works" means the code path exists and is covered by tests I could see.
- **Quality scores** (1 to 5) are maturity and execution relative to the best peer for that capability (Slack, Teams, Grok Bot), with the reason in each record's notes. A 5 means best-in-class among the references; 1 means absent, dead or broken.

## 1. What Buzz is, in one page

Buzz is Block's open-source (Apache-2.0) workspace where humans and agents share rooms, on a relay you host yourself. Its premise is that the relay is the workspace: one signed-event log carries chat, reactions, workflows, canvases, git state, approvals, huddle lifecycle and agent telemetry, and one identity model (a secp256k1 keypair) covers people and agents (DOC `VISION.md:1-31`; SRC `crates/buzz-core/src/kind.rs`). A **community** is a tenant addressed by URL; one deployment can host many (SRC `schema/tables/public/channels.sql:1`, `ARCHITECTURE.md:220`).

For the Verevon question ("Space = Teams + Grok Bot + Slack"), Buzz sits between Slack and Grok Bot and is closest to a **Slack with agents as first-class members**:

- **Slack-like human chat** is solid: stream channels, forum channels, DMs and group DMs, threads with relay-computed counts, reactions, a rich composer, search with operators, an inbox, reminders, huddles, workflows, canvases (#001 to #023, #049 to #057, #066).
- **Agents as members** is where Buzz is strongest and most original: every agent is a keypair member with a visible owner, a policy for who may address and add it, mid-turn steering, an etiquette contract, an owner-only live transcript, owner-reviewed proposals, and a benchmark that grades room behaviour (#081 to #099, #104 to #116).
- **Grok Bot-like autonomy is weak**: agents run as local processes on the owner's machine (or a bare Pod), the harness auto-approves every tool permission, there is no browser or computer use, no hosted agent PC, no spend cap (#083, #096, #118).
- **Teams-like org structure is thin**: no teams or user groups, no shared channels, no real guest model, no meetings beyond audio huddles, no enterprise compliance stack (#034, #033, #048, #161).
- **Engineering rigor is unusually high** in specific places: formally modeled multi-tenant isolation, a measured Redis bus, server-computed timeline windows, read-your-writes discipline, a TLA+ spec for git refs on object storage (#151, #135, #137, #138, #122).

## 2. Architecture

### 2.1 Components (SRC unless noted)

```
Clients                              Server (buzz-relay, Rust, Axum)                    State
Tauri 2 desktop (React 19)  --WS-->  NIP-01 on /  (NIP-42 auth)                        Postgres 17 (events partitioned monthly,
Flutter mobile (iOS/Android) --WS-->  HTTP bridge: /events /query /count (NIP-98)         channels, members, thread_metadata,
Web (repo browser + invite)  --HTTP-> REST: invites, members, reports, feedback, gifs,    reactions, audit_log, workflows, ...)
admin-web SPA                         operator, /buzz/v1 (read state), /hooks/{id}        Redis (pub/sub, presence, typing,
buzz-cli (agents, JSON)      --HTTP-> Blossom media /media, git smart HTTP /git/...         rate limits, replay guards, fences)
Agents: buzz-acp (harness) ->        huddle audio WS /huddle/{channel}/audio             S3/Silo (media, git objects)
  ACP agent (goose, claude, ...) ->  in-process: buzz-search (FTS), buzz-workflow,        Pod mesh: iroh QUIC (buzz-relay-mesh)
  MCP (buzz-dev-mcp) + buzz CLI      buzz-audit, buzz-media                              Push: buzz-push-gateway -> APNs
```

- **One relay binary** owns ingest, fan-out, REST, media, git and huddle audio; supporting crates are libraries (`buzz-core` has zero I/O; `buzz-db`, `buzz-pubsub`, `buzz-auth`, `buzz-search`, `buzz-audit`, `buzz-workflow`, `buzz-media`) (DOC `ARCHITECTURE.md:5`; SRC `crates/`).
- **Agents live outside the relay.** `buzz-acp` is a harness that subscribes to the relay as the agent, queues events per session scope, prompts an ACP agent subprocess, and lets the agent act through the `buzz` CLI or MCP tools (#107, #071). `buzz-agent` is Block's own minimal ACP agent; 14 known runtimes plus custom ones can be plugged in (#073). `sprig` is a single multi-call binary that can play harness, agent or git helper depending on argv0 (#147).
- **Desktop is the control surface for personal agents**: it spawns and supervises the harness processes, stores keys in the OS keyring, and keeps Nest, snapshots and archives locally (#083, #110, #100). That is a defining weakness for Verevon's hosted model (section 6).

### 2.2 Data model

- **One event table is the source of truth.** `events(community_id, id, pubkey, created_at, kind, tags jsonb, content, sig, channel_id, d_tag, deleted_at, search_tsv)` partitioned by month; `community_id` leads every key (SRC `schema/tables/public/events.sql:1-58`). The kind integer is the only type switch; 122 kind constants in `kind.rs` (the doc says 127, stale).
- **Projections for hot queries**: `channels` (type stream/forum/dm/workflow, visibility, TTL, topic, participant hash), `channel_members` (role), `thread_metadata` (root, parent, depth, reply counts, broadcast flag), `reactions`, `event_mentions`, `users`, `personal_read_frontiers`, `moderation_reports`/`moderation_actions`, `audit_log`, `workflows`/`workflow_runs`/`workflow_approvals`, `artifact_heads`, `push_*`, `community_*` deletion tables (SRC `schema/tables/public/`).
- **Per-user state as events**: sidebar sections, stars, mutes, sort and theme are NIP-78 kind 30078 blobs distinguished by d tag, merged client-side as per-field LWW registers (#140). Read state has two parallel systems (client-encrypted NIP-RS frontiers and a newer server-derived `/buzz/v1` API) that do not sync with each other (#019).
- **Custom NIPs** (19 drafts in `docs/nips/`): NIP-OA owner attestation, NIP-AO observer frames, NIP-AM turn metrics, NIP-AE engrams, NIP-AP personas, NIP-CW windows, NIP-RS read state, NIP-ER reminders, NIP-PL push leases, NIP-AR channel artifacts, NIP-MP projects, NIP-FI federated identity, NIP-GS git signing, and others. NIP-AB device pairing has no spec file; it exists only in code (SRC).
- **Protocol stance** (DOC `VISION.md:62-77`): conversation state stays Nostr-authoritative; "accessories" that do not fit Nostr (Blossom media, git, derived unread counts, thread-window reads) are served as HTTP extensions on the same store. In practice this produced real friction: Nostr filters cannot say "messages with no reply tag", so NIP-CW had to be invented as an HTTP query extension (#137).

### 2.3 Protocols worth knowing

- **WebSocket NIP-01** with NIP-42 challenge; HTTP bridge with NIP-98 per-request signatures and Redis replay guards; NIP-OA owner attestation lets an agent inherit its owner's admission (#082).
- **Huddles**: Opus frames over a relay-hosted WebSocket room with an 8-byte header; no SFU, no video, 25-peer soft cap (#043).
- **Mobile push**: content-free APNs wake, client fetches and verifies the real event; App Attest enrolls devices (#058).
- **Mesh compute** (optional build feature): relay membership gates a pool of members' GPUs that agents use through a local OpenAI-compatible endpoint (#117).

## 3. Product surface by taxonomy category

Each paragraph cites record ids; the table in section 5 lists best-in-class calls.

### 3.1 Messaging core (#001 to #025)

Table stakes are met: stream channels (open or private, topic, member cap), forum channels, DMs and group DMs up to 9 with idempotent participant-hash channels and per-viewer hide/resurface (#005), threads with broadcast-to-channel and relay-computed counts (#002) shown in a split or focus drawer (#003), edit and delete (including the **owner of an agent editing or deleting the agent's messages**, #006), reactions (#007), mentions bound to exact identities (#009), a Tiptap composer with markdown, code, spoilers and attachments (#010 to #014), voice notes (#014), frame-anchored video review comments (#015), sender-side link previews that never make recipients fetch third-party URLs (#016), GIF search through a relay proxy (#017), drafts that survive restarts and failed sends (#018), read state with unread dividers and manual mark-unread (#019), typing including agents (#020), presence with OS-idle auto-away and expiring status (#021), deep links and entity chips (#022).

Defined but not wired (SRC kind registry only): scheduled send, message pins, saved items; polls and forwarding do not exist (#024, #025). Shared (cross-community) channels do not exist (#034).

Highlights: **mention identity binding** (#009) stops a display name from redirecting a mention (important when many agents have similar names); **video review comments** (#015) are a zero-schema trick (a timecode prefix in a normal thread reply); **sender-side link previews** (#016) are a privacy-respecting alternative to server unfurling.

### 3.2 Org and membership (#026 to #042)

A community is a URL-selected tenant; the desktop holds many in a rail with cross-community unread (#026). Roles are a closed enum (owner, admin, member, guest, bot) with last-owner protection (#027); relay-level membership and operator roles come from NIP-43 (#028); channels are open or private with a directory and self-join (#029); **each agent declares who may add it to channels** (#030); channels can be ephemeral (TTL) (#031); invites are stateless HMAC codes with a join-policy acceptance step (#032). Onboarding creates starter channels and an agent greeter (Fizz) (#036); channel templates bundle a canvas and an agent roster but are stored locally on one desktop (#035). Identity is a keypair with encrypted backup, QR pairing for a second device and an identity archive (#037); NIP-05 handles are per community (#042).

Gaps: no teams or user groups, no `@channel` or `@here`, no shared channels (#034); the guest role is a label with almost no enforcement, and the scoped-token mechanism that would make guests real exists only as dormant code (#033, #080).

### 3.3 Calls and meetings (#043 to #048)

Audio huddles per channel (#043) on a mature desktop media stack (Opus, NetEq, VAD, push-to-talk, reconnect; #044). The standout: **agents join huddles as audio peers** with local speech-to-text, per-agent synthetic voices, human-floor policy and barge-in (#045). Optional transcript posts into the huddle chat (#046); mobile has a foreground-only huddle (#047). There is **no video, no screen share, no recording, no recap, no calendar, no whiteboard** (#048). Teams and Slack are far ahead here; Buzz only informs the voice-agent design.

### 3.4 Search and discovery (#049 to #053)

Postgres full-text search, tenant-scoped, with each hit re-authorized by a community-scoped refetch so search is never the access boundary (#049); Slack-style operators (`from:`, `in:`, `after:`, `before:`) and scoped search from a keyboard shortcut (#050); people and channel discovery (#051). The agent story relies on the same FTS ("have we seen this error before?", #052). No semantic search, no AI answers, no cross-source search (#053).

### 3.5 Notifications and attention (#054 to #065)

The **Home inbox** is a list-plus-detail view with eight filters including **Needs action** (approvals, reminders) and Agents (#054). Notification slots with per-event sounds and thread-aware rules (#055), follow and mute for threads and channels (#056), reminders encrypted to the author with a public not-before for relay scheduling (#057), unread badges with an overflow button (#061), a tray menu showing running agents (#060), and a **content-free mobile push** design that fits zero-retention constraints (#058). A draft spec suppresses mobile push while the desktop is active (#059). Pulse is a social notes feed where agent notes collapse by agent and time window (#065, preview-gated).

Weak: the job-notification sounds are wired to events nothing emits (#062); "zero notification by default" is vision, not behaviour (#063); no do-not-disturb, quiet hours or digests (#064).

### 3.6 Apps and extensibility (#066 to #080)

Workflows are YAML triggers, conditions (evalexpr), templated steps and run traces (#066, #067); inbound and outbound webhooks (#069); an HTTP bridge for signed events (#070). The **agent-first CLI** (JSON in, JSON out, typed exit codes) is the primary agent surface instead of per-tool schemas (#071); MCP tools and lifecycle hooks with injection-safe output (#072); bring-your-own agent runtime through ACP with 14 known harnesses (#073); persona packs and snapshots (#074, with no desktop import path for pack source). Slash commands are pass-through to agents only (#075).

Absent or broken: workflow approval gates fail the run instead of suspending it (#068, same failure mode as Verevon's approval continuation gap); `send_dm` and `set_channel_topic` workflow actions return NotImplemented; no app directory, tabs, forms or OpenAPI (#079); scoped API tokens are dormant (#080).

### 3.7 Agents in the room (#081 to #118), the heart of Buzz

**Identity and authority.** Every agent has its own key, profile and bot membership; its events carry an owner attestation (NIP-OA) and messages show "managed by" (#081). An agent inherits access from its owner: admitted to a closed relay or allowed to push to a protected branch because its owner is, and removing the owner revokes all their agents (#082, #149). Persona is a template, the instance is the member, and an explicit key always opens that exact agent (#114). Availability is conversational presence, never inferred from process state, and unknown is not shown as offline (#113).

**Who can address an agent.** Owner-only by default, then allowlist, anyone, nobody (proactive only), plus same-owner siblings (#088); a separate per-agent policy for who may add it to a channel (#030); mention-to-invite flow when an owned agent is not yet in the channel (#115); consent dialog for non-member mentions (#041); a persistent address lock so the user does not re-mention an agent every message (#104).

**How work flows.** `@mention` wakes a sleeping agent with a publish-first, replay-floor guarantee (#087). While an agent works, new messages **steer** it (cancel and re-prompt with the new text framed as arrived mid-task), queue, supersede, or interrupt only for the owner (#089). Context is per channel by default, optionally per thread or per task (#090). Wake rules can be mentions, all messages or a rule list with a bounded filter expression (#106). A harness pool with per-scope queues and batching prevents one run per post (#107). Context handoff summarizes and continues with a bounded ladder (#108). Heartbeats and workflow schedules give proactive behaviour (#101).

**Etiquette and visibility.** The base system prompt teaches room manners from real failures: always answer a human, never send bare acknowledgements, mention the delegator when done, narrate progress because tool calls are invisible (#091). Agents speak only by explicit publish, so reasoning never floods the room (#092). The **owner**, and only the owner, can watch a live, encrypted transcript with twelve render classes and verb-object-outcome tool cards, send cancel and model-switch controls, and see one consistent working signal with a liveness backstop (#093, #094, #095, #112). A composer activity strip shows working agents with rotating headlines (#174).

**Human in the loop.** There is **no tool-permission approval**: the harness auto-selects allow-once (#096). Buzz's real human-in-the-loop is different: an agent can only **propose** creating or editing an agent or adding a project channel, and the proposal reaches the owner as a prefilled dialog that changes nothing until saved (#097); a setup nudge card tells the owner what to fix when an agent cannot run (#098).

**Memory and workspace.** Owner-readable encrypted engrams, core memory injected every turn, a graph viewer (#099); the Nest folder convention gives every agent durable orientation, plans and an outbox (#100). Memory is scoped per agent and owner, not per room, which creates the cross-room leakage risk that the information-flow design addresses (#154).

**Sharing and distribution.** Personas and teams are private by default, shareable through a community catalog with sanitized projections (#084), as snapshots, or as PNG "agent cards" (#085). Adding an agent or a whole team to a channel is as easy as adding a person, with quick-add and drag-drop (#086).

**Infrastructure.** Local supervised processes under the desktop (#083), lifetime bounds and self-reaping for remote agents (#102), a Kubernetes provider-binary design for remote agents (#103), optional relay-gated shared GPU compute (#117), a reserved-key fence plus OS keyring for agent secrets (#110), per-turn metrics encrypted to the owner but no budget cap or spend UI in the OSS build (#111).

**Quality control.** The **buzz-dataset** benchmark grades room behaviour (reply in the right thread, notify exactly the right person, do not wake agents named in narrative, keep simultaneous requests isolated, retrieve cold memory) on a Harbor adapter that runs the real stack (#116). Nobody else in the reference set has this.

**Absent**: browser or computer use, a hosted agent PC, per-tool approvals, spend caps (#118); chat-only multi-agent coordination with unused typed job kinds (#105).

### 3.8 Collaborative artifacts (#119 to #132)

Channel **canvas**: one Markdown document per channel with full revision history, diff, restore and a relay-checked expected-revision precondition (#119, #120). It is a textarea editor, not co-editing. **NIP-AR** adds typed, revisioned records governed by channel access with compare-and-set heads; only the relay and tests use it, no client (#121). A full **forge**: relay-hosted git over smart HTTP with a formal spec (#122), role-based branch protection (#123), multi-repo projects (#124), issues (#125), pull requests with review requests and desktop-side merge (#126), branch-as-channel (#127), Nostr-signed agent commits (#129). The standout is the in-page **agent chat with selected context, untrusted-data framing and pre-send disclosure** (#128, #180). An in-app terminal docked to a channel with an environment allowlist (#130). Knowledge crystallization is only a plan (#131); there is no task board, file library, wiki or poll (#132).

### 3.9 Realtime and sync architecture (#133 to #147)

Push WebSocket with bounded loops and per-frame rejection addressing (#133); three-tier fan-out that never leaks private channels (#134); a community-scoped Redis bus measured at a 64-fold ingress reduction (#135); an ordered ingest pipeline with atomic head writes (#136); **server-computed windows with exact cursors** (#137); **read-your-writes discipline** (#138); a native Rust socket with 8 ms frame coalescing (#139); **field-level LWW registers for multi-device preferences** (#140); a head-page paint cache that is not an offline store (#141) while the mobile spec demands offline-first with pending/sent/failed semantics (#142); selective encryption with no E2EE for chat (#143); pod-to-pod QUIC mesh with Redis-fenced ownership (#144); subscribe-before-backfill repair (#145); Postgres with unmeasured scale targets (#146); a compose and Helm self-hosting kit (#147).

### 3.10 Governance, security and compliance (#148 to #161)

A complete **moderation loop** (#148) with identity-seam enforcement that cascades from an owner to the owner's agents (#149); a per-tenant hash-chain audit log that is keyless and whose verification is not wired anywhere outside tests (#150); **formally modeled tenant isolation** with conformance replay (#151); durable whole-community deletion (#152); federated enterprise identity assertions with the client flow still planned (#153); a draft **information-flow design** for agents with an unused label crate (#154); an egress guard against key-material leaks with a build-time coverage test (#155); per-principal rate limits (#156); an enumerated-deny SSRF guard with IP pinning (#157). Known weaknesses: media reads are not gated by the originating channel's ACL (#158), retention is nearly absent (#159), and SSO provisioning, SCIM, DLP, eDiscovery export, residency and legal hold do not exist (#161).

### 3.11 Client shell (#162 to #171)

Tauri 2 desktop with Rust-owned state (#162), deep links with a cold-start queue (#163), auto-update and three independent release lanes (#164), keyboard shortcuts without a command palette (#165), a Flutter mobile app (#166), a **web client that is only a repo browser and invite page** (#167), an editor-theme-derived theme engine (#168), real accessibility effort with no automated audit (#169), no localization (#170), and platform rough edges (#171).

### 3.12 UX patterns and layout (#172 to #180)

Community rail, pinned feature menu, grouped conversations and a single resizable right panel (#172); sidebar organization that syncs across devices (#173); composer activity strip (#174); empty-state action cards and an agent greeter (#175); a message row built for mixed rooms (#176); scroll anchoring (#177); **honest-state UX as a discipline** (#178); motion with reduced-motion gating and density (#179); selection-to-context for agent hand-off (#180).

## 4. Notable design decisions

1. **Membership is the only gate.** Channel membership decides read and write; roles are a closed enum; there are no ACL lists (DOC `SECURITY.md:65-75`). This keeps the model auditable and made agent scoping trivial, at the cost of weak guest and external-user stories (#033).
2. **Agents are accountable through an owner chain, not permission flags** (#081, #082). Remove the human and their agents lose access in one step.
3. **Agent text is not the message.** An agent speaks only by publishing; thoughts and tool calls go to the owner's private transcript (#092, #093). This trades live token streaming in the room for a clean, low-noise timeline.
4. **Owner-only transparency.** Observer frames are NIP-44 encrypted agent to owner, ephemeral, never stored by the relay (#093). Strong privacy and control; no shared view for teammates and no relay-side history.
5. **Proposals instead of approvals.** Agents never change structure; they propose to the owner (#097). The same rule the Verevon Space design calls the "future authority wall".
6. **Read-your-writes by construction.** Writer for decisions, replica for display, one-way consistency field (#138).
7. **Protocol-first, then BFF accessories.** The wire is Nostr; whatever Nostr cannot express becomes an extension (windows, read state). The cost is two systems for unread (#019) and spec drift.
8. **Spec before code in risky areas.** TLA+, Tamarin, mutation-tested Python models, regression-gate tables that name the test per behaviour (#151, #122, #058; DOC `docs/formal/`). In UX areas too: `docs/agent-availability.md` lists each regression test next to the rule.
9. **Desktop-hosted agents.** Keys, processes, memory and Nest live on the owner's machine (#083). It simplifies trust and creates every availability, sleep, OS and Windows problem Buzz then has to solve (#171, #087).

## 5. What is best-in-class, and what is table stakes

### 5.1 Table stakes that Buzz meets (commonly shared with Slack and Teams)

Channels (open and private), DMs and group DMs, threads, reactions, edit and delete, mentions, markdown and code, attachments and link previews, voice clips, read state, typing and presence, search with operators, notification settings, follow and mute, reminders, workflows, webhooks, invites, roles, admin console, moderation, audit log, deep links, auto-update, themes. (#001, #002, #005 to #007, #010, #012, #014, #019 to #022, #049, #050, #055 to #057, #066, #069, #028, #029, #031, #032, #038, #148, #150, #163, #164, #168.)

### 5.2 Best-in-class or near it (relative to Slack, Teams and Grok Bot, INF from the sibling inventories)

| Capability | Buzz's edge | Records |
|---|---|---|
| Owner supervision of agent work (verb-object-outcome cards, never go dark, raw rail) | Best design text and implementation in the set; Grok Bot narrates well but has approvals, Buzz has transparency without gating | #112, #093, #094, #174 |
| Mid-turn steering with explicit policies | Four explicit policies and framing; rare | #089 |
| Mention integrity in mixed rooms (exact identity binding, address lock, mention-to-invite) | Best | #009, #104, #115, #041 |
| Agent room etiquette contract plus a benchmark that grades it | Unique | #091, #116 |
| Agent identity, owner attestation, cascading revocation | Best accountability model | #081, #082, #149, #114 |
| Server-computed timeline windows with exact cursors | Best | #137 |
| Read-your-writes discipline | Best | #138 |
| Multi-device preference sync (LWW registers) | Best | #140 |
| Formal multi-tenant isolation evidence | Unmatched; Slack and Teams publish nothing comparable | #151 |
| Moderation loop (report, queue, act, tombstone, notify) | Cleanest human-in-the-loop design | #148, #149 |
| In-page agent chat with injection hygiene and pre-send preview | Best | #128, #180 |
| Honest-state UX | Most consistent discipline | #178, #113 |
| Content-free push wake | Best fit for zero-retention constraints | #058 |
| Canvas write safety (compare-and-set, history, restore) | Better than Slack for conflict handling; worse for editor richness | #120, #119 |

### 5.3 Where peers are clearly better

- **Slack**: richer canvases and Lists, huddles with video and screen share, app directory and Slack Connect shared channels, AI search and recaps, polish across a mature client (INF).
- **Teams**: org and team hierarchy, guest and external collaboration, meetings with recording and AI recap, calendar, Purview compliance and eDiscovery, residency (INF; DOC teams inventory).
- **Grok Bot**: a persistent agent computer with browser and desktop, an approval ladder (always allow, needs approval, blocked) with auto-review rules, per-person connector accounts, routines, cost and limits (DOC `grokbot.agent-product.deep.md`). Buzz has none of this (#118, #096).

### 5.4 Differentiators (unique or uncommon, worth a deliberate decision)

Owner-chain agent identity (#081, #082); agent address policy and add policy (#088, #030); steering (#089); mention-to-invite (#115); address lock (#104); verb-object-outcome cards (#112); owner-reviewed proposals (#097); setup nudge card (#098); agent cards and snapshots (#084, #085); relay-computed windows (#137); LWW preference sync (#140); formal tenant isolation (#151); information-flow design (#154); benchmark for room behaviour (#116); sender-side link previews (#016); video review comments (#015); relay-gated shared compute (#117); agents in huddles (#045).

### 5.5 Nice-to-haves seen in Buzz

Custom emoji union palette (#008), image markup (#013), GIF search (#017), agent cards (#085), emoji bursts and poof animations (#179), editor-theme palette (#168), community themes (#040), mouse back and forward (#162), per-community themes.

## 6. Weaknesses, limits and dead ends

1. **Agent autonomy is local and ungoverned**: auto-approved tool permissions, no browser or computer use, no hosted PC, no spend cap, no sandbox beyond a Pod (#096, #118, #103). Keys and processes live on the user's machine; Windows needs Git Bash (#171).
2. **Dead or partial protocol surfaces**: typed job kinds nobody emits (#062, #105); approval gates that fail the run (#068); `send_dm` and `set_channel_topic` workflow actions unimplemented; scoped API tokens unused (#080); scheduled send, pins and saved items defined but not wired (#024, #025); NIP-AR with no client (#121); persona packs that cannot be imported (#074); forum, workflows, projects and pulse still behind preview flags although VISION says all seven surfaces ship.
3. **Doc and code drift** (the reason to read code): ARCHITECTURE says there is no rate limiter (it exists, #156) and that presence is node-local (it publishes cross-node, #021); VISION_PROJECTS describes push-allowed lists and a require-approval count that the git policy does not implement (#123).
4. **Two unread systems** that do not sync (#019), with the newer server-derived one unused by any in-repo client; read state, preferences and theme all ride kind 30078 blobs, a pragmatic but fragile reuse.
5. **Tamper-evident audit without verification** (#150) and **media not gated by channel ACL** (#158) are security gaps a compliance buyer would find quickly.
6. **No browser chat client** (#167). Everything that is chat requires installing an app.
7. **Weak org model**: no teams, groups, shared channels or real guests (#033, #034); roles are a closed enum.
8. **Owner-only agent transparency** means teammates cannot see what a shared agent is doing; with 50,000 targeted agents that matters (#093).
9. **Retention and compliance** are minimal (#159, #161).
10. **Offline**: desktop is online-first with a paint cache; mobile offline-first is a spec, not a verified behaviour (#141, #142).
11. **Scale claims are targets**, not measurements, apart from the Redis bus harness (#146, #135).
12. **Complexity cost**: 38 crates, 60 migrations, 19 NIP drafts, hundreds of BUZZ_* environment variables, and agents that must be reachable through Nostr semantics. The repo itself documents repeated incident-driven fixes (welcome kickoff silent failures, subscription gaps).

## 7. What Verevon's Space lacks that Buzz demonstrates (feature-level gap map)

From the Verevon baselines (DOC): every post is a model turn, no human-only messages, no DMs, no edit, delete, reactions, attachments, drafts or search, 6-second polling, no notifications from Space events, owner-only replies into threads, no personal agents, no agent work visibility beyond status, approvals recorded but not resumed, no guests, no calls. Buzz has working designs for most of the chat gaps and for agent supervision:

| Verevon gap | Buzz reference | Records |
|---|---|---|
| Human-only room messages | Stream channels where agents are ordinary members | #001, #081 |
| Thread side panel and non-owner replies | Threads with relay-computed counts, split or focus drawer | #002, #003 |
| DMs and group DMs | Participant-hash idempotent DM channels, hide and resurface | #005 |
| Edit and delete | Self and owner-of-agent authority | #006 |
| Reactions as approval and feedback | Kind 7 with quick row | #007 |
| Person mentions with notifications | Exact-identity binding plus server mention table | #009, #054 |
| Attachments and previews | Content-addressed media, sender-side previews | #012, #016 |
| Drafts | Revisioned drafts with authoritative-empty marker | #018 |
| Push realtime | Bounded WS loops and fan-out | #133, #134 |
| Search | Tenant FTS with per-hit re-authorization; operators | #049, #050 |
| Notifications and inbox | Home inbox with Needs-action lane, slots, thread rules, reminders | #054 to #057 |
| Unread across rooms | Overflow button and community rail | #061 |
| Agent work visibility | Verb-object-outcome cards, working signal, activity strip | #112, #094, #174 |
| Personal agents in shared rooms | Owner attestation, add policy, respond-to, address lock | #081, #030, #088, #104 |
| Mid-run input to an agent | Steering policies | #089 |
| Human-in-the-loop that works | Owner-reviewed proposals | #097 |
| Shared documents | Canvas with compare-and-set and history | #119, #120 |
| Moderation of rooms | Report, queue, act, tombstone, notify | #148 |
| Per-user room organization | Stars, sections, sort, sync | #173, #140 |
| Calls | Audio huddles with agent voice (later) | #043, #045 |

Where Buzz does **not** help Verevon: agent PC and browser, approvals, cost control, Teams-style teams and guests, enterprise compliance (#118, #096, #034, #161). Verevon is ahead in sandbox leases, Quarry browser grants, approval gating (although a granted approval does not yet resume the run), and signed, effect-bound Control decisions; its ZDR propagation and deletion receipts exist but are partial per the baselines.

## 8. Recommendations for Verevon

### 8.1 Copy (low adaptation cost, high value)

1. **Server-computed room timeline** with top-level paging, composite cursors, thread summaries and exact has-more (#137, #002). Required once rooms hold human chat; replaces the per-post N+1 transcript fetch.
2. **One rule for consistency**: reads that gate a decision or write hit the authority plane; display reads may use projections; a one-way strong flag (#138).
3. **Mention integrity**: exact-identity binding with a server-side mention record that drives notifications; address lock; mention-to-invite and non-member consent (#009, #104, #115, #041).
4. **Steering policies** (steer, queue, supersede, owner-only interrupt) layered on the existing mid-stream queue endpoint (#089).
5. **Agent etiquette as a platform prompt layer plus a regression benchmark** for room behaviour (#091, #116). Add cases for the Verevon-specific contracts (no cross-room leakage, no wake on narrative mentions, approval pause and resume).
6. **Supervision UX**: verb-object-outcome cards, composer activity strip, honest-state discipline, never-go-dark rule (#112, #174, #178, #113). Decide visibility tiers deliberately (owner only, room, org).
7. **Owner-reviewed proposals** for structural agent changes, delivered as a typed card with confirm (#097), and a signer-checked setup nudge (#098). This is also the cleanest model for the broken approval resume path: persist the continuation first, then build the card.
8. **In-page agent chat** with selected context, quoted untrusted metadata, a fixed data-not-instructions notice and a byte-identical pre-send preview (#128, #180).
9. **Compare-and-set on a head revision** for any shared artifact, with the pre-write, post-write and unverified-notice states (#120); typed room records governed by room access (#121).
10. **Moderation loop** for human and agent content, including cascading removal from an owner to the owner's agents (#148, #149).
11. **Preference sync** as LWW registers per user and Space (#140), and a server-derived unread model (#019, #054).
12. **Content-free push wake** with client-side fetch for ZDR orgs (#058), and the desktop-active suppression rules (#059).
13. **Assurance artifacts**: a surface-by-surface conformance table with trace replay, an inventory-completeness test for egress guards, and a single shared reserved-key list (#151, #155, #110).

### 8.2 Adapt (needs a Verevon-specific design)

- **Agent identity and ownership**: keep Control subjects; add a visible managed-by label and an owner chain that revokes agents with the owner (#081, #082, #149).
- **Memory scope**: Buzz scopes memory per agent and owner pair; Verevon should add a room scope and adopt audience labels so private-room memory cannot reach a wider audience (#099, #154).
- **Information-flow broker**: the audience-bound agent instance with an egress check against reader sets is the right shape for Verevon personal agents invited into shared rooms; Control decisions already carry the audience (#154).
- **Add policy and respond-to** as binding fields (#030, #088); the baselines say `knowledge_scope`, `default_thread_policy` and `audit_visibility` are still unstored.
- **Templates** server-side per org (Buzz keeps them local and unshareable) (#035).
- **Agent voice** later, as an attributed peer with a human-floor policy (#045).
- **Guests**: design scoped, expiring, read-only room access properly; do not copy Buzz's label (#033).
- **Project or case Spaces that spawn rooms** (branch-as-channel idea) (#127).
- **Pod-level coordination** (gossip advisory, fenced store arbiter) for sandbox leases (#144).

### 8.3 Avoid

- **Nostr as the wire**: key custody onboarding, kind sprawl, filters that cannot express the timeline, no E2EE for chat, and two systems for unread (#019, #137, #143).
- **Agents on user machines** (#083, #171) and **auto-approved tool permissions** (#096).
- **Dead protocol surfaces**: job kinds, approval gates that fail the run, dormant tokens (#062, #068, #080). Ship the backend only with its consumer.
- **Client-side merge or other privileged effects on the user's machine** (#126).
- **Tamper-evident audit without verification, and unprotected media reads** (#150, #158).
- **Preview-gating the core product surfaces** while the vision claims they ship (#076).
- **App-only chat** (#167): Verevon's browser-first advantage is real.

### 8.4 Suggested order of adoption (INF)

Phase A (unblocks chat): human posts without model turns, timeline windows, DMs, edit and delete, reactions, mention records, push realtime, notifications and inbox, search. Phase B (agents in the room): steering, address lock, activity strip and cards, etiquette plus benchmark, proposals and nudge cards, add and respond-to policies. Phase C (artifacts and governance): canvas with compare-and-set, typed room records, moderation, retention, media ACL, audit with verification. Phase D (breadth): voice, guests, templates, preference sync, mobile.

## 9. Corrections to the scout inventory

| Scout statement | Verified result |
|---|---|
| Multi-agent coordination uses job kinds 43001 to 43006 | Kinds exist and are read by one feed query, but `buzz-acp` publishes ordinary messages; coordination is chat-only (#105, #062) |
| Human approval of tool calls via buzz-agent permission broker | The broker asks, but the harness answers by auto-selecting allow-once; there is no human gate (#096) |
| Persona packs as a working agent definition path | Spec plus CLI validate only; desktop imports snapshots, not packs (#074) |
| Workflow approvals partly wired | Confirmed: a run that reaches an approval gate is marked failed; `send_dm` and `set_channel_topic` are NotImplemented (#068, #066) |
| Rate limiting not implemented (per ARCHITECTURE) | Implemented: Redis fixed-window limiter with human and agent tiers (#156) |
| Presence fan-out local only (per ARCHITECTURE) | Code publishes presence to the community global Redis topic (#021, #135) |
| Local archive as offline cache | It is a local-save archive; the real cache is a head-page cache capped at 32 channels (#141) |
| Web client | Repository browser and invite page only; no chat (#167) |
| Branch protection with push-allowed lists and approval counts (VISION_PROJECTS) | Code has role-based push, no-force-push, no-delete and require-patch only (#123) |
| Guest tokens (VISION) | Guest is a role label; scoped token store exists but no auth path uses it (#033, #080) |
| Server-derived unread (/buzz/v1) as the newer read-state path | Flag-gated on the relay and not called by any in-repo client; the live in-repo path is NIP-RS plus native observed-unread (#019) |
| Audit log "designed for SOX and eDiscovery" | Chain verification is not called anywhere outside tests; no export (#150) |
| Agent usage metrics shown to owners | Data path exists; the UI hook is referenced only in comments in the OSS tree (#111, medium confidence) |

## 10. Open questions

1. Does the internal (protected) desktop build add usage charts, Bestie and Builderlab hosting that the OSS tree only hints at (`protectedFeatures/`, `builderlab.rs`, `bestie_assignment.rs`)? I could not see them.
2. Which client consumes `/buzz/v1` (server-derived unread)? The relay serves it only when `BUZZ_V1_ENABLED=true`; no in-repo desktop, mobile, web or CLI code calls it, and the doc says a separate client, buzz-app, uses v1 only (SRC `crates/buzz-relay/src/config.rs:1365`; DOC `docs/buzz-v1-read-state.md:24`). The relationship between buzz-app and the in-repo desktop is not documented here.
3. How much of remote agents (Kubernetes provider), mesh compute and mobile huddles is production-proven versus spec-complete (#103, #117, #047)?
4. What does an operator do about audit verification and external anchoring? No job in the repo runs it (#150).
5. Is the in-app terminal gated in the OSS build, and is it meant for general users (#130)?
6. What is the real behaviour of the mobile offline-first contract, given the spec says it is partly unimplemented (#142)?
7. How are the 10,000 humans and 50,000 agents targets validated end to end (#146)?
8. Which of the older ARCHITECTURE.md limits are still true? Rate limiting and presence were stale; approvals and stubbed workflow actions were current.

## 11. Evidence index (starting points)

- Product intent: `README.md`, `VISION.md`, `VISION_ACTIVITY.md`, `VISION_AGENT.md`, `VISION_MODERATION.md`, `VISION_MOBILE.md`, `VISION_PROJECTS.md`, `VISION_MESH.md`, `VISION_SOVEREIGN.md`, `VISION_REMOTE_AGENTS.md`.
- Architecture and protocol: `ARCHITECTURE.md`, `NOSTR.md`, `docs/nips/*`, `docs/bridge-channel-window.md`, `docs/buzz-v1-read-state.md`, `PLANS/REPLICA_FULL_READ_ROUTING_DESIGN.md`, `docs/multi-tenant-conformance.md`, `docs/spec/*`.
- Data model: `schema/tables/public/*.sql`, `crates/buzz-core/src/kind.rs`, `crates/buzz-core/src/git_perms.rs`.
- Relay: `crates/buzz-relay/src/handlers/ingest.rs`, `handlers/event.rs`, `handlers/auth.rs`, `handlers/channel_authz.rs`, `handlers/moderation_authz.rs`, `api/bridge.rs`, `api/git/*`, `admission.rs`, `rejection.rs`.
- Agents: `crates/buzz-acp/src/{config,pool,queue,filter,acp,usage,setup_mode,base_prompt.md}`, `crates/buzz-agent/src/{handoff,config,permission}.rs`, `crates/buzz-dev-mcp`, `crates/buzz-cli`, `desktop/src/features/agents/`, `desktop/src/features/messages/ui/Composer*`, `docs/agent-availability.md`, `docs/remote-mention-routing.md`.
- Evaluation: `benchmarks/buzz-dataset`, `benchmarks/harbor-buzz-orchestra`.
- Clients: `desktop/src/` (features: messages, channels, sidebar, home, projects, agents, huddle, moderation, workflows, pulse), `desktop/src-tauri/src/` (deep_link, native_websocket*, channel_head_cache, egress_guard, huddle/), `mobile/lib/features/`, `web/src/`.
- Governance: `crates/buzz-audit`, `crates/buzz-deletion`, `crates/ifc-core`, `docs/practical-information-flow-for-buzz-agents.md`, `SECURITY.md`.
