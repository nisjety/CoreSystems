# Buzz inventory (t1, scout pass)

Read-only inventory of the Buzz open-source repo for the later deep-dive agent.
Repo: C:/dev/buzz, HEAD 799cfd705 ("Start the ACP idle interval when channel turns complete (#8149)").
Paths below are relative to C:/dev/buzz unless stated. Nothing in the repo was modified.

Provenance tags:
- SRC = read in source or manifests (pointer given)
- DOC = repo documentation (README, VISION*, NIP drafts, ARCHITECTURE, AGENTS, etc.). DOC states intent; code is ground truth.
- SRC+DOC = both agree
- INF = inference, with the basis stated and a confidence (conf high/med/low)
- REA = not used (open-source repo, read directly)

Counts come from greps and wc during this pass and include test files unless stated.

---

## 1. Identity

- Product: Buzz, "A workspace where humans and agents build together, on a relay you own." Apache-2.0, built by Block, Inc. [DOC README.md, LICENSE]
- Versions: desktop 0.5.28 (desktop/package.json; desktop/src-tauri/tauri.conf.json productName "Buzz", identifier xyz.block.buzz.app) [SRC]. Rust workspace 0.1.0 (Cargo.toml [workspace.package]) [SRC]. Mobile 0.0.0+1 (mobile/pubspec.yaml) [SRC]. admin-web 0.1.0, web 0.1.0 [SRC]. Latest release notes: CHANGELOG.md "v0.5.28" [SRC].
- Legacy naming: Cargo repository = github.com/block/sprout; dev skill folder "sprout-cli" (.agents, .claude, .codex, .goose /skills); AGENTS.md ecosystem repos "squareup/sprout-oss", "sprout-backend-blox" [SRC]. Treat "Sprout" as the former name.
- Ecosystem (5 repos): OSS block/buzz; internal releases squareup/buzz-releases; relay image squareup/sprout-oss; Terraform/ArgoCD stack; sprout-backend-blox (Blox compute provider for remote agents) [DOC AGENTS.md "Ecosystem"].

Stack:
- Relay and backend: Rust 1.88+ (Cargo rust-version 1.88.0); Axum 0.8 and Tokio; sqlx 0.9 on Postgres (rustls); Redis (redis 1.0, deadpool-redis); S3/MinIO (crate "s3"); kube 2.0 (Kubernetes backend); nostr 0.44 (features nip44, nip98); iroh 1.0.0-rc.0 (QUIC mesh); OpenTelemetry OTLP and Prometheus metrics; LaunchDarkly SDK behind buzz-feature-flags; rmcp 1.1 (MCP SDK); reqwest 0.13 (webhooks); jsonwebtoken with aws_lc_rs (NIP-FI) [SRC Cargo.toml; crates/*/Cargo.toml]
- Desktop: Tauri 2 shell (desktop/src-tauri, excluded from the workspace) with a React frontend (Vite; TanStack Router, Query, Virtual; Tiptap; react-markdown; Radix UI; Tailwind; sonner; Playwright E2E). React 19 per VISION.md [SRC desktop/package.json, desktop/src-tauri/Cargo.toml; DOC VISION.md "Architecture"]
- Mobile: Flutter (Dart SDK ^3.11.4) with hooks_riverpod, nostr, WebSocket, secure storage, biometrics, QR scanning [SRC mobile/pubspec.yaml]
- Web client: Vite + React, TanStack Router and Query, isomorphic-git with lightning-fs (in-browser git), react-markdown [SRC web/package.json]
- Admin web: Vite + React SPA with Vitest and Playwright [SRC admin-web/package.json]
- Agent harness: buzz-acp (ACP harness bridging Buzz events to agent runtimes), buzz-agent (ACP agent calling LLMs with MCP tools), buzz-dev-mcp (MCP shell and file tools), buzz-cli (agent-first JSON CLI) [SRC crates/]
- Local voice: ONNX Runtime (ort), sherpa-onnx, SentencePiece and tokenizers, symphonia audio decode; Pocket TTS "english_2026-04" bundle [SRC crates/buzz-voice/Cargo.toml; crates/buzz-voice/src/pocket.rs]

Dependency names by capability (strong feature signals):
- Messaging and protocol: nostr (nip44, nip98), postcard, serde_json, serde_yaml (workflows), evalexpr (conditions), cron 0.16 (schedules), pulldown-cmark (markdown), infer, blurhash, imagesize, mp4, image, ulid [SRC]
- Realtime and infra: axum ws, tokio-tungstenite (client), redis, deadpool-redis, dashmap, moka (caches), tower-http (compression, CORS), async-compression, metrics + metrics-exporter-prometheus, opentelemetry-otlp, hmac, subtle, zeroize [SRC]
- Storage: sqlx (postgres), s3, tempfile, bytes, quick-xml, rusqlite (desktop local archive) [SRC]
- Auth and crypto: ed25519-dalek, aes-gcm (push gateway), appattest + minicbor (Apple App Attest), rustls, sha2 [SRC]
- Mesh: iroh; mesh-llm-sdk, mesh-llm-host-runtime, mesh-llm-client, mesh-llm-node, mesh-llm-system, mesh-llm-events (desktop) [SRC desktop/src-tauri/Cargo.toml]
- Desktop native media: opus, neteq (huddle jitter buffer, huddle/jitter.rs), rodio (audio out), rubato (resampling), earshot (VAD, huddle/stt.rs), sherpa-onnx (STT and TTS), png, image, arboard (clipboard), zip, tar, flate2, bzip2 (runtime and model archives) [SRC]
- Desktop shell: Tauri plugins deep-link, opener, single-instance, window-state, dialog, updater, process, global-shortcut, notification; portable-pty (terminal); strip-ansi-escapes; webbrowser (agent OAuth in buzz-agent) [SRC]
- Frontend (npm, desktop): @tanstack/react-router, react-query, react-virtual; @tiptap/* (editor, markdown); react-markdown, remark-gfm, remark-breaks; shiki (code highlight); emoji-mart and @emoji-mart/data; @dnd-kit/* (sidebar and community sorting); react-diff-view and diff (PR diffs); motion; qrcode (device pairing); jdenticon (identicons); @mediapipe/tasks-vision (animated avatar capture); embla-carousel-react; react-day-picker; linkifyjs; sonner; virtua; upng-js; zod; yaml; @radix-ui/* (dialog, popover, tabs, switch, context-menu, tooltip); @fontsource Inter and JetBrains Mono; nostr-tools and @noble/hashes (dev); Playwright [SRC desktop/package.json]
- Mobile (pub): hooks_riverpod; nostr; pointycastle; web_socket_channel; flutter_secure_storage; local_auth (biometrics); mobile_scanner, camera, image_picker, photo_manager; video_player, just_audio, record (voice and video media); app_links (deep links); app_badge_plus; share_plus; open_filex; gpt_markdown and highlight; google_mlkit_selfie_segmentation (animated avatar capture); connectivity_plus; shared_preferences; scrollable_positioned_list; flutter_svg; url_launcher [SRC mobile/pubspec.yaml]
- Web (npm): isomorphic-git, @isomorphic-git/lightning-fs, buffer, nostr-tools, @tailwindcss/typography, sonner [SRC web/package.json]

---

## 2. Directory and module map

Top level (C:/dev/buzz):
- crates/: 38 Rust crate directories; workspace members listed in Cargo.toml; desktop/src-tauri excluded [SRC]
- desktop/: Tauri 2 + React desktop app. src/ (frontend), src-tauri/ (Rust backend), src-tauri/crates/buzz-terminal (PTY terminal engine) [SRC]
- mobile/: Flutter client. lib/features: activity, age_gate, channels, forum, home, invites, pairing, profile, pulse, search, settings. lib/shared: auth, community, crypto, custom_emoji, deeplink, emoji, huddle, identity_names, mentions. Also HUDDLES.md, CHANGELOG.md, scripts/, shaders/ [SRC]
- web/: Vite/React browser client. src/features: invite, repos. Also an app/ router tree [SRC]
- admin-web/: Vite/React admin console SPA (App.tsx, api.ts, useResource.ts, types.ts) [SRC]
- docs/: 31 top-level docs (MCP_DRIVEN_HOOKS, agent-availability, agent-profile-identity, buzz-push, buzz-entity-links, gif-search, git-on-object-storage, mention-editor, mobile-push-suppression, multi-tenant-relay, operator-community-deletion, remote-agents, storage-accounting, usage-metrics, and others), plus nips/ (19 NIP drafts), spec/ (TLA+ MultiTenantRelay, GitOnObjectStore; Tamarin MultiTenantAuth), formal/ (nip-pl, nip-rs-unread, STATEFUL_GATEWAY.md), admin/, assets/ [SRC]
- migrations/: 60 Postgres migrations, 0001 to 0060 (last 0060_push_single_application.sql) [SRC]
- schema/: schema.sql plus tables/, functions/, partitions/, triggers/, types/, seeds/, community_write_fence.sql [SRC]
- deploy/: charts/buzz (Helm), charts/buzz-push-gateway, compose/ (compose.yml, compose.dev.yml, compose.caddy.yml, Caddyfile, run.sh), local/ (build-and-deploy.sh, quickstart-ha-values.yaml) [SRC]
- examples/: countdown-bot (Rust NIP-42 example bot), meadow-core (three-agent persona pack) [SRC]
- benchmarks/: buzz-dataset; harbor-buzz-orchestra (Python adapter that runs a Buzz team inside Harbor; manifests, personas, testbed) [SRC]
- scripts/: about 70 CI, release, dev and maintenance scripts (sh, py, mjs, jq, sql), incl. desktop_release.py, build-sprig.sh, bundle-sidecars.sh, post-screenshots.sh, cutover/, maintenance/ [SRC]
- perf/: RELAY_BUS_SCALING.md, relay_bus_scaling.py (+ test) [SRC]
- PLANS/: REPLICA_FULL_READ_ROUTING_DESIGN.md (status: active) [SRC]
- patches/: isomorphic-git.patch, virtua@0.49.3.patch [SRC]
- bin/: Hermit toolchain pins (actionlint, biome, cargo-deny, cmake, flutter 3.41.7, just, lefthook, node 24.15.0, pgschema, pnpm 11.4.0, rustup, uv) [SRC]
- .github/: 27 workflows, ci/, scripts/, issue and PR templates [SRC]
- .agents/, .claude/, .codex/, .goose/ skills/: developer skills only (desktop-screenshot, sprout-cli) [SRC, names only]
- Root docs: README.md, VISION.md and 8 VISION_*.md (ACTIVITY, AGENT, MESH, MOBILE, MODERATION, PROJECTS, REMOTE_AGENTS, SOVEREIGN), ARCHITECTURE.md, AGENTS.md, CONTEXT.md, NOSTR.md, TESTING.md, RELEASING.md, SECURITY.md, CONTRIBUTING.md, GOVERNANCE.md, CODE_OF_CONDUCT.md, CHANGELOG.md (445 KB) [SRC]
- Root config: Justfile (about 107 recipes), Cargo.toml, deny.toml, lefthook.yml, biome.json, renovate.json, preview-features.json, pnpm-workspace.yaml, docker-compose*.yml, Dockerfile, Dockerfile.push-gateway, Dockerfile.sprig, prometheus.yml, ct.yaml [SRC]

Crates (description from Cargo.toml unless noted; file counts are .rs files):
- buzz-relay (118): WebSocket relay server, NIP-01, REST, huddle audio, git, media, operator APIs
- buzz-core (27): core types, event verification, filters, kind registry (kind.rs, 122 kind consts)
- buzz-db (67): Postgres event store and data access
- buzz-auth (23): NIP-42 and NIP-98 auth, rate-limit traits (rate_limit.rs)
- buzz-pubsub (11): Redis pub/sub fan-out, presence, typing, Redis rate limiter, NIP-98 and NIP-FI command replay guards
- buzz-search (4): community-scoped Postgres full-text search
- buzz-audit (6): per-community hash-chain audit log
- buzz-workflow (5): YAML-as-code workflow engine (schema.rs, executor.rs, action_sink.rs)
- buzz-media (13): Blossom and S3 media storage, validation, thumbnails
- buzz-deletion (1): durable whole-community deletion engine
- buzz-feature-flags (7): typed boolean and integer flags; LaunchDarkly evaluator (launchdarkly.rs)
- buzz-conformance (5): trace schema and replay checker for MultiTenantRelay.tla
- buzz-push-gateway (16): blind, capability-gated NIP-PL gateway for APNs (apns.rs, app_attest.rs, grant.rs)
- buzz-relay-mesh (9): inter-relay QUIC mesh over iroh (endpoint, gossip, membership, registry, wire)
- buzz-mesh-smoke (7): mesh-llm smoke harnesses (no library)
- buzz-acp (36): ACP harness (acp/, pool/, relay/, queue.rs, observer.rs, engram_fetch.rs, usage.rs, isolated_execution.rs, recovery_wake.rs, session_model_*.md)
- buzz-agent (36): minimal ACP agent (llm.rs with Anthropic, OpenAI, Databricks, OpenRouter; mcp.rs; permission.rs; handoff.rs; auth.rs OAuth)
- buzz-dev-mcp (12): MCP server with shell, read_file, view_image, str_replace, todo (no Cargo description)
- buzz-cli (33): agent-first CLI, JSON in and out
- buzz-sdk (13): typed Nostr event builders
- buzz-persona (9): parser and loader for .persona.md and plugin.json packs
- buzz-voice (6): local voice primitives (pocket.rs, pocket_april.rs, pocket_models.rs, imported.rs)
- buzz-backend-kubernetes (15): Kubernetes provider for remote agents (client, pod, reconcile, gc, cluster, observe)
- buzz-admin (4): operator CLI for relay administration and NIP-43 membership
- buzz-pair-relay (4): ephemeral sidecar relay for NIP-AB device pairing
- buzz-pairing-cli (1): NIP-AB pairing interop CLI
- buzz-ws-client (4): WebSocket client (no Cargo description)
- buzz-test-client (25): NIP-01 test client and E2E suite
- buzz-datastore-tracing (2): privacy-preserving datastore tracing macros
- ifc-core (1): generic reader-set information-flow-control primitives
- git-sign-nostr (2): NIP-GS git commit and tag signing program
- git-credential-nostr (3): NIP-98 git credential helper
- sprig (1): all-in-one ACP harness, agent and dev MCP (deploy-anywhere binary)
- examples/countdown-bot: example NIP-42 bot

desktop/src (frontend; file counts include tests):
- features/: agents 245, messages 244, projects 218, channels 145, onboarding 56, profile 63, sidebar 49, settings 44, workflows 48, home 34, communities 30, huddle 30, admin-console 19, forum 15, reminders 12, moderation 11, notifications 11, pulse 11, terminal 11, search 10, community-members 9, local-archive 8, mesh-compute 7, user-status 6, custom-emoji 5, presence 4, agent-memory 3, gifs 3, channel-templates 2, identity-archive 1, chat 1
- shared/: api 93, ui 133, lib 64, styles 18, layout 12, hooks 13, theme 10, features 7 (manifest and gate), context 4, constants (kinds.ts)
- app/: routes (TanStack file routes), shell and overlays, keyboard shortcuts, tray, deep links, desktop notifications
- protectedFeatures/: internal.ts and internalUi.ts (internal build only), public.ts and publicUi.ts (OSS no-ops)
- testing/: e2eBridge*.ts (test bridge)

desktop/src-tauri/src (Rust backend):
- lib.rs (invoke handler at line 529); commands/ (97 files with #[tauri::command]; 377 unique command names)
- managed_agents/: agent lifecycle, personas, teams, nest, bestie, discovery, reconcile, readiness, snapshots, retention, relay_mesh, custom harnesses, env vars
- huddle/: audio pipeline, jitter, playout, STT and TTS, agent voice, state, reconnect
- archive/: local SQLite archive, sync, retention, agent usage and metrics
- mesh_llm/: mesh compute node control, catalog, usage
- migration/: legacy-to-current data migration (backfill, fold, team membership)
- relay/, native_relay_client*.rs, native_websocket*.rs: relay connection
- terminal_runtime/, terminal_transport.rs: PTY terminal runtime
- deep_link.rs, tray_menu.rs, app_menu.rs, prevent_sleep.rs, egress_guard.rs, key_backup.rs, hpke_key_backup.rs, identity_storage.rs, secret_store.rs, reset.rs, webkit_rendering/, linux_media.rs, macos_notifications.rs, mouse_nav.rs, ptt_shortcut.rs, observed_unread.rs, unread_catch_up.rs, templates/, persona_catalog.rs, team_catalog.rs

---

## 3. Feature candidates (by taxonomy)

Every bullet names its capability and gives a pointer and provenance tag. Paths are repo-relative.

### 3.1 Messaging core
- Stream channels (group chat, kind 9, NIP-29 h tag): crates/buzz-core/src/kind.rs (KIND_STREAM_MESSAGE); NOSTR.md "Group chat"; VISION.md "Surfaces" [SRC+DOC]
- Forum channels (posts 45001, comments 45003, votes 45002; channel type "forum"): crates/buzz-core/src/kind.rs; crates/buzz-cli/src/lib.rs (ChannelType::Forum); desktop/src/features/forum/; preview-features.json id "forum" [SRC]
- Direct messages, 1:1 and group up to 9: DM open 41010, add-member 41011, hide 41012, created 41001 (kind.rs); DM visibility snapshot NIP-DV kind 30622 (docs/nips/NIP-DV.md); NIP-17 gift wrap kind 1059 accepted and stored community-wide, not indexed (NOSTR.md); commands open_dm, hide_dm; CLI "dms" group [SRC+DOC]
- Threads and replies: NIP-10 replies create thread_metadata (NOSTR.md); thread windows NIP-CW with kinds 39005, 39006, 39007 (docs/nips/NIP-CW.md; migrations/0049_thread_window_index.sql); desktop useThreadReplies.ts, useIndependentThreadPanel.ts (features/messages/); command get_thread_replies; CLI "messages thread" [SRC+DOC]
- Edit and delete: edit kind 40003; deletion kind 5 (NIP-09, self-authored); commands edit_message, delete_message; CLI "messages edit|delete" [SRC]
- Reactions: kind 7 (NIP-25); add_reaction, remove_reaction, get_note_reactions; reaction payload migration 0028 (long_reaction_payloads); CLI "reactions add|remove|get" [SRC]
- Mentions: mention-editor contract (docs/mention-editor.md); HOME_MENTION_EVENT_KINDS in desktop/src/shared/constants/kinds.ts; remote mention routing (docs/remote-mention-routing.md) [SRC+DOC]
- Rich text and markdown: react-markdown with remark-gfm and remark-breaks; Tiptap with tiptap-markdown; shiki code highlighting; code-block copy (desktop/src/shared/lib/codeBlockClipboard.ts); server-side pulldown-cmark (crates/buzz-relay/Cargo.toml) [SRC]
- Diffs in messages: kind 40008 diff messages (CLI "messages senddiff"); react-diff-view in desktop [SRC]
- Attachments and media: Blossom PUT /media/upload and GET /media/{sha256}.{ext} (crates/buzz-relay/src/router.rs; crates/buzz-relay/src/api/media.rs); thumbnails and blurhash (crates/buzz-media); composer attachments (desktop/src/features/messages/ui/ComposerAttachments.tsx; features/messages/lib/useMediaUpload.ts); pick_and_upload_media command [SRC+DOC]
- Voice notes: audio attachments named voice-note-*, max 300 seconds (desktop/src/features/messages/lib/audioAttachment.ts, VOICE_NOTE_MAX_DURATION_SECONDS); mobile recording with record and just_audio (mobile/pubspec.yaml) [SRC]
- Link previews: fetch_link_preview_metadata, release_link_preview_metadata, cancel_link_preview_metadata commands; link_preview_tags.rs; screenshot directory env BUZZ_LINK_PREVIEW_SCREENSHOTS_DIR [SRC]
- Entity links: buzz://message?channel=&id=&thread= (desktop/src-tauri/src/deep_link.rs); repo, project, pr, issue hosts allowlisted (deep_link.rs ENTITY_LINK_HOSTS, line 369); design in docs/buzz-entity-links.md [SRC+DOC]
- GIF search and share: relay-proxied KLIPY search and share (POST /gifs/search, POST /gifs/share; crates/buzz-relay/src/api/gifs.rs); desktop features/gifs; CLI "gifs search|share"; env BUZZ_KLIPY_API_KEY and BUZZ_RATE_LIMIT_GIF_SEARCHES_PER_MIN; docs/gif-search.md [SRC+DOC]
- Custom emoji: emoji sets (kind 30030) and emoji list (kind 10030); desktop features/custom-emoji (ui/CustomEmojiSettingsCard.tsx); CLI "emoji list|set|rm|export|import"; mobile shared/custom_emoji [SRC]
- Emoji autocomplete and picker: desktop features/messages/lib/useEmojiAutocomplete.ts; emoji-mart; mobile shared/emoji [SRC]
- Drafts: local draft store with v1 to v2 migration (desktop/src/features/messages/lib/useDrafts.ts); home drafts (desktop/src/features/home/useHomeDrafts.ts); VISION_MOBILE requires drafts survive restarts [SRC+DOC]
- Scheduled send: kind 40006 KIND_STREAM_MESSAGE_SCHEDULED defined (crates/buzz-core/src/kind.rs); no composer UI or CLI verb found [SRC; INF: defined but not wired, conf med]
- Pins: kind 40004 pinned messages; pin list kind 10001; desktop pin hooks (about 19 files reference pinning in features/messages, channels, home) [SRC]
- Saved messages (bookmarks): kind 40005 and bookmark list kind 10003 defined in kind.rs; no user-facing saved-messages UI found [SRC; INF: defined only, conf med]
- Message forwarding: not found; keyword hits are only forwardRef and Tab-order code [SRC; INF, conf med]
- Polls: not found as a message type. Forum votes (kind 45002) and CLI "messages vote" up or down are the only voting paths. VISION.md lists native /poll as planned [SRC+DOC]
- Read state: per-user read state (kind 30078, d tag) and NIP-RS cross-device sync (docs/nips/NIP-RS.md; migrations 0056-0058 personal_read_state); desktop useHomeInboxReadState.ts; mark-as-read shortcuts (desktop/src/app/useMarkAsReadShortcuts.ts) [SRC+DOC]
- Typing indicators: ephemeral kind 20002, Redis pub/sub cross-node (crates/buzz-pubsub; NOSTR.md); desktop features/messages/useTypingBroadcast.ts and useChannelTyping.ts [SRC]
- Presence and status: ephemeral presence kind 20001 (Redis); user status kind 30315 with expiry (desktop/src/features/user-status/, including statusExpiration.test.mjs); CLI "users presence|setpresence|setstatus"; visible presence is separate from desktop activity (CONTEXT.md) [SRC+DOC]
- Reminders: event reminders NIP-ER (kind 30300, docs/nips/NIP-ER.md); stream reminders kind 40007; route /reminders; desktop features/reminders/useReminderNotifications.ts [SRC+DOC]
- System and membership notices: system message kind 40099; membership notices kinds 44100 and 44101 (kinds.ts; NOSTR.md) [SRC]
- Product feedback form: kind 42000; relay /feedback routes; desktop features/settings/ui/SendFeedbackDialog.tsx [SRC]

### 3.2 Org and membership
- Communities (tenant = host or URL): multi-community deployment; operator provisioning and lifecycle endpoints under /operator/communities (provision, archive, unarchive, delete, transfer, availability); docs/multi-tenant-relay.md [SRC+DOC]
- Hosted community limit: raised to fifty (CHANGELOG v0.5.28) [SRC]
- Community switching: desktop/src/features/communities/ (30 files); CommunityRail.tsx (drag sort via dnd-kit); app/communityViewTransition.ts; AGENTS.md "Community Switching" [SRC+DOC]
- Per-community theme: kind 30078 community theme; shared/theme/CommunityThemeController.tsx [SRC]
- Channel roles: owner, admin, member, guest, bot (crates/buzz-relay/src/handlers/channel_authz.rs; desktop/src/features/channels/lib/memberUtils.ts rank map) [SRC]
- Relay operator roles and admin actions: kinds 9030-9033 (add member, remove member, change role, set workspace profile; kind.rs); /operators routes (crates/buzz-relay/src/api/admin/mod.rs); migrations 0035, 0036-0038, 0039, 0047, 0055 [SRC]
- Relay membership (NIP-43): add, remove, list, change role (NOSTR.md "Relay Membership"; buzz-admin CLI; commands add_relay_member, change_relay_member_role) [SRC+DOC]
- Guest access: channel-scoped guest role selectable in add-agent and add-team dialogs (desktop/src/features/agents/ui/AddAgentToChannelDialog.tsx; features/channels/ui/ChannelMemberInviteCard.tsx); VISION.md describes scoped guest tokens [SRC+DOC]
- Channel management: create, update, archive, unarchive, delete, duplicate; topic; purpose; join and leave; add and remove members; change member role; CLI "channels" group [SRC]
- Channel add policy: owner_only, nobody, anyone (NOSTR.md; env BUZZ_ACP_ALLOWED_CHANNEL_ADD_POLICIES; CLI "channels setaddpolicy") [SRC+DOC]
- Channel visibility: open or private (CLI ChannelVisibility; VISION.md "Access" table) [SRC+DOC]
- Channel templates: create, list, update, duplicate, delete channel templates; useApplyTemplate.ts; ChannelTemplatesSettingsCard.tsx [SRC]
- Starter channels: ensure_starter_channels command [SRC]
- Open channel directory: get_open_channel_directory; features/channels/openChannelDirectory.ts [SRC]
- Member and user directory: GET /members/search, search_users, get_users_batch, get_profile; CLI "users get" [SRC]
- Restrictions: ban, timeout, list restrictions (/members/{pubkey}/ban, /members/{pubkey}/timeout, /members/restrictions; api/admin/mod.rs) [SRC]
- Invites: stateless invite codes minted and claimed over NIP-98 (POST /api/invites, POST /api/invites/claim, POST /api/invites/accept-policy; crates/buzz-relay/src/api/invites.rs); invite code arrives via deep-link parameter (desktop deep_link.rs "code") [SRC]
- Join policy with terms and privacy pages (GET /api/join-policy, /terms, /privacy; desktop opens them in the system browser) [SRC]
- Onboarding: desktop/src/features/onboarding/ (56 files: community onboarding, machine onboarding, claim invite, welcome kickoff stage and entrance hooks) [SRC]
- Welcome kickoff: a built-in agent named Fizz posts an opener and closer in the Welcome channel, with teammates introducing themselves in a thread (docs/welcome-kickoff-silent-failures.md; desktop/src/features/onboarding/) [SRC+DOC]
- Device pairing: NIP-AB pairing with SAS confirmation (start_pairing, confirm_pairing_sas, cancel_pairing); pair relay sidecar (crates/buzz-pair-relay); pairing CLI (crates/buzz-pairing-cli); desktop MobilePairingCard.tsx; mobile lib/features/pairing [SRC]. Note: no docs/nips/NIP-AB.md exists; the spec lives only in code.
- Identity recovery and key backup: start_identity_recovery_pairing; NIP-49 ncryptsec backups (key_backup.rs); HPKE-wrapped backups (hpke_key_backup.rs); settings EncryptedBackupCreator.tsx, BackupTestFlow.tsx, PrivateKeyBackupRow.tsx [SRC]
- Multiple identities: identity archive and unarchive (archive_identity, list_archived_identities, unarchive_identity); shared identity (is_shared_identity, persist_current_identity); sign_out [SRC]
- Shared (cross-community) channels: not found. "shared" hits are cache names only [SRC; INF, conf med]

### 3.3 Calls and meetings
- Huddles per channel: WebSocket Opus relay at /huddle/{channel_id}/audio; NIP-42 auth; membership check; in-memory room; soft cap 25 peers, hard cap 255 (crates/buzz-relay/src/audio/handler.rs; ARCHITECTURE.md "Huddle Audio") [SRC+DOC]
- Huddle lifecycle events: started 48100, participant joined 48101, participant left 48102, ended 48103, liveness 48104, guidelines 48106; huddle reaction kind 24810 (kind.rs; kinds.ts) [SRC]
- Huddle client UI: desktop/src/features/huddle/ (HuddleBar, ParticipantList, MicControls, AddAgentDialog, AgentVoiceMenu, HuddleTranscriptIntro, HuddleAttachment, HuddleRoomHeader) [SRC]
- Huddle media engine (desktop): Opus with NetEq jitter buffer (desktop/src-tauri/src/huddle/jitter.rs); playout.rs; pipeline.rs; reconnect.rs; rodio output; open_huddle_window companion window [SRC]
- Agents in huddles: add_agent_to_huddle, remove_agent_from_huddle, speak_agent_message, interrupt_huddle_speech, sync_agents_to_active_huddle; agent TTS publisher (huddle/agent_tts_publisher.rs) [SRC]
- Local speech pipeline: STT via sherpa-onnx with earshot VAD and a 300 ms silence flush (huddle/stt.rs; huddle/latency_bench.rs); local TTS via Pocket (huddle/pocket.rs; crates/buzz-voice) [SRC]
- Transcript toggle: set_huddle_transcription_enabled; HuddleBar "Start transcript" (Captions icon). No on-screen caption overlay found [SRC]
- Push-to-talk: global Ctrl+Space while a huddle is connected (desktop/src-tauri/src/ptt_shortcut.rs) [SRC]
- Mobile huddles MVP: mobile/HUDDLES.md (control plane, media plane protocol v2, native boundary) [DOC]
- Not built: huddle recording and per-track publishing. Kinds are reserved, no producer (ARCHITECTURE.md section 9 item 4; VISION.md "Huddles") [DOC]
- Not found: screen share, video calls, whiteboard, calendar scheduling, AI meeting recap [SRC; INF, conf med from keyword checks]
- Voice models and voices: download_voice_models, list_voice_registry, import_pocket_voice, preview_pocket_voice, set_pocket_voice, delete_pocket_voice; desktop TTS settings (VoiceSettingsCard.tsx; set_tts_settings) [SRC]

### 3.4 Search and discovery
- Keyboard entry points: Ctrl/Cmd+K search everything; Ctrl/Cmd+F search current channel; Ctrl/Cmd+Shift+K new message; Ctrl/Cmd+Shift+N create channel (desktop/src/app/useAppShellKeyboardShortcuts.ts) [SRC]
- Full-text search: Postgres FTS with a generated search_tsv column, community-scoped (crates/buzz-search/src/query.rs, lib.rs; ARCHITECTURE.md section 8; FTS-related migrations 0005, 0008, 0033) [SRC+DOC]
- NIP-50 one-shot search REQs (not persistent subscriptions) (NOSTR.md) [SRC+DOC]
- Search filters: CLI "messages search" supports --query, --author (hex, npub, or display name), --since, --limit; command search_messages [SRC]
- People search: search_users, GET /members/search [SRC]
- Search history: desktop/src/shared/hooks/useHistorySearchState.ts [SRC]
- Search highlight route state: desktop/src/app/routes/searchHighlightRouteState.ts; features/search/ [SRC]
- Semantic or vector search: not found (no pgvector or embedding use in buzz-search, buzz-db or migrations) [SRC; INF, conf med]
- Agent-driven retrieval ("ask the project"): agents query history through buzz CLI search and thread reads (README "Three little stories"; VISION.md "Incident memory") [DOC]
- Pulse feed: notes, reactions, agent activity (desktop/src/features/pulse/; commands get_global_notes, get_user_notes, get_notes_timeline, publish_note) [SRC]

### 3.5 Notifications and attention
- Desktop notifications: desktop/src/features/notifications/ (use-feed-desktop-notifications.ts, useNotificationSenderName.ts); show_native_notification; request_notification_access; notification_permission_state; macos_notifications.rs; tauri-plugin-notification [SRC]
- Mobile push via APNs: crates/buzz-push-gateway (apns.rs, app_attest.rs, grant.rs, token.rs); NIP-PL push leases (kind 30350; crates/buzz-relay/src/handlers/push_lease.rs; docs/nips/NIP-PL.md); push_runtime.rs in the relay [SRC+DOC]
- Push transports named in NIP-PL: apns, fcm, unifiedpush. Only APNs delivery code found in the gateway. "fcm" appears only in a relay test fixture [SRC+DOC; INF, conf med]
- Push suppression while desktop is active: docs/mobile-push-suppression.md; CONTEXT.md glossary (desktop activity, inactivity cutoff, additional notification delay) [SRC+DOC]
- Zero-notification defaults per surface: Stream and Forum zero, DMs URGENT only, Workflows approvals only (VISION.md "Surfaces") [DOC]
- Mention routing to the home feed: HOME_MENTION_EVENT_KINDS; personal inbox (desktop/src/features/home/useHomePersonalInbox.ts); hidden DM inbox (useHiddenDmInboxNavigation.ts) [SRC]
- Unread: unread_catch_up.rs, observed_unread.rs, unread_catch_up_recovery_tests.rs; NIP-RS sync [SRC]
- Channel mute: per-channel mute list (kind 30078 mutes tag; about 13 sidebar files reference mute) [SRC]
- Do-not-disturb or quiet hours: not found [SRC; INF, conf med]
- Digest or email summary: not found; the activity surfaces are Home inbox and Pulse [SRC; INF]
- Tray: "Running" and "Recent" agent activity items; tray action queue (desktop/src-tauri/src/tray_menu.rs; take_tray_actions, update_tray_agent_activity, requeue_tray_actions) [SRC]
- Notification sounds: SoundPicker.tsx in settings [SRC]
- Mobile badges: app_badge_plus (mobile/pubspec.yaml) [SRC]
- Reminder notifications: desktop/src/features/reminders/useReminderNotifications.ts [SRC]

### 3.6 Apps and extensibility
- Workflows (YAML-as-code): triggers message_posted, reaction_added, diff_posted, schedule (cron crate), webhook; actions send_message, send_dm, set_channel_topic, add_reaction, call_webhook, request_approval, delay; "if" conditions via evalexpr (crates/buzz-workflow/src/schema.rs, executor.rs) [SRC]
- Workflow stubs: send_dm and set_channel_topic return NotImplemented (crates/buzz-workflow/src/error.rs "action_not_implemented"; ARCHITECTURE.md section 9 item 6, WF-07) [SRC+DOC]
- Workflow approval gates: schema, REST (/workflows/{id}/runs, /workflows/{id}/runs/{run}/approvals), grant and deny kinds 46030 and 46031, commands grant_approval, deny_approval. Executor does not persist or resume (ARCHITECTURE.md section 9 item 5; VISION.md WF-08) [SRC+DOC]
- Workflow UI and runs: desktop/src/features/workflows/ (48 files; ui/); routes /workflows and /workflows/$workflowId; workflow run and step kinds 46001-46012 (kind.rs) [SRC]
- Workflow flag: preview-features.json id "workflows" [SRC]
- Inbound webhooks: POST /hooks/{id}, secret-authenticated, no NIP-98 (crates/buzz-relay/src/router.rs; api/bridge.rs) [SRC]
- Outbound webhooks: call_webhook action (reqwest); SSRF protections (ARCHITECTURE.md section 7) [SRC+DOC]
- Bots: countdown-bot example (NIP-42 bot; deterministic commands such as !countdown and !fib; owner-attested BUZZ_AUTH_TAG path) (examples/countdown-bot; examples/README.md) [SRC+DOC]
- Client SDKs: buzz-sdk (typed event builders), buzz-ws-client, buzz-test-client (NIP-01 test client) [SRC]
- HTTP bridge: POST /events, POST /query, POST /count, NIP-98 auth (crates/buzz-relay/src/router.rs; api/bridge.rs) [SRC]
- Buzz v1 accessory APIs: GET /me/sidebar, POST /me/read-state (crates/buzz-relay/src/api/buzz_v1/) [SRC]
- Custom ACP harnesses: user-defined harness JSON (desktop/src-tauri/src/managed_agents/custom_harnesses.rs; desktop/src/features/settings/ui/CustomHarnessForm.tsx; AddCustomHarnessDialog.tsx) [SRC]
- Built-in harness IDs: goose, claude, codex, buzz-agent (custom_harnesses.rs TIER1) [SRC]
- Agent runtime slash commands: discover_acp_commands; AcpCommandField.tsx. The culture "/ship" command is planned only (VISION.md) [SRC+DOC]
- MCP tools for agents: buzz-dev-mcp exposes shell, read_file, view_image, str_replace, todo, plus _Stop and _PostCompact hooks (crates/buzz-dev-mcp/src/lib.rs) [SRC]
- Default MCP server for hosted agents: BUZZ_ACP_MCP_COMMAND=buzz-dev-mcp (crates/buzz-backend-kubernetes/src/env.rs, line 267) [SRC]
- Per-agent MCP servers: desktop/src/features/agents/ui/McpServersSection.tsx; crates/buzz-agent/src/mcp.rs [SRC]
- Persona packs: .plugin/plugin.json manifest, *.persona.md files, pack_instructions, mcp_config, hooks_config, skills directory (crates/buzz-persona/src/manifest.rs; examples/meadow-core) [SRC]
- Lifecycle hooks: MCP-driven _Stop and _PostCompact (docs/MCP_DRIVEN_HOOKS.md; buzz-dev-mcp) [SRC+DOC]
- Skills: SKILL.md-style skill files (desktop/src-tauri/src/managed_agents/nest_skill.md, which documents buzz-cli). screenshot_skill.md is developer-only [SRC]
- Forms: product feedback form only; no general form builder found [SRC; INF]
- Custom emoji: see 3.1 [SRC]
- App directory, marketplace, tabs, pinned apps: not found [SRC; INF, conf med]
- OpenAPI or GraphQL spec for Buzz's own API: not found; openapi appears only as Kubernetes client types (crates/buzz-backend-kubernetes) [SRC]
- Preview features: preview-features.json (workflows, projects, pulse, forum, agentManagedProfiles); desktop ExperimentalFeaturesCard.tsx [SRC]
- Internal-only feature "bestie" (personal agent, "Try a personal agent that is always close at hand"): defined in desktop/src/protectedFeatures/internal.ts; public.ts exports an empty list [SRC]
- Mesh compute as a provider: see 3.7 [DOC]

### 3.7 Agents in the room
- Agent identity: each agent has its own Nostr keypair and profile. Agents are channel members with the bot role. NIP-AA agent auth (docs/nips/NIP-AA.md); NIP-OA owner attestation via auth tag (docs/nips/NIP-OA.md; env BUZZ_AUTH_TAG; BUZZ_ALLOW_NIP_OA_AUTH) [SRC+DOC]
- Agent profile identity and agent-managed profiles: docs/agent-profile-identity.md; preview flag agentManagedProfiles; set_agent_managed_profiles [SRC+DOC]
- Managed agents on desktop: create, start, stop, restart managed agent and runtime; process lifecycle; auto-restart and start-on-launch toggles (desktop/src-tauri/src/managed_agents/process_lifecycle.rs; commands set_managed_agent_auto_restart, set_managed_agent_start_on_app_launch) [SRC]
- Backend kinds: Local, or Provider {id, config} for remote substrates (desktop/src-tauri/src/managed_agents/types.rs BackendKind) [SRC]
- Remote agents: provider protocol (discovery, info, deploy, launch, stop, delete; inactivity self-stop) in docs/remote-agents.md; Kubernetes backend (crates/buzz-backend-kubernetes); commands discover_backend_providers, probe_backend_provider [SRC+DOC]
- Remote-agent status: VISION.md lists remote agents as "spec in review", Kubernetes first [DOC]
- Personal agent "Bestie": one durable designation per owner and retention scope (desktop/src-tauri/src/managed_agents/bestie_assignment.rs); commands assign_bestie, clear_bestie_assignment, get_bestie_assignment, resolve_bestie_conversation; internal build only (see 3.6) [SRC]
- Buzz Nest: persistent workspace at ~/.buzz with AGENTS.md, GUIDES/, PLANS/, RESEARCH/, WORK_LOGS/, OUTBOX/, REPOS/, .scratch/ (desktop/src-tauri/src/managed_agents/nest.rs; nest_agents.md) [SRC]
- Personas: create, update, delete, set active or shared, publish to catalog (create_persona, update_persona, update_persona_and_publish, set_persona_active, set_persona_shared, delete_persona; managed_agents/personas.rs; persona_catalog.rs; fetch_persona_catalog) [SRC]
- Teams: create, update, delete, add from catalog, export and import team snapshots (create_team, add_team_from_catalog, export_team_snapshot, confirm_team_snapshot_import; managed_agents/teams.rs; team_catalog.rs) [SRC]
- Agent snapshots: export, preview import, confirm import (managed_agents/spawn_snapshot/; agent_snapshot_envelope.rs; AgentSnapshotExportDialog.tsx; AgentSnapshotImportDialog.tsx) [SRC]
- Agent cards: PNG ".agent.png" with an embedded, round-trip-verified snapshot. Commands mint_agent_card, load_agent_card, list_agent_cards, save_agent_card; reroll; OpenAI key stored for card minting (commands/personas/card.rs; features/agents/ui/AgentCardMintDialog.tsx; cardMintStore.ts; card_mint_save_openai_key). Image generation via OpenAI is inferred [SRC; INF, conf med]
- Agent @-invocation and trigger modes: respond_to owner-only, allowlist, anyone (desktop RespondTo); buzz-acp adds nobody (crates/buzz-acp/src/config.rs); persona triggers mentions, keywords, all_messages; lifecycle hooks on_start, on_stop, on_message (crates/buzz-persona/src/persona.rs) [SRC]
- Owner-only access control in UI: features/agents/ui/OwnerOnlyAccessField.tsx; agent_access_owner_only command [SRC]
- Per-channel sessions: one independent session per agent identity per channel, thread or task (crates/buzz-acp/src/session_model_channel.md, session_model_thread.md, session_model_task.md; env BUZZ_ACP_SESSION_POLICY) [SRC]
- Agent work visibility: AgentSessionTranscriptList, AgentSessionToolItem (tool-call cards), ManagedAgentLogPanel, FileEditDiffView (features/agents/ui/); observer frames kind 24200 (NIP-AO; decrypt_observer_event; crates/buzz-acp/src/observer.rs); turn metrics kind 44200 (NIP-AM; crates/buzz-acp/src/usage.rs) [SRC+DOC]
- Activity feed: desktop/src/features/pulse/ui/AgentActivityCard.tsx; VISION_ACTIVITY.md render classes (verb, object, outcome) [SRC+DOC]
- Human approval of tool calls: buzz-agent permission broker (session/request_permission) for every LLM-issued MCP tool call (crates/buzz-agent/src/permission.rs); env BUZZ_ACP_PERMISSION_MODE, BUZZ_AGENT_PERMISSION_TIMEOUT_SECS [SRC]
- Owner-reviewed agent drafts: CLI "agents draft-create|draft-update" require BUZZ_AUTH_TAG (nest_skill.md) [SRC]
- Multi-agent coordination: orchestrator plus workers (examples/meadow-core: Skip, Lev, Bana); job kinds 43001-43006 (request, accepted, progress, result, cancel, error); env BUZZ_ACP_TEAM_INSTRUCTIONS; Harbor orchestra benchmark (benchmarks/harbor-buzz-orchestra) [SRC+DOC]
- Handoff: context-recovery ladder and summarizer (crates/buzz-agent/src/handoff.rs); BUZZ_AGENT_MAX_HANDOFFS [SRC]
- Memory: NIP-AE engrams (kind 30174), core engram fetched per session (crates/buzz-acp/src/engram_fetch.rs); agent memory CLI "mem ls|get|hash|set|patch|rm"; desktop features/agent-memory; env BUZZ_ACP_NO_MEMORY [SRC+DOC]
- Scheduled and recurring agent work: heartbeat (BUZZ_ACP_HEARTBEAT_INTERVAL; BUZZ_ACP_HEARTBEAT_PROMPT or _FILE); workflow schedule triggers; recovery wake (crates/buzz-acp/src/recovery_wake.rs) [SRC]
- Idle and lifetime bounds: BUZZ_ACP_IDLE_TIMEOUT, BUZZ_ACP_EXIT_AFTER_INACTIVITY, BUZZ_ACP_MAX_TURN_DURATION, BUZZ_ACP_MAX_TURNS_PER_SESSION; remote agents self-stop on inactivity (docs/remote-agents.md) [SRC+DOC]
- Agent execution sandbox: one fresh process per local task (crates/buzz-acp/src/isolated_execution.rs); shell runs through buzz-dev-mcp on the host or in the pod. No browser automation found (no playwright, puppeteer, or headless in agent crates) [SRC; INF, conf med: no browser or computer-use capability]
- Secrets and env: per-agent env var editor (features/agents/ui/EnvVarsEditor.tsx; managed_agents/env_vars.rs; reserved_env_keys.rs); keyring-backed secret store (desktop/src-tauri/src/secret_store.rs) [SRC]
- Cost and usage: get_agent_usage_series; usage archive (desktop/src-tauri/src/archive/agent_usage.rs); NIP-AM metrics; docs/usage-metrics.md; BUZZ_USAGE_METRICS_* env. No spend cap or budget found [SRC+DOC; INF]
- Turn and round limits: BUZZ_AGENT_MAX_ROUNDS, BUZZ_AGENT_MAX_TOKEN_RECOVERIES, BUZZ_AGENT_TOOL_TIMEOUT_SECS, BUZZ_AGENT_LLM_TIMEOUT_SECS [SRC]
- Model providers for agents: Anthropic, OpenAI-compatible, Databricks (serving endpoints and AI Gateway v2), OpenRouter (crates/buzz-agent/src/config.rs Provider enum; llm.rs) [SRC]
- Model discovery and effort: discover_agent_models, get_model_status; BUZZ_AGENT_THINKING_EFFORT; BUZZ_AGENT_PROMPT_CACHING [SRC]
- OAuth for Databricks: PKCE in crates/buzz-agent/src/auth.rs with webbrowser [SRC]
- Availability and ownership: docs/agent-availability.md; docs/owned-agent-discovery.md; docs/remote-mention-routing.md [DOC]
- Mesh compute for agents: members share local models; agents use a local OpenAI-compatible endpoint (VISION_MESH.md). Desktop mesh_llm/ and commands mesh_start_node, mesh_stop_node, mesh_node_status, mesh_model_catalog, mesh_installed_models, mesh_serving_usage (features/mesh-compute/) [SRC+DOC]
- Developer-only skill: desktop-screenshot (contributor tooling, not product) [SRC]

### 3.8 Collaborative artifacts
- Canvases: one shared document per channel (kind 40100); get_canvas, set_canvas, get_canvas_history, restore; conflict handling (desktop/src/features/channels/canvasConflict.ts; ui/CanvasHistoryPanel.tsx; CLI "canvas get|set|history|restore") [SRC]
- Channel artifacts (NIP-AR): kinds 45010 and 45011 (removal); relay handlers crates/buzz-relay/src/handlers/artifact.rs; api/artifact.rs; migration 0052_channel_artifacts.sql. Desktop UI is thin (about 4 files mention artifact) [SRC+DOC; INF, conf med]
- Git repositories hosted by the relay: smart HTTP at /git/{owner}/{repo}/info/refs, git-upload-pack, git-receive-pack (crates/buzz-relay/src/api/git/transport.rs); policy hook /internal/git/policy (api/git/policy.rs; hook.rs); repo settings (api/git/settings.rs) [SRC]
- Repo model (NIP-34 and NIP-MP): repo announcements 30617, repo state 30618, project announcement 30621, patches 1617, pull requests 1618 and 1619, issues 1621, statuses 1630-1633 (kind.rs; desktop kinds.ts; docs/nips/NIP-MP.md) [SRC+DOC]
- Projects: create, delete, attach and bind repos, project channels and channel requests, owner control, project home repos (desktop/src/features/projects/, 218 files) [SRC]
- Branch as channel: create_project_remote_branch; project channel creation (projectChannelCreation.ts); VISION_PROJECTS.md "Branches as Channels" [SRC+DOC]
- Merge flow: merge_project_pull_request; publish_project_pull_request_merged_status; pullRequestReviews.ts; projectPullRequestConflictRecovery.ts [SRC]
- Review requests and status: sign_project_pull_request_review_request; sign_project_pull_request_status [SRC]
- Issues: create, get, list, status, assign, unassign (CLI "issues" group; sign_project_issue_assignment, issueAssignments.ts) [SRC]
- Branch protection: CLI "repos protect list|set|remove" and "repos defaultbranch"; api/git/settings.rs [SRC]
- Local checkouts and terminal: list_project_local_repositories, pull and push local repository, open_project_terminal; PTY terminal (desktop/src-tauri/crates/buzz-terminal; features/terminal/) [SRC]
- Repo browser in the browser: web/ with isomorphic-git and lightning-fs; web/src/features/repos/ [SRC]
- Git signing and credentials: git-sign-nostr (NIP-GS, docs/nips/NIP-GS.md) and git-credential-nostr (NIP-98) [SRC+DOC]
- Git object storage: refs over object storage with pack cache (docs/git-on-object-storage.md; env BUZZ_GIT_PACK_CACHE_*) [SRC+DOC]
- Files and storage: uploads to S3 or MinIO; bundled object storage named Silo (CHANGELOG v0.5.28; deploy/compose and deploy/charts); storage accounting (docs/storage-accounting.md; migration 0046) [SRC+DOC]
- Shared knowledge: Nest workspace; NIP-AE engrams; Knowledge Crystallization is planned (VISION.md "Culture Features") [SRC+DOC]
- Tasks, lists, boards: project work items (projectWorkItems.ts, projectTaskCategories.ts). No kanban board UI found [SRC; INF, conf low]
- Docs as canvases: VISION_PROJECTS.md table ("Docs to Canvases") [DOC]

### 3.9 Realtime and sync architecture
- Transport: NIP-01 over WebSocket at "/" (crates/buzz-relay/src/router.rs); NIP-11 relay info on GET /; NIP-05 at /.well-known/nostr.json; /health, /_liveness, /_readiness, /_status, /_mesh [SRC]
- Fan-out: subscription registry with three-tier fan-out (ARCHITECTURE.md section 5); Redis pub/sub across nodes (crates/buzz-pubsub) [SRC+DOC]
- Connection lifecycle: community binding, semaphore, NIP-42 challenge, active loops, cleanup (ARCHITECTURE.md section 3) [DOC]
- Ordering and fences: created_at fence floor (migration 0021); NIP-33 last-write-wins with exit code 5 on conflict (CLI exit codes; AGENTS.md) [SRC+DOC]
- Kind ranges: replaceable 10000-19999, ephemeral 20000-29999 (not stored), parameterized replaceable 30000-39999, Buzz custom 40000-49999 (ARCHITECTURE.md section 2) [DOC]
- Event store: Postgres 17, partitioned monthly (VISION.md "Scale"; schema/partitions/; partition manager envs BUZZ_PARTITION_MANAGER_*) [SRC+DOC]
- Read scaling: replica routing design (PLANS/REPLICA_FULL_READ_ROUTING_DESIGN.md, active); env BUZZ_REPLICA_*; bus scaling study perf/RELAY_BUS_SCALING.md [SRC+DOC]
- Optimistic UI: optimistic patterns in about 10 files under features/messages/, plus project hooks (e.g., features/projects/useOptimisticProjectBranches.ts) [SRC]
- Offline and local cache: desktop local archive (SQLite via rusqlite; features/local-archive/; archive/sync.rs; event_sync.rs); channel_head_cache.rs; unread catch-up [SRC]
- Mobile offline-first design: VISION_MOBILE.md (show cached content immediately; queue actions) [DOC]
- Reconnect: relay_reconnect_hook and relay_reconnect_hook_configured commands; relay_requires_membership [SRC]
- Multi-device: NIP-RS read-state sync; device pairing (NIP-AB); identity recovery pairing [SRC+DOC]
- Encryption: channels and DMs are server-managed, with no end-to-end encryption for channels (VISION.md "Encryption"). NIP-44 to-self encryption for local data (nip44_encrypt_to_self, nip44_decrypt_from_self); NIP-49 and HPKE key backups [SRC+DOC]
- Egress guard: NIP-49 key-backup material must not reach the relay (desktop/src-tauri/src/egress_guard.rs) [SRC]
- Rate limiting: Redis fixed-window limiter (crates/buzz-pubsub/src/rate_limiter.rs; its header warns fixed windows allow 2x bursts) with tiers in crates/buzz-auth/src/rate_limit.rs; env BUZZ_RATE_LIMIT_*. ARCHITECTURE.md section 9 item 2 says no limiter is enforced (stale; see section 6) [SRC; conflict]
- Backpressure: bounded per-peer audio channels with drop-on-full; env BUZZ_SLOW_CLIENT_GRACE_LIMIT, BUZZ_MAX_CONNECTIONS, BUZZ_MAX_CONCURRENT_HANDLERS [SRC]
- Observability: Prometheus metrics, OpenTelemetry OTLP traces, usage metrics (prometheus.yml; BUZZ_METRICS_PORT, BUZZ_HEALTH_PORT) [SRC]
- Scale targets: 10K humans, 50K agents, about 600K events per day, Redis fan-out under 50 ms p99 (VISION.md "Scale"). These are targets, not measurements [DOC]

### 3.10 Governance, security and compliance
- Audit log: per-community hash chain (crates/buzz-audit; rows keyed by community_id and seq); migrations 0039 (operator audit), 0048 (hash version); env BUZZ_AUDIT_ENABLED [SRC+DOC]
- Retention: NIP-RS retention (migration 0007); event TTL refresh (migrations 0022, 0024); observer retention days; agent-metric archive retention (archive/retention.rs) [SRC]
- Moderation: reports (kind 1984; /reports, resolve, reopen, cancel), bans 9040, unbans 9041, timeouts 9042, untimeouts 9043, report resolution 9044; moderation audit and restricted lists (crates/buzz-relay/src/router.rs; docs/nips) [SRC]
- Moderation model: two layers (community versus platform safety), private reports, signed moderation commands, honest tombstones (VISION_MODERATION.md) [DOC]
- Escalation: writes a durable record for the platform operator; the inbox is a separate build (VISION_MODERATION.md "Escalation is a hook today") [DOC]
- Admin console (desktop): reports, feedback, communities and actions tabs; about 25 admin_* commands; admin-web SPA [SRC]
- Operator CLI: buzz-admin (crates/buzz-admin) [SRC]
- Federated identity: NIP-FI JWT assertions (crates/buzz-relay/src/api/nip_fi.rs; env BUZZ_NIP_FI_MODE, _ISSUERS, _COMMUNITIES, _MAXIMUM_ASSERTION_AGE_SECS); NIP-FA adapter (docs/federated-identity-adapter.md; docs/nips/NIP-FI.md, NIP-FA.md) [SRC+DOC]
- Enterprise auth adapter: build-time env BUZZ_BUILD_ENTERPRISE_AUTH_ADAPTER_BASE_URL and BUZZ_BUILD_ENTERPRISE_AUTH_RELAYS [SRC]
- SSO, SAML and SCIM: not found. OIDC appears only in buzz-agent's LLM OAuth (crates/buzz-agent/src/auth.rs) [SRC; INF, conf med]
- DLP and data-residency controls: not found [SRC; INF, conf med]
- eDiscovery-like export: local archive reads and exports (archive_events, read_archived_events, read_archived_observer_events_for_channel); CLI "emoji export" [SRC; INF, conf low that this is a compliance export]
- Tenant isolation: TLA+ spec (docs/spec/MultiTenantRelay.tla), Tamarin spec (docs/spec/MultiTenantAuth.spthy), conformance replay crate (crates/buzz-conformance), docs/multi-tenant-conformance.md [SRC+DOC]
- Community deletion: whole-community deletion engine (crates/buzz-deletion); operator job docs/operator-community-deletion.md; migrations 0029, 0030, 0051, 0053, 0054 [SRC+DOC]
- Information-flow control: crates/ifc-core (generic reader-set labels; no dependent crate found in the repo) and docs/practical-information-flow-for-buzz-agents.md (audience-bound agent instances, trusted broker, managed memory) [SRC+DOC; INF, conf med on "no dependents"]
- Agent sandboxing: crates/buzz-acp/src/isolated_execution.rs; process-group kill on every exit path (VISION_AGENT.md) [SRC+DOC]
- SSRF protection and webhook security: ARCHITECTURE.md section 7 [DOC]
- Webhook and git hook secrets: BUZZ_HOOK_SECRET, BUZZ_GIT_HOOK_HMAC_SECRET; secret-header checks in api/bridge.rs, api/git/hook.rs, api/git/policy.rs [SRC]
- Replay guards: nip98_replay.rs, nip_fi_command_replay.rs (crates/buzz-pubsub) [SRC]
- Mobile age gate: native age-signal request and under-18 restriction page (mobile/lib/features/age_gate/: age_signal_provider.dart, age_restriction_page.dart); env BUZZ_AGE_ATTESTATION_REQUIRED [SRC]
- Terms and privacy acceptance on join (POST /api/invites/accept-policy) [SRC]
- Supply chain: deny.toml (cargo-deny), lefthook.yml, Justfile recipe security-review-check, SECURITY.md, .intersect/sadscan.yaml (scanner config, not read) [SRC]
- Feature-flag gating: LaunchDarkly evaluator with a static default evaluator; only integer key documented is db.events.query-version (crates/buzz-feature-flags/README.md) [SRC]

### 3.11 Client shell
- Tauri 2 app: single-instance, window-state, dialog, process, opener plugins; identifier xyz.block.buzz.app (desktop/src-tauri/Cargo.toml; tauri.conf.json) [SRC]
- Deep links: scheme "buzz" (tauri.conf.json plugins.deep-link.desktop.schemes); pending deep-link queues (take_pending_community_deep_link, take_pending_entity_deep_link, take_pending_navigation_deep_link) [SRC]
- Auto-update: tauri-plugin-updater; settings update card (features/settings/SidebarUpdateCard.tsx, UpdateChecker.tsx, UpdateIndicator.tsx); is_auto_update_supported. Endpoint and public key come from build env (BUZZ_UPDATER_ENDPOINT, BUZZ_UPDATER_PUBLIC_KEY); tauri.conf.json lists no endpoints [SRC; INF, conf med on build-time injection]
- Release channels: .release/desktop-candidate.json; RELEASING.md (signed macOS canary, promotion to auto-update) [DOC]
- Packaging: dmg (arm64, x64), AppImage, deb, unsigned Windows setup exe (README.md "Install") [DOC]
- Global shortcuts: tauri-plugin-global-shortcut (push-to-talk); KeyboardShortcutsCard.tsx; zoom shortcuts (app/useWebviewZoomShortcuts.ts); close window (app/useCloseWindowShortcut.ts); settings (app/useSettingsShortcuts.ts); reload (app/useReloadShortcut.ts); mouse back and forward (mouse_nav.rs) [SRC]
- Command palette: none (no cmdk or CommandPalette component). Cmd+K opens search instead [SRC]
- Native menus: app_menu.rs (macOS app, Edit, View, Window, Help submenus) [SRC]
- Window chrome: set_window_vibrancy (macOS), title_bar_double_click, community view transitions [SRC]
- Sleep prevention: prevent_sleep.rs (macOS power assertion while managed agents run); PreventSleepSettingsCard.tsx [SRC]
- Multi-account and workspace switching: community rail; identity archive; shared identity; hosted communities card (HostedCommunitiesSettingsCard.tsx) [SRC]
- Accessibility: aria attributes in 416 desktop source files; reduced-motion references in 74 files; VISION_MOBILE.md accessibility goals (screen readers, text scaling, reduced motion) [SRC+DOC]
- Internationalization: no i18n library in desktop/src, web/src or admin-web/src; mobile has no .arb or l10n bundles. UI strings appear English-only [SRC; INF, conf med]
- Themes: adaptive theme, theme loader, terminal palette, community theme; animated Grainient background (desktop/src/shared/theme/; app/ThemeGrainientBackground.tsx); AppearanceSettingsControls.tsx [SRC]
- Build-time branding env: BUZZ_THEME_NAME, BUZZ_BASE_THEME, BUZZ_DARK_BASE_THEME [SRC]
- Windows shell: Git Bash prerequisite (managed_agents/git_bash.rs; discover_git_bash_prerequisite); env BUZZ_SHELL (README.md "Windows prerequisites") [SRC+DOC]
- Linux media capture: linux_media.rs (WebKitGTK getUserMedia) [SRC]
- Terminal: PTY sessions (terminal_attach, terminal_input, terminal_resize, terminal_scroll, terminal_focus, terminal_close, terminal_detach) (desktop/src-tauri/crates/buzz-terminal; features/terminal/) [SRC]
- Encrypted backups and sign-out: EncryptedBackupProvider.tsx; SignOutSection.tsx [SRC]
- Test bridge: desktop/src/testing/e2eBridge*.ts [SRC]

### 3.12 UX patterns and layout
- Routes: /, /agents, /pulse, /reminders, /settings, /workflows, /workflows/$workflowId, /projects, /projects/$projectId, /messages/new, /channels/$channelId, /channels/$channelId/posts/$postId (desktop/src/app/routes.ts) [SRC]
- Surfaces: Home, Stream, Forum, DMs, Agents, Workflows, Search; "three lenses": Stream, Forum, Workflow (VISION.md "Surfaces") [DOC]
- Stream versus Forum: Slack-like topic and sub-reply model versus Discourse-like posts with flat replies (VISION.md) [DOC]
- Density preference: desktop/src/shared/lib/conversationDensityPreference.ts [SRC]
- Auxiliary panels: AuxiliaryPanelShell, AuxiliaryPanelHeader, AuxiliaryPanelBody (desktop/src/shared/layout/) for threads, canvas and members [SRC]
- Composer: Tiptap-based editor with mention contract, attachments, voice notes, GIF and emoji autocomplete (desktop/src/features/messages/lib/) [SRC]
- Home inbox and feed: useHomePersonalInbox, useHomeInboxAutoSelection, hidden DM inbox (desktop/src/features/home/; useLiveHomeFeedActions lives in desktop/src/app/) [SRC]
- Pulse: PulseScreen, PulseView, NoteCard, AgentActivityCard, PulseTabBar (desktop/src/features/pulse/ui/) [SRC]
- Onboarding and empty states: welcome kickoff, starter channels, claim invite (desktop/src/features/onboarding/) [SRC]
- Sidebar: sections, stars, mutes, sort, DM sidebar metadata (desktop/src/features/sidebar/; kinds 30078 d tags in kinds.ts) [SRC]
- Thread focus: focusedThreadCloseRequest.ts (desktop/src/features/channels/) [SRC]
- Mobile navigation: feature folders activity, channels, forum, home, pulse, search, settings, profile, invites (mobile/lib/features/) [SRC]
- Motion: motion library, animated count (AnimatedCount.tsx), reduced-motion gating in 74 files [SRC]

---

## 4. Extension points
- Workflow YAML: triggers, actions and conditions (crates/buzz-workflow/src/schema.rs); REST in crates/buzz-relay/src/api/workflows.rs
- Inbound webhook trigger: POST /hooks/{id} (crates/buzz-relay/src/api/bridge.rs)
- Outbound webhook action: call_webhook (crates/buzz-workflow)
- Persona packs: .plugin/plugin.json plus *.persona.md (crates/buzz-persona; examples/meadow-core)
- Custom ACP harnesses: JSON definitions (desktop/src-tauri/src/managed_agents/custom_harnesses.rs)
- Remote-provider binaries: discovery, info, deploy, launch, stop, delete protocol (docs/remote-agents.md; commands discover_backend_providers, probe_backend_provider)
- MCP servers attached per agent (features/agents/ui/McpServersSection.tsx; crates/buzz-agent/src/mcp.rs)
- Skill files: SKILL.md-style (managed_agents/nest_skill.md; persona-pack skills directory)
- Lifecycle hooks: _Stop and _PostCompact (docs/MCP_DRIVEN_HOOKS.md; crates/buzz-dev-mcp)
- HTTP surface: POST /events, /query, /count (NIP-98); /me/sidebar and /me/read-state (buzz_v1); /gifs/search and /gifs/share; /api/invites; /feedback; /reports; /operator/communities; /members (crates/buzz-relay/src/router.rs; api/*)
- WebSocket: NIP-01 with NIP-10, NIP-17, NIP-29, NIP-42, NIP-50 (NOSTR.md)
- Huddle audio WebSocket: frame protocol v2 (ARCHITECTURE.md "Huddle Audio")
- Blossom media: BUD-01 and BUD-02 (router.rs)
- CLI: buzz-cli groups agents, messages, channels, canvas, reactions, emoji, gifs, dms, users, workflows, feed, social, notes, repos, projects, patches, issues, pr, media, upload, mem, pack, moderation; JSON stdout (crates/buzz-cli/src/lib.rs)
- Rust SDK: buzz-sdk typed builders; buzz-ws-client; buzz-test-client
- Nostr NIP drafts: 19 custom specs in docs/nips (new message type = new kind integer, VISION.md "Protocol")
- Tauri IPC: 377 unique #[tauri::command] names; handler registration via generate_handler in desktop/src-tauri/src/lib.rs (line 529) and native_websocket.rs (line 355)
- Feature flags: LaunchDarkly (crates/buzz-feature-flags); desktop preview-features.json; internal protectedFeatures overlay (desktop/src/protectedFeatures/)
- Admin API: /operator/*, /reports, /feedback, /operators, /members (crates/buzz-relay/src/api/admin/mod.rs; router.rs); buzz-admin CLI; admin-web SPA
- Example bot: examples/countdown-bot
- Single binary: sprig (crates/sprig; Dockerfile.sprig; Cargo profile "sprig")
- Benchmark adapter: benchmarks/harbor-buzz-orchestra (Harbor custom agent)
- Environment configuration: several hundred BUZZ_* names (families in section 5)

---

## 5. Data and protocol hints
- Event format: NIP-01 signed events (secp256k1 Schnorr); kind is the only type switch (VISION.md "Protocol"; nostr 0.44) [DOC+SRC]
- Auth kinds: 22242 (NIP-42), 24242 (Blossom auth), 24243 (NOSTR_IDENTITY_BINDING), 27235 (NIP-98 HTTP auth) (crates/buzz-core/src/kind.rs) [SRC]
- Auth flows: NIP-42 for humans on the WebSocket; NIP-98 for REST, git, media and invites; NIP-OA owner attestation via auth tag; NIP-AA agent auth; NIP-FI JWT assertions (jsonwebtoken, aws_lc_rs) [SRC+DOC]
- Pairing: kind 24134 via the pair relay (crates/buzz-pair-relay) [SRC]
- Storage: Postgres 17 (sqlx; partitioned events table with search_tsv; 60 migrations); Redis (pub/sub, presence, rate limits, replay keys); S3 or MinIO (media and git objects); desktop SQLite via rusqlite (local archive) [SRC]
- Replay guards: nip98_replay.rs, nip_fi_command_replay.rs (crates/buzz-pubsub) [SRC]
- Mesh wire: postcard-encoded messages over iroh QUIC; gossip and membership (crates/buzz-relay-mesh) [SRC]
- Huddle audio: 8-byte big-endian header (sequence u16, 48 kHz timestamp u32, level dBov i8, flags u8) plus opaque Opus payload (ARCHITECTURE.md) [DOC]
- Push: NIP-PL leases (kind 30350), gateway grants, APNs (apns.rs), Apple App Attest (app_attest.rs) [SRC+DOC]
- Git: smart HTTP (info/refs, upload-pack, receive-pack); NIP-98 credential helper; NIP-GS signing through gpg.x509.program; object-storage refs and pack cache [SRC+DOC]
- Workflows: YAML via serde_yaml; conditions via evalexpr; schedules via cron [SRC]
- Persona packs: JSON plugin.json manifest and Markdown persona files [SRC]
- Configuration: BUZZ_* environment variables (several hundred names; families below); TOML (Cargo); Tauri JSON configs; .env.example [SRC]
- Desktop sync: archive/sync.rs; event_sync.rs; native_relay_client*.rs; native_websocket*.rs (WebSocket batching) [SRC]
- Relay ingest: handlers/ingest.rs, req.rs, event.rs, side_effects.rs [SRC]
- Key material: NIP-49 ncryptsec backups (key_backup.rs); HPKE-wrapped backups (hpke_key_backup.rs); keyring-backed secret store; egress guard [SRC]
- Observability: tracing (JSON logs), OpenTelemetry OTLP, Prometheus metrics [SRC]
- Sync semantics: REQ and EOSE; thread windows (39006, 39007) and channel windows (ARCHITECTURE.md section 5; NIP-CW) [DOC]

BUZZ_* environment families (names extracted with a regex, no values read):
- BUZZ_AGENT_* (agent runtime: rounds, handoffs, tokens, timeouts, prompt caching, thinking effort, permission timeout, MCP restart)
- BUZZ_ACP_* (harness: respond-to, subscribe, session policy, heartbeat, idle and turn limits, permission mode, memory toggles, base prompt, team instructions, channels, kinds)
- BUZZ_RATE_LIMIT_* (human and agent tiers; GIF searches)
- BUZZ_PUSH_* (push gateway: APNs, App Attest, grants, token keys, quotas)
- BUZZ_GIT_* (repo limits, pack cache, hook HMAC, probes)
- BUZZ_MESH_* (mesh bind address, iroh relays, demo echo)
- BUZZ_S3_*, BUZZ_MEDIA_* (object storage, upload limits, upload records)
- BUZZ_NIP_FI_* (federated identity mode, issuers, communities, lifetime)
- BUZZ_DB_*, BUZZ_REDIS_* (pool sizes, timeouts)
- BUZZ_PARTITION_MANAGER_* (partition creation and advance)
- BUZZ_TTS_*, BUZZ_STT_* (voice threads, streaming)
- BUZZ_USAGE_METRICS_*, BUZZ_STORAGE_* (metrics intervals, sweeps)
- BUZZ_ADMIN_* (admin host, token, web directory)
- BUZZ_UPDATER_* (desktop update endpoint and key); BUZZ_BUILD_*, BUZZ_DESKTOP_BUILD_* (baked build environment)
- BUZZ_FEATURE_FLAG_* (feature flag relay)
- BUZZ_V1_* (buzz v1 API switch and retention)
- BUZZ_CLI_* (CLI verb groups)

---

## 6. Blockers, limits and conflicts

Blockers and limits:
- Read-only pass. No builds, tests or app launches were run. REA was not used (open-source source).
- The repo-wide keyword census timed out (Windows file scans past 120 s). I stopped it and used targeted greps per taxonomy item. Items tagged INF with conf med may have missed occurrences.
- Secret-bearing or key-material files were not read: desktop/.env.e2e; .env.example (only BUZZ_* names were pulled by regex, no values printed); desktop/src-tauri/testdata/hpke_nsec_backup_v1.json (key-backup fixture). Also not read: .intersect/sadscan.yaml and .release/desktop-candidate.json. Secret-bearing file skipped.
- No credential, session, cookie, local-storage or per-user data stores were touched.
- Partial reads: ARCHITECTURE.md (sections 2, 3 headings, 9, huddle); AGENTS.md (headings, product contract, deep links, ecosystem); CHANGELOG.md (first 14 lines only); VISION docs (bullet lines and headings only); desktop command bodies (names only); relay handlers (names only).
- Mobile: pubspec and folder layout only, plus names in age_gate and animated_avatar_capture. web/ and admin-web/: manifests and folder listings only.
- NOSTR.md interop claims were not checked against running code.
- Counts include test files unless stated. Justfile count (about 107) and relay route counts are grep-based approximations.

Doc-versus-code conflicts (code is ground truth):
- ARCHITECTURE.md section 9 item 2 says no rate limiting is implemented. The code has a Redis fixed-window limiter (crates/buzz-pubsub/src/rate_limiter.rs) and a relay admission path (crates/buzz-relay/src/admission.rs). Deep-dive should confirm where it is wired.
- ARCHITECTURE.md section 9 items 5 and 6 (approval gates not wired; send_dm and set_channel_topic stubbed). The NotImplemented error variant still exists, so these may be current or may have changed. Verify in crates/buzz-workflow/src/executor.rs and action_sink.rs.
- ARCHITECTURE.md says ALL_KINDS has 127 entries; kind.rs has 122 kind constants. Likely drift.
- NIP-AB pairing is referenced in code and commands but has no docs/nips/NIP-AB.md.
- NOSTR.md lists kind 9009 (create invite) as a no-op; real invites go through the REST /api/invites routes. Different paths.
- VISION.md says the desktop supports all seven surfaces, but Pulse, Projects, Forum and Workflows are preview-gated in preview-features.json.
- Naming drift: Sprout versus Buzz across Cargo metadata, dev skills, ecosystem table and some docs.
- LaunchDarkly is documented only in crates/buzz-feature-flags/README.md (no VISION or ARCHITECTURE mention). The OSS source shows no production flag keys beyond db.events.query-version; other flag names are test fixtures. Not a conflict, but a gap in documentation.

Notable absences (for the deep-dive to confirm or reject): agent browser or computer-use capability; screen share and video calls; poll and scheduled-send UI; saved-messages UI; forwarding; do-not-disturb; SSO, SAML and SCIM; DLP; data residency; i18n; UnifiedPush and FCM delivery; OpenAPI spec; app directory and tabs.

Notable items worth a deep look: per-channel agent sessions and respond-to modes; agent cards and snapshots; Bestie (internal) and Nest; hash-chain audit; workflow YAML with traces; NIP-34 and NIP-MP projects; huddles where agents speak (local STT and TTS); relay-gated mesh compute; NIP-AO observer frames and NIP-AM metrics; App Attest in the push gateway.
