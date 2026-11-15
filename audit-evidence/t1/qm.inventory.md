# QM inventory (t1 scout pass, read-only)

Scope: C:/dev/qm (Git-Bash path /c/dev/qm). Purpose: input for the deep-dive "Space" audit (Teams + Slack + personal agents + buzz/qm inspiration). Breadth first; pointers are meant to be verified by the next agent.

Snapshot: branch main, HEAD 1e975255 (2026-10-12, "Harden the spend export token and row order (#2211)"). 894 commits in total; 821 commits since 2026-08-13, the date of the Verevon QM comparison baseline. [SRC git log, read-only]

Provenance legend (every bullet carries one tag):
- SRC = read directly in source or config (path given; line numbers where useful).
- DOC = documentation (repo docs, README, SPEC, SECURITY, desktop README) or a git commit subject (cited as DOC git:<hash>). Subjects state intent; they were not all verified in code.
- INF = my inference; the basis is stated.
- NEG = negative result from a grep. Read it as "not found in src, plugins, desktop, cli", not as proof of absence.
- REA = not used (no shipped artifact was analyzed; all evidence is source).

Confidence is high unless marked med or low.

## 1. Identity

- Product: "QM", tagline "A multiplayer agent harness for work. In Slack and on the web." Run in the operator's own cloud with the operator's own models and keys. Employees get isolated per-person workspaces and can also work with the agent in channels, group DMs and projects. Each person and each room has its own scoped memory, files, keychain view, permissions, crons, web apps and durable sandbox. [DOC README.md]
- Design north stars (SPEC): Unhobble; Free the brain (state in Postgres, agent can query it); Fast is the best feature; Agent UX = human UX; Delete before you add. [DOC docs/SPEC.md]
- Packages:
  - Root package "qm" 0.1.0, private, MIT. Description: "Headless core for the shared org agent (managed-agents architecture)". [SRC package.json]
  - CLI "@yc-software/qm" 0.1.6, bin "qm" -> dist/bin/qm.js, zero runtime dependencies. [SRC cli/package.json]
  - Desktop "qm-desktop" 0.1.0 (Electron). [SRC desktop/package.json]
  - Plugins, all 0.1.0: qm-admin-plugin, qm-auth-plugin, qm-plugin-chassis, qm-portal-plugin, qm-web-ui-plugin. plugins/onboarding has a skill only, no package.json. [SRC plugins/*/package.json]
- Runtime: Node >=24.15.0 and npm >=11.10.0 (engines). TypeScript runs directly on Node with no build step for core. [SRC package.json; .node-version]
- Language and frameworks:
  - TypeScript, ESM ("type":"module"). [SRC]
  - HTTP: Fastify ^5.12.5 (core API). [SRC package.json]
  - Slack: @slack/bolt ^4.7.3, @slack/socket-mode ^2.0.7, @slack/web-api ^7.19.0. [SRC]
  - Persistence and queue: pg ^8.13.1 and pg-boss ^12.26.1 (cron and job queue). In-memory stores are the default when DATABASE_URL is unset. [SRC package.json; src/config.ts]
  - Web UI and admin: Lit components built with Vite. README: "the web UI builds with Vite and renders with Lit". [DOC README.md; SRC plugins/web-ui/package.json]
  - Desktop: Electron 44.4.3, @electron/packager 20.3.0. [SRC desktop/package.json]
  - Schemas: zod 4.4.3 and typebox ^1.1.38. [SRC package.json]
  - Lint and format: oxlint ^1.74, eslint ^10.4.1 with typescript-eslint, prettier 3.9.1, knip ^6.26, husky 9.1.7, lint-staged 17. [SRC package.json]
  - Tests: node:test with --experimental-test-module-mocks. 712 root test files in test/*.test.ts; 1,107 test files across test/, plugins/, cli/ and desktop/. [SRC package.json scripts; file count]
- Vendored code: @earendil-works/pi-ai (release tarball qm.1, 0.82.0) and @earendil-works/pi-coding-agent (qm-security.6), both fetched from GitHub release assets. A vendor/pi-ai/ directory is also in the tree. [SRC package.json; vendor/]
- Dependencies grouped by capability (names are feature signals):
  - Agent harnesses: @anthropic-ai/claude-agent-sdk 0.3.280; @openai/codex 0.156.1 (bundled Codex); @opencode-ai/sdk, @opencode-ai/plugin and opencode-ai 1.18.31; the two Pi packages above. [SRC]
  - Model and tokens: @anthropic-ai/tokenizer ^0.0.4. [SRC]
  - Sandbox and compute SDKs: @fly/sprites 0.2.3 (Sprites), e2b ^2.51.0 (E2B), modal ^0.10.1 (Modal), porter-sandbox ^0.1.41 (Porter), @superserve/sdk ^0.8.3 (Superserve). Agent37, smolmachines and DigitalOcean Managed Agents use plain REST clients. [SRC package.json; src/sandbox/*]
  - AWS: @aws-sdk/client-ecs 3.1096.0, client-s3, client-secrets-manager, client-sts, credential-provider-node, s3-request-presigner; @smithy/signature-v4; @aws-crypto/sha256-js. [SRC]
  - Networking and egress: socks-proxy-agent 10.1.0; ws ^8.21.1; @grpc/grpc-js ^1.14.5 with @grpc/proto-loader ^0.8.1 (call sites not traced). [SRC]
  - Auth and crypto: jose ^6.2.3. [SRC]
  - Scheduling: croner ^10.0.1 and cronstrue 3.29.0 (cron to human text). [SRC]
  - Documents and archives: officeparser ^8.0.0 (document text extraction), tar-stream ^3.2.0. [SRC]
  - Caching and misc: lru-cache ^11.5.2; emoji-datasource ^16.0.0 (emoji picker and shortcode completion). [SRC]
  - Observability: @sentry/node 10.75.0 (backend), @sentry/browser and posthog-js (web-ui). [SRC]
  - Dev only: @modelcontextprotocol/sdk 1.31.0; esbuild 0.28.1; devbar.sh 1.1.0 (app devbar); typescript ^5.8.2; "@yc-software/qm": "file:./cli". [SRC devDependencies]
  - Security pins in overrides: hono 4.13.9, @hono/node-server 2.0.10, undici 8.10.2, protobufjs 7.6.5, ip-address 10.7.2, fast-uri, brace-expansion ^5.0.12. [SRC package.json overrides]
- web-ui plugin dependencies: CodeMirror 6 (state, view, language, search, language-data), @lezer/highlight, @earendil-works/pi-agent-core, pi-ai and pi-web-ui, @mariozechner/mini-lit, @tanstack/ai and ai-client, lit, lucide, marked, mermaid, morphdom, papaparse, dompurify, jszip, date-fns with @date-fns/tz, dockview-core (split panes), posthog-js, @sentry/browser. [SRC plugins/web-ui/package.json]
- Admin plugin: lit, esbuild, @sentry/node. Auth plugin: jose, @sentry/node. Portal plugin: jose, lru-cache, @sentry/node. [SRC]
- Architecture (as documented): Postgres holds sessions, memory and the queue. A headless core (API, identity, policy, scheduler) runs the agent loop (Pi, OpenCode, Claude Code, Codex) against a per-scope sandbox. The web UI and admin share one service; the portal and an optional built-in auth broker share another; both call core over HTTP. Slack is an optional in-process plugin that core starts and supervises. [DOC README.md; docs/combined-services.md]
- Deployment contract: a deployment directory with qm.config.jsonc is validated and deployed by the qm CLI. Deployment layers live under deploy/layers/<org>/. [DOC README.md; cli/README.md; docs/deploy-directory.md]
- Security posture (as documented): single-organization assumption, not a hardened multi-tenant boundary. Security postures strict, auto, dangerous. Sharing postures isolated, open. [DOC SECURITY.md; README.md]

## 2. Directory and module map

Top level of C:/dev/qm (purpose):
- src/ (552 TS/MJS files, about 138K lines): headless core and all subsystems in section 2b. [SRC]
- plugins/ (6 packages): web-ui (chat and workspace UI, 141 src files); admin (Lit admin console); portal (sign-in, desktop login, trusted OIDC entry, app host proxy); auth (built-in auth broker: password, email codes, sessions); chassis (shared plugin-to-core plumbing: signed core client, source-auth signer, router, branding, claims); onboarding (one skill). [SRC]
- cli/ (npm @yc-software/qm: 22 commands plus help and version, templates for AWS, Fly, Slack manifests and the deployment skill). [SRC]
- desktop/ (Electron client for an existing QM web deployment). [SRC; DOC desktop/README.md]
- deploy/ (deployment templates: Dockerfiles and fly.toml for admin, auth, core, portal, web-ui; egress-proxy (Envoy) and egress-proxy-pub; connector-sdk build; e2b and do-managed-agents templates; Helm chart; layers README; stacks examples; porter-runner). [SRC]
- aws/microvm-agent/ (Dockerfile and agent.mjs for AWS microVM sandboxes); fly/ (Dockerfile, README, tools/x-api script); superserve/templates/ (qm-agent template and verify script); porter/ (Dockerfile, apps/*.yaml for core, portal, web-ui, egress-proxy; legacy/); local/Dockerfile (local Docker sandbox image). [SRC]
- skills-seed/ (25 seed skills, section 3.6). [SRC]
- docs/ (34 Markdown docs including SPEC.md, qa/ runbooks, demos/credential-approvals.html, images/). [SRC]
- adrs/ (two ADR-style notes: an Agent37 sandbox backend proposal and a refused-agent-computer message). [SRC]
- scripts/ (dev-instance tooling; smoke tests per sandbox and provider; migrations for principals and transcript tape; backfills; AWS ECS blue-green and RDS restore drills; memory bench; Wall-of-shame check; reuse-ci). [SRC]
- test/ (712 root tests; e2e/ 7; live-slack/ 21 for live Slack E2E and gallery screenshots; integration/; memory-bench/; support/; fixtures/). [SRC]
- .github/workflows: cicd.yml (jobs: reuse, core-typecheck, core-tests, core, cli, coauthor-trailers, dependency-audit, lint, core-postgres, admin-plugin, web-ui-plugin, auth-plugin, portal-plugin, certify, notify-deployments); wall-of-shame.yml ("Wall of shame review"); release.yml ("Release"); release-package.yml ("Build signed QM images"); publish-cli.yml ("Publish the QM CLI"); pgbouncer.yml. [SRC]
- .claude/skills (dev-instance, update-qm, upstream-pr) and .codex/skills (deploy-qm, dev-instance, update-qm, upstream-pr): developer-agent skills for maintaining QM, not product skills. [SRC]

### 2b. src/ modules (file count; approximate lines; purpose)

- src/api (96 files, ~27.8K lines): HTTP surface. http.ts, app.ts, contract.ts, deps.ts, control-service.ts, credential-broker.ts, git-http-broker.ts, artifact-share.ts, capability-destination.ts, agent-api-catalog.ts (962 lines; the agent self-API catalog). routes/ holds 47 modules: admin/ (artifacts, auto-flagger-test, custom-providers, files, mcp-servers, memory, model-providers, model-registry, observability, origins, principal-links, sandbox, scope-config, security, sessions, skill-packs, slack-installation, and more), admin-resources, auth-broker, background-work, blobs, browser-model, composio, connectors, context, context-policy, credentials, crons, deployment-layer, deployments, directory, egress-audit, emoji, environments, file-uploads, inbox, keychain, loop-ingress, loop-item-events, loop-items, loops, memory-access, pins, projects, reach, route, run-events, search, secret-drop, session-sharing, session-state, shared, skill-packs, slack-events, suggested-activities, surface-cache, surface, swarms, turns, user-model-auth, webhooks. 307 distinct "/v1/..." path literals in src/api. [SRC]
- src/harness (28 files, ~16.9K lines): agent turn drivers and the tool catalog. agent-tools.ts (4.6K lines, tool catalog), harness.ts, harness-router.ts, pi-harness.ts, codex-harness.ts, codex-app-server.ts, codex-auth*.ts, claude-harness.ts, opencode-harness.ts, opencode-plugin.ts, mock-harness.ts, goal.ts, grind.ts, context-compaction.ts, history-summary.ts, replay.ts, tape-fold.ts, tape-projection.ts, runtime-control.ts, runtime-recovery.ts, runtime-types.ts, redact-secrets.ts, provider-error.ts, document-inputs.ts, context-snapshot.ts. [SRC]
- src/sandbox (35 files, ~11.8K lines): agent-computer contract and providers (section 3.7). [SRC]
- src/core (29 files, ~9.6K lines): orchestrator.ts (turn execution), orchestrator/ (attach-tool, compaction, external-tools, lease-keepalive, prompt-blocks, sandboxes, security-screen, surface-tools, turn-helpers, types), approval-store.ts, approval-id.ts, attachments.ts, document-inputs.ts, document-text-worker.ts, environment-facts.ts, failure-copy.ts, gateway-context.ts, image-downscale.ts, individual-auth-routing.ts, message-revisions.ts, session-title.ts, turn-error.ts, turn-options.ts, turn-origin.ts, turn-outcome.ts, turn-resume.ts, wake-envelope.ts. [SRC]
- src/slack (44 files, ~10.1K lines): Slack surface (Bolt events, turn flow, approval cards, deliveries, acks, mirror, forwards, external access). Section 3.1 and 3.6. [SRC]
- src/loops (23 files, ~5.2K lines): Loops and the Inbox (durable work queues; Gmail and Slack sources; triage; ship gate; governor; ledgers; outputs; Gmail Pub/Sub push). [SRC]
- src/memory (24 files, ~3.1K lines): notebook.ts (line grammar), memory-service and Postgres variant, memorable/ (procedural memory), strategies/ (agent-only, consolidation, per-turn, scratch-promote), provider-router, mcp-memory-provider, disclosure, context-boundary, classification, recall-delta, records, policy, bench. [SRC]
- src/model (18 files, ~2.9K lines): model catalog, gateway catalog and models, model gateway, custom providers and store, model overlays (fast mode, effort), model verification, provider endpoints, pi-models, browser-model and browser-completion, subscription-oauth (Claude and ChatGPT subscription login), codex-device-login, model and user-model credential stores. [SRC]
- src/resolution (15 files, ~2.5K lines): resolution-service.ts (the scope resolver), config-store, context-files, context-filter, egress-policy, sharing-access, sharing-posture, scope-membership, scope-reach, publish-audience, audience-floor, branding, external-slack, prompt-vars, turn-context. [SRC]
- src/runs (21 files, ~2.7K lines): run store (memory and Postgres), worker and worker-main, reaper, drain, instance-registry, background-controller and background-ownership, run-signal-store, run-activity-store, run-stream-events, turn-stream, tool-ledger, session-state-bus, retry-delay. [SRC]
- src/sessions (9 files, ~5.4K lines): session-store (memory and Postgres), session-mailbox (inter-session messages), session-syscalls, session-share, session-status, entry-search, history-search. [SRC]
- src/cron (7 files, ~1.7K lines): scheduler, job-queue (pg-boss), cron-store, fire-store (deterministic fire keys), schedule (croner), runtime, authority. [SRC]
- src/swarms (4 files, ~1.1K lines): swarm-service, swarm-store, swarm-settings, swarm-fence. [SRC]
- src/teams (1 file): teams.ts (team store, teamsForTurn, scopeTeamsToTurn). [SRC]
- src/projects (1 file): project-store.ts (versioned project roster). [SRC]
- src/acl (3 files): acl-store, postgres-grant-store, resource-ref. [SRC]
- src/admin (17 files): admin-service, admin-grant-store, error-log, metrics-sink, retention, audit, egress and credential usage sinks (memory and Postgres), invite-email, users, attribution, scoped-event-sink. [SRC]
- src/auth (7 files): capability-token, signed-token, source-auth (HMAC-signed surface requests), broker-sessions, portal-identity, replay-dedupe, aws-role-broker. [SRC]
- src/audit (1 file): audit-log.ts. [SRC]
- src/classify (1 file): scope-classifier.ts. [SRC]
- src/connectors (8 files, ~1.5K lines): oauth.ts, oauth-flow-store, connector-client-store, consent-link, browser-session-store, background-exec-broker, emoji-upload-service, gmail-sent. [SRC]
- src/credentials (10 files, ~2.6K lines): keychain, keychain-approval, connector-status, connector-token, execute-files, harness-auth-env, secret-drop, secret-source, paths, resident-paths. [SRC]
- src/delivery (4 files): delivery-store (memory and Postgres), run-result-delivery, web-transcript-delivery. [SRC]
- src/deploy (23 files, ~4.7K lines): deploy-service, deploy-store, deploy-git-store, deploy-provider and providers (aws, docker, fly, porter, shared), Fly tunnel and peer claims, page-views, share-notice, viewer-session, app-shell, app-annotate, embed-ancestors, email-access, access-token, deployment-credentials; vendor/ (devbar assets). [SRC]
- src/deployment (5 files): deployment-layer and store, load-layer, postdeploy-smoke, secret-schema. [SRC]
- src/directory (4 files): directory-store (memory and Postgres), person, visibility. [SRC]
- src/environments (1 file): environment-store.ts. [SRC]
- src/files (5 files): file-artifact-store (SHA-addressed), durable-byte-store, file-upload-store, direct-file-upload, Postgres variants. [SRC]
- src/identity (3 files): identity-service, principal-links, external-members. [SRC]
- src/idempotency (1 file): idempotency-store.ts. [SRC]
- src/mcp (3 files): mcp-client, mcp-server-store, mcp-tool-service. [SRC]
- src/monitors (3 files): monitor-broker, monitor-poller, monitor-store (durable watches). [SRC]
- src/onboarding (1 file): onboarding.ts. [SRC]
- src/persistence (9 files): pg-pool, postgres-listener, postgres-notify-bus, leader-lease, advisory-lock, durable-map, blob-transfer, s3, like. [SRC]
- src/playgrounds (1 file): playground.ts (HTML playground artifacts with validatePlaygroundHtml). [SRC]
- src/policy (1 file): command-policy.ts (911 lines; approval rules and hard denials). [SRC]
- src/processes (3 files): process-registry, process-reaper, reconcile (background processes). [SRC]
- src/ratelimit (4 files): rate-limiter and budget, memory and Postgres variants. [SRC]
- src/reach (1 file): reach.ts. [SRC]
- src/search (3 files): core-search, resource-search, backends. [SRC]
- src/security (3 files): security-posture, security-screener, secret-masking. [SRC]
- src/skills (12 files): skill-store, skill-bundle-store, skill-pack-store, pack-fetcher, ingest, materialize, normalize, seed, skill-sync-engine, skill-collision, skill-name, frontmatter. [SRC]
- src/suggestions (1 file): activities.ts (suggested activities). [SRC]
- src/surface-cache (6 files): surface-cache, ambient-judge, ambient-judgment-store, channel-policy-store, ack-emoji-pick-store, types. [SRC]
- src/surfaces (5 files): slack-installation, slack-managed, slack-manifest, slack-runtime, ui-state. [SRC]
- src/tasks (3 files): task-store (memory and Postgres). [SRC]
- src/tools (2 files): primitives.ts (tool context and typed results such as NeedsApproval and CommandDenied), admin.ts (adminOperations table that calls admin handlers). [SRC]
- src/triggers (6 files): trigger-store (recipient consent), run-trigger, provenance, consent-notice, edit-notice, keychain-ask. [SRC]
- src/util (22 files): errors, async (keyed queue), sweeper, crypto, http-proxy, network, safe-regex, shell, text, tokens, time, paths, event-bus, product-analytics, scope-storage-key, admitted-work, conversation-links, admin-links. [SRC]
- src/wake (3 files): wake, engaged-registry, sweep. [SRC]
- src/webhooks (3 files): webhook-receiver, webhook-store, verifiers. [SRC]
- src/workspace (1 file): workspace-store.ts. [SRC]
- Top-level src files: config.ts (1,733 lines; all env parsing and validation), feature-flags.ts (9 flags), index.ts (entry), wiring.ts (composition root), types.ts (scope ids and kinds), egress-authz-main.ts (egress authorization entry), migrate-main.ts (migration entry), instrument.ts (Sentry). [SRC]

### 2c. plugins and cli, in one line each

- plugins/web-ui (141 src files + server/ + public/ + shared.html): main chat and workspace UI. Key modules: chat.ts (createChatSurface), composer.ts and composer-loadout.ts, shell.ts, split-layout.ts and split.ts (dockview), browse.ts, inbox.ts and inbox-chat.ts, loops.ts, crons.ts, webhooks.ts, skills.ts, memory.ts, contexts.ts, connectors.ts, keychain-state.ts, deploys.ts and deploy-view.ts (apps), files.ts, file-viewer.ts, file-editor.ts, sent-mail.ts, calendar.ts, search.ts and search-resources.ts (Cmd+K), model-picker.ts, emoji-picker.ts, mermaid-block.ts, app-annotations.ts, app-devbar.ts, onboarding-*.ts, welcome-ideas.ts, suggested-activities.ts, slack-setup.ts, slack-account.ts, theme-import.ts, deep-link.ts, density.ts, subagent-activity.ts, swarm-strip.ts, goal-strip.ts, playground.ts. [SRC]
- plugins/admin (Lit admin console): ui/ holds activity, admin (shell), artifacts and artifacts-skills, design, governance and governance-state, history, integrations and integrations-state, keychain, metrics, onboarding, settings (branding, credentials, flags, providers, setting-controls), slack-activity, spend and spend-chart, transcript, user-detail, users. About 30 test files. [SRC]
- plugins/portal: admin-login, desktop-login, invite-login, oidc (trusted OIDC), proxy, session, trusted-admin, trusted-entry, cors. Serves the isolated app host proxy. [SRC]
- plugins/auth: config, email (SMTP and Resend), hash-password, password, pages, sessions, tokens, keys; the built-in password and email-code broker. [SRC]
- plugins/chassis: branding, claims, core-client, desktop-browser, env, error-reporting, errors, external-members, failure-copy, http, portal-identity, principal-allowlist, router, security-quarantine, source-auth-sign, suggested-activities, timing. [SRC]
- plugins/onboarding: skills/onboarding/SKILL.md. [SRC]
- cli (@yc-software/qm, 22 commands plus help and version): init, setup, dev, check, config, slack, outputs, admin-login, conformance, infra, layer, up, plan, migrate, sandbox, status, logs, down, rollback, doctor, proof, secrets (plus help and version). Backends: aws, fly, docker, dev-ci. Templates: aws terraform, fly tomls, slack-manifest.json, slack-sso-manifest.json, deployment SKILL.md and references. About 50 test files plus e2e/. [SRC cli/src/cli.ts dispatch lines 244-590; cli/templates; cli/test]
- desktop (Electron): main.mjs (windows and IPC), login.mjs, url.mjs, shortcuts.mjs, setup.html/js/css (instance chooser), preload.cjs (exposes "qm"), workspace-preload.cjs (exposes "qmDesktop"), package.mjs (packager), 7 test files including an Electron navigation test. [SRC]

## 3. Feature candidates, grouped by taxonomy

Each category lists what is present (with pointers), then what is absent (NEG). Commit-subject items are DOC git:<hash> or DOC (#PR). Where the source contradicts a commit subject, the source is the tie-breaker.

### 3.1 Messaging core

Present:
- Conversation kinds: dm, channel, group. [SRC src/types.ts:55, :69; src/triggers/run-trigger.ts:145-229]
- Scope kinds: personal, channel, team, org, group (ScopeId is "kind:ref"). [SRC src/types.ts:19-40]
- Slack Bolt event handlers: app_mention (src/slack/events.ts:129), message (:176), member_joined_channel (:356), member_left_channel (:394), reaction_added (:404), reaction_removed (:409), a loop of further event names (:387), and no-op assistant_thread_started and assistant_thread_context_changed (:401-402). [SRC]
- Slack thread-based conversations: DOC git:b875b0a7 ("Restore Slack thread-based conversations"). Unmentioned thread follow-ups that QM decides to answer are acknowledged: DOC git:6e8d7a80. Preserved reply context through DM routing: DOC git:d54c54c7 (#1271) and 15c9df56 (#1328).
- Edit and delete: message revisions store [SRC src/core/message-revisions.ts]; edit-notice trigger [SRC src/triggers/edit-notice.ts]; Slack deliveries for cut reaction, pin and delete are repeated with state checks: DOC git:12f1e556, git:93c64f51.
- Reactions: Slack reaction events [SRC src/slack/reactions.ts]; agent-chosen acknowledgment emoji [SRC src/slack/ack-emoji.ts; src/surface-cache/ack-emoji-pick-store.ts]; agent-settable session emoji status: DOC git:b29fda25 (#1475); Slack-style emoji shortcode completion in web chat: DOC git:747be10c [SRC plugins/web-ui/src/emoji-completion.ts; emoji-picker.ts].
- Mentions: app_mention routing and gating [SRC src/slack/message-gating.ts]; people-mention results in web [SRC plugins/web-ui/src/people-results.ts].
- Rich text: Markdown through marked with DOMPurify sanitization [SRC plugins/web-ui/src/markdown-sanitize.ts, message-markdown.ts, marked-dedupe.ts]; lazy highlight.js and KaTeX (src/lazy-hljs.ts, lazy-katex.ts, markdown-dollars.ts); Mermaid diagrams in chat and shared conversations: DOC git:e49134f3 [SRC plugins/web-ui/src/mermaid-block.ts]; Slack mrkdwn conversion [SRC src/slack/mrkdwn.ts]; Markdown, tables and text attachments in app preview: DOC git:2859da0c.
- Attachments: direct multipart uploads [SRC src/api/routes/file-uploads.ts; src/files/direct-file-upload.ts; env FILES_DIRECT_UPLOADS_ENABLED in src/config.ts]; document inputs turned into text, including officeparser [SRC src/core/document-inputs.ts, document-text-worker.ts; DOC docs/document-inputs.md]; image downscaling [SRC src/core/image-downscale.ts]; compact image previews above bubbles: DOC git:03673db6; conversation documents sent directly as model inputs: DOC git:a60ea051; files and images accepted while steering a running task: DOC git:037d3940.
- Link previews: INF low. Only API and app-messaging "unfurl" hits (src/api/app-messaging.ts, src/api/app-types.ts); no user-facing link preview card found.
- Drafts: web drafts [SRC plugins/web-ui/src/drafts.ts]; inbox drafts unified with chat and "Send it as a prompt": DOC git:10933487 (#1078); drafted new chat from a ?q= link: DOC git:0d452428 (#1682).
- Pins: pins API [SRC src/api/routes/pins.ts; /v1/pins]; channel header pin [SRC plugins/web-ui/src/channel-header.ts; /v1/channel-header-pin]; condensed pinned prompt while stuck: DOC git:30ddd393; Slack pin deliveries: DOC git:12f1e556.
- Forwarding: forwarded message content [SRC src/slack/forwards.ts messageWithForwardedContent].
- Shared conversations: session sharing [SRC src/api/routes/session-sharing.ts; src/sessions/session-share.ts; plugins/web-ui/server/shared-session.ts, shared.html; /v1/share, /v1/shared-sessions/:token(/files/:fileId)].
- Forks: fork from the clicked message in forked conversations: DOC git:acc549c5 and git:485afd87; /v1/sessions/:id/fork [SRC].
- Composer: composer.ts, composer-loadout.ts, embedded-composer.ts; model, effort and speed pickers [SRC plugins/web-ui/src/model-picker.ts, model-options.ts]; compact composer as multiview default: DOC git:129f34b9.
- Transcript: paged by turns: DOC git:4d4e260b; thinking moved into the transcript: DOC git:bcf982bd; transcript-viewport.ts, timeline.ts [SRC].
- Message links to exact messages: DOC git:f94ad37e; message-link.ts [SRC].
- Slack history mirror and backfill: [SRC src/slack/mirror.ts, history.ts, history-rate-limit.ts; /v1/admin/slack-mirror]; DOC git:3cb05a20, git:c5472eba.
- Steering a running task: DOC git:46387a93 (steering messages shown where the model receives them).

Absent (NEG):
- Scheduled send of a message: grep for scheduledAt, sendAt and send_at hits only cron files (src/cron/*). Scheduled agent turns exist through crons (3.7).
- Saved items and bookmarks: only emoji data and API catalog text match.
- Polls: process-poll.ts is sandbox process polling (src/sandbox/process-poll.ts), not chat polls.
- Voice clips and audio messages: no getUserMedia or MediaRecorder hits. The desktop prototype denies microphone, camera and notification permissions (SRC desktop/main.mjs:157-158, a request handler returning false and a check handler returning false; DOC desktop/README.md).
- Read receipts: no per-message read-state UI found (unread appears only in src/loops/inbox-loop.ts, src/persistence/pg-pool.ts and agent-tools.ts; INF low).
- Typing and presence indicators: no user-visible typing or presence UI. The Slack loading indicator sits behind feature flag slack_loading_indicator [SRC src/feature-flags.ts].
- Session-level sidebar label "Sessions" removed: DOC git:d3526537.

### 3.2 Org and membership

Present:
- Scope model: personal, channel, team, org, group [SRC src/types.ts:19-40]; resolution through src/resolution/resolution-service.ts (coherent scope resolver, cited by the baseline as a QM strength).
- Teams: team scopes behind feature flag team_scopes (added 2026-10-11): DOC git:242436b9. Postgres team store, teamsForTurn and scopeTeamsToTurn [SRC src/teams/teams.ts].
- Projects: versioned project roster [SRC src/projects/project-store.ts]; /v1/projects and members routes [SRC src/api/routes/projects.ts]; Slack channel binding /v1/projects/:id/slack-channel; inherited project roster for managed project turns: DOC git:b5251c91.
- Directory: /v1/directory, /v1/directory/meta, /v1/directory/resolve, /v1/directory/channels/:channelId/members/:principalId [SRC src/api/routes/directory.ts; src/directory/*]; directory roster carried into scheduled channel and group turns: DOC git:9add29b1.
- Roles: org_admin role referenced in the skill registry doc [DOC docs/skill-registry.md]; admin grants and grant changes [SRC src/admin/admin-grant-store.ts; /v1/admin/grants]; human-only authority walls for grant changes, impersonation and approval decisions [DOC SECURITY.md; AGENTS.md].
- Impersonation and stop: /v1/admin/impersonate and /v1/admin/impersonate/stop [SRC src/api/routes/admin.ts; test/admin-impersonate.test.ts].
- External users and guests: /v1/admin/external-users(/:email) [SRC]; external members [SRC src/identity/external-members.ts]; external Slack workspaces and private continuation: DOC docs/external-slack.md; DOC git:cc95b9ef (opt-in external Slack isolation); SRC src/slack/external-access.ts, src/slack/private-continuation.ts, src/resolution/external-slack.ts.
- External app sharing: off unless an org admin enables flag external_app_sharing: DOC git:27501bfe (#1781); SRC src/feature-flags.ts externalAppSharingAllowed.
- Principal links: one person with several sign-ins; /v1/principals/:id/canonical, /deactivate, /reactivate; principal-link admin routes [SRC src/identity/principal-links.ts; src/api/routes/admin/principal-links.ts]; DOC docs/principal-links.md; personal Slack accounts linked to existing web identities: DOC git:e29acebd.
- Grants and ACL: /v1/grants and /v1/grants/revoke [SRC]; ACL store, resource refs and Postgres grant store [SRC src/acl/*]; single-use credential grants restored: DOC git:b9c1e458.
- Channel membership: channel policy store [SRC src/surface-cache/channel-policy-store.ts]; membership wait before sandbox qualification: DOC git:3e5dc7a8.
- Sidebar and organization: pinned sessions [SRC /v1/pins]; suggested activities [SRC src/suggestions/activities.ts]; Browse tabs for skills, crons, webhooks, apps, files [SRC plugins/web-ui/src/browse.ts].
- Admin console: users, spend, governance, settings, keychain, integrations, Slack activity, artifacts and skills, transcripts, onboarding [SRC plugins/admin/ui/*].
- Invites: /v1/admin/users/invite [SRC src/admin/invite-email.ts]; teammate invitations and single-use sign-in links: DOC git:b81ab34d (#1513); /v1/auth/invitations/redeem [SRC].
- Onboarding state per user: /v1/admin/users/:principalId/onboarding [SRC]; onboarding skill [SRC plugins/onboarding/skills/onboarding/SKILL.md]; Slack bot onboarding checklist: DOC git:48894822; cohort welcome: DOC git:4c0ee215.
- Access reset: /v1/admin/users/:principalId/reset [SRC].
- Deactivate and reactivate principals: /v1/principals/:id/deactivate and /reactivate [SRC].
- Trusted OIDC sign-in: optional trusted OIDC portal entry (DOC git:e4b35024); administrators provisioned after opted-in OIDC sign-in (DOC git:56bbd92e); trusted sign-in switching with provider memory (DOC git:86264954). SRC plugins/portal/src/oidc.ts, trusted-admin.ts, trusted-entry.ts.
- Single organization: SECURITY.md states QM assumes one organization and is not a hardened multi-tenant boundary. ORG_ID env [DOC SECURITY.md; SRC src/config.ts].

Absent (NEG):
- SCIM provisioning: 0 hits for scim.
- Passkeys or WebAuthn: 0 files.
- Multi-tenant org isolation: not present by design (see above).

### 3.3 Calls and meetings

Absent (NEG):
- Audio or video calls, huddles, screen share, WebRTC, LiveKit, getDisplayMedia, getUserMedia, MediaRecorder: 0 hits across src, plugins and desktop.
- Recording, meeting transcripts, captions, meeting chat, AI meeting recap: none found. Closest are transcript views of chat sessions [SRC plugins/admin/ui/transcript.ts] and the morning digest skill [DOC skills-seed/morning-digest/SKILL.md].
- Meeting scheduling or booking: none found.

Present (adjacent):
- Calendar tab in the web UI: plugins/web-ui/src/calendar.ts (renderCalendar). DOC git:03a793fa "Add Email Archiving, Sent Mail, and a Calendar Tab (#1324)". Calendar access is through Google Calendar OAuth in the google-workspace skill [DOC skills-seed/google-workspace/SKILL.md].
- Whiteboard or canvas: a "ui_canvas" feature with canvas and UI-state skills was added (DOC git:19fdb8d0, 2026-10-09) and then reverted (DOC git:f5e37f63, 2026-10-10). Treat as not shipped.

### 3.4 Search and discovery

Present:
- Global search: /v1/search [SRC src/api/routes/search.ts; src/search/core-search.ts; src/search/backends.ts].
- Cmd+K resource search over skills, crons, apps and projects: /v1/resources/search; src/search/resource-search.ts; plugins/web-ui/src/search-resources.ts, search.ts. DOC git:80b87a19 (#1281).
- Conversation search: /v1/sessions/search [SRC src/sessions/entry-search.ts, history-search.ts]; indexed conversation search exposed through the sessions tool: DOC git:ada7af76 (#2204). Materialization barriers removed from entry search: DOC git:1ea3e28b.
- Memory recall: per-turn recall; recall delta and once-per-context recall [SRC src/memory/recall-delta.ts]; DOC git:ca47f5c8 (#1478). SPEC describes the default recall as vector search [DOC docs/SPEC.md].
- People search: /v1/directory/resolve and people-results.ts [SRC].
- Email search: Gmail search through the google-workspace skill [DOC skills-seed/google-workspace/SKILL.md]; saved email filters in the Inbox: DOC git:db4c3a19 (#1599).
- Browse filters and tabs: plugins/web-ui/src/browse.ts [SRC]. Admin session listing bounded and cheap: DOC git:df06097f (#2076).

Absent (NEG):
- A user-facing semantic search or AI answer UI with citations over documents. Nearest is memory recall (INF, med confidence).

### 3.5 Notifications and attention

Present:
- Delivery to the surface: run results and web transcripts [SRC src/delivery/run-result-delivery.ts, web-transcript-delivery.ts]; Slack DMs and threads are the main push channel (INF).
- Count badges on Crons and Apps only: DOC git:3c8ac28c.
- Digest: morning-digest skill delivers one short digest per cron owner [DOC skills-seed/morning-digest/SKILL.md]; suggested activities generated by scheduled sessions: DOC git:0d797e98 (#1164); the suggested-activities feature flag/env SUGGESTED_ACTIVITIES_ENABLED [SRC src/config.ts].
- Inbox attention: Loops and Inbox with Gmail and Slack sources [SRC src/loops/sources/gmail.ts, slack.ts, index.ts]; triage [SRC src/loops/triage.ts]; prioritization and consolidation (optional): DOC git:7d7bd51f (#1802); Slack sender avatars and source badge: DOC git:79843c8c.
- Ambient attention in channels: an LLM judge decides batches of unaddressed channel messages [SRC src/surface-cache/ambient-judge.ts judgeAmbientBatch, parseAmbientDecision; web policy in plugins/web-ui/src/ambient-policy.ts; /v1/admin/ambient-judgments].
- Mention routing and allow-lists: [SRC src/slack/allow-from.ts, message-gating.ts].
- Work progress: subagent activity cards, work duration, working dot [SRC plugins/web-ui/src/subagent-activity.ts, work-duration.ts, working-dot.ts]; subagent elapsed time ticks every second: DOC git:29c17206.

Absent (NEG):
- Browser or mobile push notifications: plugins contain 4 files referencing notifications; desktop denies the Notification permission (see 3.11). No push gateway found (INF).
- Mute, snooze or do-not-disturb: none (grep hits are CSS and devbar only).
- Unread counts per conversation: not confirmed (INF low).

### 3.6 Apps and extensibility

Present:
- Slack app: managed or custom Slack install and manifests [SRC src/surfaces/slack-managed.ts, slack-installation.ts, slack-manifest.ts; /v1/slack/managed/events, /v1/slack/managed/installation; /v1/admin/slack-installation]; manifest templates cli/templates/slack-manifest.json and slack-sso-manifest.json [SRC]; provider-managed Slack installation: DOC git:98ffa809.
- Slash commands: absent. No app.command handler found (NEG). The Slack surface uses events, message handlers and block actions (approvals.ts action regex hilo_; keychain-approvals.ts regex keychain_; deploy-access.ts regex deploy_access_; task-ack.ts add and remove actions) [SRC].
- Workflows and automation: crons [SRC src/cron/*; /v1/crons]; loops [SRC src/loops/*; /v1/loops]; triggers [SRC src/triggers/*]; monitors (durable watches) [SRC src/monitors/*]; background work [SRC /v1/background-work; src/runs/background-*]; DOC docs/loop-ingestion.md.
- Webhooks: inbound /v1/webhooks/incoming/:id with HMAC-SHA256 verification [SRC src/webhooks/verifiers.ts getVerifier; the only scheme literal is "hmac-sha256"; src/api/routes/webhooks.ts]; webhook message history and session links: DOC git:f617e01d (#1196); registration checks tightened: DOC git:8ff4a4e7 (#1922); webhook enable, disable and events routes [SRC]. Outbound webhooks: NEG.
- Gmail Pub/Sub push: verifyGmailPush and createGmailPushClient [SRC src/loops/gmail-push.ts]; env GMAIL_PUBSUB_TOPIC, GMAIL_PUBSUB_SERVICE_ACCOUNT, GMAIL_PUBSUB_AUDIENCE [SRC src/config.ts].
- Marketplace or app directory: absent (NEG). Skill packs imported from git repositories, admin-gated and audited: [SRC src/skills/pack-fetcher.ts, skill-pack-store.ts; /v1/admin/skill-packs*; DOC docs/skill-registry.md].
- APIs: REST /v1 with 307 distinct path literals across src/api [SRC]; the agent self-API catalog is served at /v1/apis [SRC src/api/agent-api-catalog.ts; agentApiMatches, renderAgentApis; test/agent-api-discovery.test.ts]; admin API under /v1/admin [SRC]; SSE event routes (see 3.9).
- SDKs and CLI: the qm CLI [SRC cli/]; the connector SDK, built into sandbox images before creation: DOC git:fc055480 (#1277) and git:a83118e9 (#1278) [SRC deploy/connector-sdk/build.mjs; src/sandbox/connector-sdk.ts].
- MCP: client to admin-managed MCP servers [SRC src/mcp/mcp-client.ts, mcp-server-store.ts, mcp-tool-service.ts; /v1/admin/mcp-servers]; MCP connectors with per-user credentials: DOC git:31f44cde (#1189). MCP server exposure of QM: NEG (no McpServer, registerTool or StreamableHTTPServerTransport in the server; @modelcontextprotocol/sdk is a dev dependency only).
- Client-executed tools: a turn can call tools that the caller executes: DOC git:bbd7ee4e (#1608); SRC src/core/orchestrator/external-tools.ts.
- Composio: /v1/composio/* (authorize, complete-auth, connections, execute, identity, slack, toolkits, tools); Composio keys kept in the backend: DOC git:97e44a22; Composio through ordinary credentials and skills: DOC git:79b0d272 (#1195). SRC src/api/routes/composio.ts; DOC docs/composio.md.
- Connect-apps OAuth consent: /v1/connectors/oauth/* (start, consent mint and redeem, revoke, status), /v1/connectors/token, /v1/connectors/catalog [SRC src/api/routes/connectors.ts; src/connectors/oauth.ts, consent-link.ts, oauth-flow-store.ts]. OAuth provider ids present in source: x, google, github, slack, notion, linear, dropbox (SRC src/connectors/oauth.ts; src/credentials/connector-status.ts). Gmail, Drive, Sheets, Docs, Slides, Calendar and Tasks sit under google (SRC; DOC skills-seed/google-drive-sheets, google-workspace). Other apps come through Composio. plugins/web-ui/src/connectors.ts also mentions stripe once (INF: catalog or example; not verified).
- Seed skills, 25: admin, browse, cloud-cli, composio, connect-apps, define-loop, dropbox, email-draft-in-voice, email-voice-profile, github-gitlab, google-drive-sheets, google-workspace, hand-off, interactive-login, linear, memory, miniapp, morning-digest, popular-web-designs, publish, request-access, send, slack-drafts, taste-skill, use-shared-credential [SRC skills-seed/*/SKILL.md]. Skill seeding at boot is deduplicated: DOC git:b06e8c95. Plugin-provided skill directories: PLUGIN_SKILLS_DIRS env [SRC src/config.ts].
- Playgrounds: agent-authored HTML playground artifacts [SRC src/playgrounds/playground.ts createPlaygroundArtifact, validatePlaygroundHtml; skills-seed/miniapp/SKILL.md].
- Custom emoji: /v1/emoji and /v1/admin/slack-emoji; emoji upload service [SRC src/connectors/emoji-upload-service.ts].
- Forms: credential drop forms /v1/keychain/drops/:id/form [SRC src/api/routes/secret-drop.ts; src/credentials/secret-drop.ts].
- X API helper: fly/tools/x-api, a bash wrapper. It uses an org-level bearer token by default, or the connected user's token with --as me. [SRC fly/tools/x-api]
- Custom model providers and gateway: [SRC src/model/custom-providers.ts; /v1/admin/custom-providers; src/model/model-gateway.ts; env MODEL_GATEWAY_URL]. See 3.7.
- Tabs and pinned apps: Browse tabs and pins [SRC browse.ts; /v1/pins].

### 3.7 Agents in the room

Present, identity and presence:
- Agent as a Slack bot user and as a web session; personal agents per person scope with isolated memory, files, keychain, crons, apps and sandbox. [DOC README.md; SRC src/types.ts]
- @-invocation: app_mention [SRC src/slack/events.ts:129]; thread follow-ups [DOC git:6e8d7a80]; ambient judgment of unaddressed channel messages (3.5).
- Ambient policy and per-channel bot ledger: [SRC src/surface-cache/channel-policy-store.ts BOT_* and ledger parsing; plugins/web-ui/src/ambient-policy.ts].

Present, work visibility and tool-call cards:
- Activity presentation and tool purposes; concise tool purposes and skill names in web activity: DOC git:437829c0. [SRC plugins/web-ui/src/activity-presentation.ts]
- Slack task cards for ongoing work (opt-in): DOC git:82342138 (#1655); [SRC src/slack/task-ack.ts]. Slack task completion ownership is a baseline "band-aid" item in SPEC.
- Nonzero command exits shown neutrally instead of Failed: DOC (#1648): nonzero command exits shown neutrally instead of Failed.
- Interrupted tool results marked as interrupted after shutdown: DOC git:32046dc5 (#2060).

Present, approvals and human-in-the-loop:
- Approval store and ids [SRC src/core/approval-store.ts, approval-id.ts]; routes /v1/approvals, /v1/approvals/pending, /v1/approvals/:id, /v1/sessions/:id/approvals [SRC].
- Slack approval cards and Run/Deny actions, persisted with atomic claims [SRC src/slack/approvals.ts (action regex hilo_), approval-cards.ts; SPEC wall of shame].
- Native web credential approvals: DOC git:f94ad37e (#1502); the approval card is the only way to approve credential use: DOC git:1906e4af (#1713); command-approval cards posted in the requesting DM thread: DOC git:09c074ce (#2110); gated call arguments shown in strict-posture approvals: DOC git:50beec77 (#1702).
- Security posture strict: every harness tool call pauses for human approval, except the two no-effect turn enders [DOC README.md]; command policy applies in every posture [DOC README.md; SRC src/policy/command-policy.ts].

Present, tool surface (agent-facing tools; SRC src/harness/agent-tools.ts, names are literal "name:" values):
- execute (three definitions at lines 876, 894, 918). Command launcher: DOC git:a40bc891 "Make execute the sole command launcher (#2205)".
- sandbox (932), skill (1069), read (1134), write (1168), publish (1200), memory (1314), history (1460), subagents (1529), sessions (1741), background (1932), cron (2280), webhook (2770), guidance (2902), share (3091), a dynamic surface tool named "slack" or the web surface name (3369), attach (3813), finish_silently (3844), goal tools create (3910), get (3984), update (4000), context (4218; compact mode "recent"), runtime (4239). Admin tool (agent-tools.ts admin.description near 3360; calls into src/tools/admin.ts adminOperations).
- Tool-purpose and unambiguous actions for session, background and guidance tools: DOC git:8c69f826 (#1649). Split sessions into subagents and web-only sessions tools: DOC git:1b699fb1 (#1656).

Present, goals and autonomy:
- Goal tool with create, get and update (agent-tools.ts 3910-4000) [SRC]; goal record, governor and continuation prompts [SRC src/harness/goal.ts: GoalRecord, governGoal, applyGovernorVerdict, goalContinuationPrompt, goalCapPrompt, goalPausedNote]; goal evidence files read into the goal context [SRC agent-tools.ts GOAL_EVIDENCE_FILE_CHARS].
- Governor steps back then pauses a stuck goal: DOC git:dc93931b (#2132). Only a human stop or an independent verifier can end a goal: DOC git:9143874b (#1715). Agents cannot pause their own goals: DOC git:54e04d1f (#1637). A person can resume a paused goal: DOC git:9a42ce20 (#2116). Goals preserved across compaction and worker recovery: DOC git:3b5403aa (#1621). Goals enforced for every harness: DOC git:01e8da46 (#1519). Goal status indicator and exact timed floors: DOC git:6147d8a9 (#1734).
- Grind budget meter: [SRC src/harness/grind.ts GrindBudget, GrindMeter, meterGrindCall, grindState]; cap prompt when a budget is reached [SRC goal.ts goalCapPrompt].

Present, harness and runtime (the "runtime" is harness + model + effort; DOC docs/SPEC.md):
- Harness ids: mock, pi, opencode, codex, claude [SRC src/config.ts error text; src/harness/harness-router.ts; mock-harness.ts; pi-harness.ts; opencode-harness.ts; codex-harness.ts; claude-harness.ts].
- Per-harness adapters: Pi uses vendored pi-ai and pi-coding-agent; Codex uses codex-app-server.ts (JSON-RPC style reply settling: DOC git:1e2a57e1); OpenCode uses opencode-plugin.ts (fast mode per request: DOC git:41ced005); Claude uses @anthropic-ai/claude-agent-sdk. Native effect tools are disabled for governed turns (DOC baseline; INF on exact mechanism).
- Harness security: HARNESS_SECURITY_POSTURE dangerous|auto|strict; HARNESS_SHARING_POSTURE isolated|open [SRC src/config.ts ~1090-1099].
- Recent harness/model changes: bundled Codex 0.156.1 without native goals: DOC git:4c5e7038 (#1788); Pi receives the ChatGPT subscription token as OAuth: DOC git:fe3cda05 (#1305).
- Refusal and unavailable-model fallback: DOC git:e33bee7a (#1743). SPEC lists qm#1743 as a band-aid (regex matching of usage-policy text).
- Typed provider errors replace regex matching in core: DOC git:1b2d07e9 (#2068).

Present, models:
- Model provider ids: openai, anthropic, openrouter, gateway, custom [SRC src/config.ts MODEL_PROVIDERS; src/model/provider-endpoints.ts]. Browser-backed inference: [SRC src/model/browser-model.ts, browser-completion.ts; /v1/browser-model/chat/completions]; browser inference follows the saved AI access: DOC git:d5768610 (#1722).
- Subscription login: Claude (startClaudeLogin, completeClaudeLogin) and ChatGPT (refreshChatGPTTokens) [SRC src/model/subscription-oauth.ts]; routes /v1/user-model-auth/* (account, api-key, chatgpt/start and poll, claude/start and complete, disconnect, status) [SRC]; Codex device login [SRC src/model/codex-device-login.ts]; user-level model credentials [SRC src/model/user-model-credential-store.ts].
- Custom providers: /v1/admin/custom-providers; Responses protocol for custom providers: DOC git:b52b1c17 (#1178); reasoning capability on custom models: DOC git:9627efc7 (#1810).
- Gateways: MODEL_GATEWAY_URL, MODEL_GATEWAY_API_KEY, MODEL_GATEWAY_API_KEY_HEADER, MODEL_GATEWAY_MODELS [SRC src/config.ts]; gateway model discovery without an environment allowlist: DOC git:8738eab4 (#1162); session-affinity headers for gateway models: DOC git:007624c7.
- Model catalog and overlays: model-catalog.ts (canonical model varieties, effort levels, speed multipliers, prices per DOC docs/SPEC.md); model-overlay.ts with fast mode [SRC]; model verification [SRC src/model/model-verification.ts; docs/admin-model-verification.md].
- Presets: eight saved model presets: DOC git:aa0956ae. Model picker simplified with compact presets: DOC git:9b854415 (#1132).
- Fast or Ultrafast tier: Astra Ultrafast speed switch: DOC git:6c09f179; GPT-6 Astra Ultrafast uses service_tier "ultrafast" via the Responses API at 6x cost: DOC docs/model-gateway.md (Astra Ultrafast section).
- Model additions per commit subjects (DOC, not verified in code individually): Claude Haiku 5.5 as helper and judge model: git:bca3b27f; Claude Sonnet 5.5 with adaptive thinking: git:0d59cb20; claude-opus-5-5 with preserved-thinking replay: git:a0b97066; GPT-6 Sol and GPT-6 Luna: git:8a9b1808; GPT-6.1 Sol: git:62f8d9e9; price table sync: git:091d130b.

Present, sandboxes (the agent computer):
- Backend enum (from src/config.ts error text): sprites, smolmachines, e2b, modal, porter, agent37, superserve, do-managed-agents, aws, local. [SRC]
- Implementations (src/sandbox/*): sprites-sandbox.ts (718 lines; Sprites SDK 0.2.3 with WebSocket exec and checkpoints: DOC git:cec1fade); smolmachines-sandbox.ts (627; SMOLMACHINES_* env); e2b-sandbox.ts with e2b-client.ts (649; E2B template and lifetime); modal-sandbox.ts with modal-client.ts and modal-image.ts (968); porter-sandbox.ts with porter-client.ts (477); agent37-sandbox.ts (637; the Agent37 REST backend, ADR adrs/agent37-sandbox-backend.md); superserve-sandbox.ts with superserve-client.ts (775; template qm-agent; DOC git:7df57367); managed-agents-sandbox.ts with managed-agents-client.ts and managed-agents-tunnel.ts (631; DigitalOcean Managed Agents, DO_AGENTS_* env; DOC git:51385cfd); aws-sandbox.ts (590) with aws-microvm-api.ts (AWS_SANDBOX_* env; ECS and microVM image under aws/microvm-agent/); local-sandbox.ts with docker-exec.ts (562; LOCAL_SANDBOX_* env; local/Dockerfile).
- Contract and routing: sandbox.ts (profile: persistence, processes, backup, egress), sandbox-routing.ts (SRC; baseline says it fails closed on a capability downgrade), sandbox-resources.ts (SRC; docs/sandbox-resources.md), process-poll.ts, exec-process-session.ts, exec-file-ops.ts, home-snapshot.ts, ro-layers.ts (read-only layers), layer-tool-install.ts, sandbox-env.ts, sandbox-resource-upgrade.ts, tar.ts. Retired legacy routing: DOC git:1ea79127 (#2019).
- Egress: fail-open warnings when egress proxy URLs are missing for sprites, e2b, do-managed-agents, smolmachines and porter [SRC src/config.ts ~1262-1301]. Egress proxies are built in deploy/egress-proxy (Envoy) and deploy/egress-proxy-pub.
- Recent sandbox changes: exclusive per-command machine lock removed: DOC git:c616b40f (#1748); shared computer paused only after the last turn releases it: DOC git:daeb2deb; AWS CLI v2 in E2B and Modal images: DOC git:e916da17 (#2168); retries of transient provider errors with request ids: DOC git:7832ccf3 (#1412); sandbox status probes exec on every sandbox: DOC git:3ca5bfdf (#1684); instance names capped at 63 characters: DOC git:9136bde7 (#2115).
- Fly is a deploy provider, not a sandbox backend (src/config.ts:79 deployProvider "docker"|"aws"|"fly"|"porter").

Present, memory (per scope):
- Notebook: append-only per-scope notebook with a fixed line grammar [SRC src/memory/notebook.ts; docs/memory-records.md]; memory tool [SRC agent-tools.ts 1314]; memory audience and provenance-aware disclosure: DOC git:58a9dc63 (#1636) [SRC src/memory/disclosure.ts, context-boundary.ts, classification.ts, policy.ts].
- Procedural memory (Memorable): [SRC src/memory/memorable/capture.ts, inject.ts, provider.ts, relay.ts, config.ts]; env MEMORY_CAPTURE, MEMORY_CAPTURE_MAX_TURNS, MEMORY_CAPTURE_QUIET_MS, MEMORY_RECALL, MEMORY_STRATEGY, MEMORY_CONSOLIDATE_AFTER, DEFAULT_CAPTURE_QUIET_MS [SRC src/config.ts].
- Strategies: agent-only, consolidation, per-turn, scratch-promote [SRC src/memory/strategies/*].
- External memory providers with routing by scope and an MCP memory provider; the BRAIN_* variables are retired: [SRC src/memory/provider-router.ts, provider-factory.ts, mcp-memory-provider.ts; DOC docs/memory-providers.md].
- Per-turn context trimmed (grouped memory deltas): DOC git:54c5f16a (#2163). Stable prompt prefixes for caching: DOC git:a61db71b (#1618); inventories in stable order: DOC git:3b1e3cf4 (#1690).

Present, skills (SRC src/skills/*, skills-seed/*):
- Skill store and bundles; skill packs from git with admin gating; skill sync poll (SKILL_SYNC_POLL_MS); signing secret SKILL_SIGNING_SECRET; seeds from SKILLS_SEED_DIR and SEED_SKILLS.
- Explicit skill tool replaces projection of a shared skills index: DOC git:98ac521e (#1432); skill contents materialized only on explicit access: DOC git:743b5c0d (#1325); published skill sources read without starting sandboxes: DOC git:dfdde0bc (#1280); shortlists ranked and native discovery: DOC git:6374e44c (#2210).
- Skill frontmatter parsing and collision handling [SRC src/skills/frontmatter.ts, skill-collision.ts, skill-name.ts].

Present, subagents, swarms and coordination:
- Subagents tool [SRC agent-tools.ts 1529]; subagent tree run cap raised from 10 to 50: DOC git:93b2c434 (#2169); idle parent woken when a subagent finishes: DOC git:6392c5c2 (#2170); subagent completion wakes coalesced until parents are idle: DOC git:68f7efa1 (#1480); subagents inherit parent capabilities: DOC git:81a08e00 (#2155); subagent delegated task shown in its transcript: DOC git:978aa7b9 (#2150).
- Persistent subagents with durable coordination and inline badges: DOC git:1bd53763 (#1287); feature flag persistent_subagents [SRC src/feature-flags.ts]; docs/persistent-subagent-sessions.md (lifecycle, parent changes and completion, storage and scope, swarms, feature flag).
- Swarms: durable agent pools, scoped messages and blank Modal workers [DOC docs/swarms.md; DOC README.md]; swarm sessions added: DOC git:e0b8686c (#1105); per-person gating, shared board and worker controls: DOC git:7df97144 (#1770); artificial maximums removed: DOC git:1ecb06c5 (#2141); deployments can disable swarms: DOC git:e32e9431 (#1347). SRC src/swarms/swarm-service.ts (createSwarmService), swarm-store.ts, swarm-settings.ts, swarm-fence.ts; /v1/swarm, /v1/sessions/:id/swarm, /v1/swarms routes; feature flag swarms; web swarm-strip.ts.
- Session mailbox: inter-session messages [SRC src/sessions/session-mailbox.ts createSessionMailbox]. Sessions tool actions [SRC agent-tools.ts 1741].
- Session-owned work continues while subagents run: DOC git:7c1a333f (#1689).

Present, crons, loops, triggers, watches, webhooks as agent work:
- Cron tool [SRC agent-tools.ts 2280]; crons with destination and runtime overrides: DOC git:5fecef03 (#1593) and org runtime defaults for crons and subagents: DOC git:f7718c33 (#1619); cron authority and owner approval for credentials: DOC git:570db440 (#1381); personal automation access to owner credentials restored: DOC git:4b2838e0 (#1594); cron fire keys and slot claims [SRC src/cron/fire-store.ts; src/cron/scheduler.ts]; session-tied crons shown as ongoing work: DOC git:4becece4 (#1711).
- Loops (durable work queues the agent works through): [SRC src/loops/*; /v1/loops/*]; define-loop skill [DOC skills-seed/define-loop/SKILL.md]; autopilot and fire routes [SRC /v1/loops/:id/autopilot, /v1/loops/:id/fire]; per-loop icons and custom logos: DOC git:6ae009eb (#1494), git:cb6e2d44 (#1496).
- Watches (monitors): durable process watches with cursor, heartbeat, expiry, destination and cancel [SRC src/monitors/monitor-broker.ts, monitor-poller.ts, monitor-store.ts; the baseline describes them as QM strengths].
- Triggers: recipient-consent-gated runs for group, channel and DM kinds, and poll surfaces [SRC src/triggers/trigger-store.ts, run-trigger.ts; /v1/triggers/:id/consent].
- Background work: /v1/background-work; ownership transfer between live deployment revisions: DOC (#1291): background ownership transferred between live deployment revisions; single owner record: DOC git:877e872c (#2057).

Present, permissions, secrets and cost:
- Keychain: per-user credentials, drops, grants, asks, approvals, overview [SRC src/credentials/keychain.ts; /v1/keychain/*]; keychain-ask trigger [SRC src/triggers/keychain-ask.ts].
- Credential usage and approvals per execution: DOC git:d42e4829 (#1566); masking of selected credential output [SRC src/security/secret-masking.ts; src/harness/redact-secrets.ts].
- Request-access and use-shared-credential skills [DOC skills-seed/request-access/SKILL.md; skills-seed/use-shared-credential/SKILL.md]; interactive login saved to the keychain [DOC skills-seed/interactive-login/SKILL.md].
- Capability tokens: [SRC src/auth/capability-token.ts; DOC docs/capability-tokens.md]; SANDBOX_CAPABILITY_TTL_HOURS (default 48h, or 0 or none for non-expiring) [SRC src/config.ts; DOC git:dc91d0d3 (#1747)].
- Budgets and cost: budget windows [SRC src/ratelimit/budget.ts; env BUDGET_USD_PER_WINDOW, ORG_BUDGET_USD_PER_WINDOW, BUDGET_WINDOW_MS]; spend dashboard with time-series breakdowns: DOC (#1614); spend reports from saved daily totals: DOC git:ed0c894d (#1641); spend export with a token: DOC git:ed11cb6c (#1798); hardened token and row order: DOC git:1e975255 (#2211); custom spend windows: DOC git:05a62c14 (#2213).

Present, human-to-agent hand-off:
- hand-off skill: decide whether to do the work yourself or ask the person with the needed login, files or memory [DOC skills-seed/hand-off/SKILL.md]; Slack DM hand-off: DOC git:fc54a60b (#2167).
- Credential approval handoff from a chat turn: DOC git:faee4364 (#2146) "Repair device login capture and explicit credential writeback" (SPEC notes a related deletion PR, qm#2146 per SPEC, status open).

Absent (NEG):
- Human-in-the-loop calls or meetings: see 3.3.
- Agent-to-human voice: none.

### 3.8 Collaborative artifacts

Present:
- Files: file-artifact store with SHA-addressed artifacts [SRC src/files/file-artifact-store.ts]; /v1/files and /v1/files/:id/content [SRC]; admin files routes [SRC src/api/routes/admin/files.ts]; durable file publication with direct protocol and recovery [DOC docs/files-publication.md]; recurring job checkpoints kept in durable files: DOC git:63674107 (#1694); working notes kept in Files: DOC git:770972b9 (#1994).
- Web apps ("publish"): publish tool [SRC agent-tools.ts 1200; skills-seed/publish/SKILL.md]; /v1/deployments with routes for always-on, archive, display-name, embed-ancestors, fetch, git-url, logs, name, redeploy, restore, rollback and share [SRC src/api/routes/deployments.ts]; public shares /v1/public-shares [SRC]; opt-in public app sharing: DOC git:0edd2377 (#1571); publishing stays private, share separately: DOC git:a153974f (#2026); General access menu with Restricted, org and "Anyone with the link": DOC git:75c83bc1 (#1783); external app sharing off by default: DOC git:27501bfe (#1781); app page views recorded: DOC git:29ba14c0 (#2027); apps can opt into framing by named origins: DOC git:f75cfd1c (#1503); publish audience parameter removed: DOC git:bafcf3f1 (#2130).
- App annotations: native chat attachments behind a per-user flag: DOC git:781e9c58; feature flag app_annotations [SRC src/feature-flags.ts; src/deploy/app-annotate.ts]; app devbar [SRC plugins/web-ui/src/app-devbar.ts; src/deploy/vendor/devbar*]; app text selection and editing [SRC plugins/web-ui/src/app-text-selection.ts, app-edit.ts].
- Deployment providers for apps (DEPLOY_PROVIDER docker|aws|fly|porter): Porter serves published apps in-cluster by default: DOC git:2fdc93c2 (#1679); durable shared Fly publishing with private scale-to-zero routing: DOC git:5a3a4dc3 (#1216); AWS isolated deployments on shared foundations: DOC git:518b68c5 (#1186); Porter uploads app files as one bundle: DOC git:fe383aae (#1724). SRC src/deploy/*-deploy-provider.ts; cli/src/backends/*.
- Playgrounds (HTML artifacts): [SRC src/playgrounds/playground.ts; skills-seed/miniapp/SKILL.md].
- Tasks: task store (memory and Postgres) [SRC src/tasks/*]; loop items with triage, actions, follow-up and images [SRC /v1/loops/:id/items*]; swarm shared board: DOC git:7df97144.
- Code and repos: git HTTP broker [SRC src/api/git-http-broker.ts]; git credential broker enforces published-app token binding: DOC git:87eae257 (#1794); deploy git store [SRC src/deploy/deploy-git-store.ts]; repository work through resident gh, glab and git [SRC skills-seed/github-gitlab/SKILL.md]; skill "send" makes a PR, waits for CI, runs independent review and merges only when gates pass [SRC skills-seed/send/SKILL.md].
- Projects: project store and Slack channel binding [SRC src/projects/project-store.ts].
- Review and approval flows: ship gate [SRC src/loops/ship-gate.ts]; ship grant store [SRC src/loops/ship-grant-store.ts]; success evaluation [SRC src/loops/success-evaluation.ts]; output decisions [SRC /v1/loops/:id/outputs/:outputId/decide].
- Knowledge: notebook, guidance and skills (see 3.7) [SRC agent-tools.ts guidance at 2902].
- Email artifacts: email drafts in the user's voice (drafts only) [DOC skills-seed/email-draft-in-voice/SKILL.md; email-voice-profile]; sent-mail view [SRC plugins/web-ui/src/sent-mail.ts; /v1/connectors/gmail/sent].
- Slack drafts: drafting in the user's Slack composer through per-user OAuth [DOC skills-seed/slack-drafts/SKILL.md].

Absent (NEG):
- Collaborative document or wiki editor: none. The file editor (plugins/web-ui/src/file-editor.ts) uses CodeMirror for files (INF).
- Whiteboard: none (canvas reverted, see 3.3).
- Kanban or board UI for tasks: none beyond the swarm shared board and loop items (grep "board" is dominated by "dashboard" and "keyboard").

### 3.9 Realtime and sync architecture

Present:
- Transport: HTTP JSON over Fastify [SRC src/api/http.ts, app.ts]. Server-Sent Events: run events (/v1/runs/:id/events, src/api/routes/run-events.ts), session state (/v1/session-state/events, session-state.ts), loop item events (/v1/loops/... loop-item-events.ts) [SRC]; MCP client SSE handling [SRC src/mcp/mcp-client.ts]. Run tool calls and results streamed as events: DOC git:d42cf2a8 (#1607), with the RunStreamEvent type and emitRunText [SRC src/runs/run-stream-events.ts].
- AG-UI: DOC git:d42cf2a8 claims AG-UI events. NEG: no "AG-UI" or "agui" string in src, plugins/web-ui or server (case-insensitive grep). Treat the AG-UI claim as unverified (INF; med confidence the events are an AG-UI-like envelope under another name).
- Slack transport: HTTP events mode when SLACK_EVENTS_MODE=http with SLACK_EVENTS_PORT; otherwise Socket Mode [SRC src/config.ts ~1492; src/slack/index.ts; @slack/socket-mode]. Default mode: INF (not confirmed).
- Postgres LISTEN/NOTIFY: postgres-listener.ts, postgres-notify-bus.ts [SRC src/persistence/]; run workers woken by PostgreSQL notifications: DOC git:bf8f3264 (#1255); one listener per process: DOC git:d32fd209 (#1185); DB-backed session state bus [SRC src/runs/postgres-session-state-bus.ts].
- Run durability: Postgres run store with leases, heartbeats, reaper, signal replay and a per-attempt tool ledger [SRC src/runs/postgres-run-store.ts, reaper.ts, run-signal-store.ts, tool-ledger.ts; DOC baseline]. Runs can be withdrawn and given input or signals [SRC /v1/runs/:id/input, /signal, /withdraw].
- Session tape: an append-only session log that replaces session_entries [DOC docs/session-tape-spec.md; SRC src/harness/tape-fold.ts, tape-projection.ts; env SESSION_TAPE_MODE; SPEC wall of shame: both stores remain]. Exact transcripts on the tape: DOC git:45be37ee (#1272).
- Queue for cron: pg-boss [SRC src/cron/job-queue.ts]; per-second tick removed: DOC git:b528b8ff (#2007).
- Coordination primitives: leader lease [SRC src/persistence/leader-lease.ts]; advisory lock [SRC src/persistence/advisory-lock.ts]; durable map [SRC src/persistence/durable-map.ts]; idempotency store [SRC src/idempotency/idempotency-store.ts]; replay dedupe for signed requests [SRC src/auth/replay-dedupe.ts]; swarm fence [SRC src/swarms/swarm-fence.ts].
- Blob transfer: presigned S3 transfers and multipart uploads [SRC src/persistence/blob-transfer.ts, s3.ts; /v1/blobs; /v1/files/uploads/:id/parts/:part; /v1/files/uploads/:id/complete].
- Recovery and resume: recovery of saved responses after polling timeouts: DOC git:4ac5b6c9 (#1640); older history loads during live turns: DOC git:db9693b3 (#1761); chat streaming and recovery runbook [DOC docs/qa/chat-streaming/README.md]; retry-safe tool calls re-run transparently when a deploy interrupts a turn: DOC git:a096a496 (#2055).
- Multi-instance and deploys: instance registry heartbeats only from worker instances: DOC git:c5f667e1 (#1318); drain and resume of background loops: DOC git:a8408096 (#1290); blue-green core cohorts: DOC git:c8e85156 (#2047); AWS ECS blue-green drill [SRC scripts/aws/ecs-blue-green-drill.ts]; PgBouncer compatibility [DOC docs/pgbouncer.md; .github/workflows/pgbouncer.yml].
- Secrets at rest: keychain logins are encrypted in the owner's keychain and refreshed centrally [DOC .env.example comments; docs/credential-refresh-handoff.md].

Absent (NEG):
- Push gateway for mobile: none (no mobile client in repo) (INF).
- Multi-device sync engine for the desktop app: none beyond a shared server session (INF).
- End-to-end encryption of messages or sessions: not found (INF, med confidence).
- NATS, Temporal or Nostr transports: none in QM (the baseline mentions Verevon's Temporal and NATS, not QM's).

### 3.10 Governance, security and compliance

Present:
- Audit: audit log module [SRC src/audit/audit-log.ts]; Postgres audit log [SRC src/admin/postgres-audit-log.ts]; /v1/admin/audit [SRC]; resource filter matches % and _ literally: DOC git:be9b3c46 (#1926). Baseline notes the log-and-drop behavior of postgres-audit-log.ts:53-66 as a QM weakness.
- Retention: retention module [SRC src/admin/retention.ts]; route /v1/admin/retention [SRC]. The admin Retention and Metrics tabs were removed from the UI: DOC git:b7994395 (#1362). plugins/admin/ui has no retention view (NEG; only artifacts.ts matches "retention"). INF: retention is API-only now.
- Retention cohorts: weeks start on Monday: DOC git:722d79ad (#1928) (product analytics or admin cohorts; INF).
- Egress: egress policy [SRC src/resolution/egress-policy.ts]; egress audit [SRC /v1/egress-audit; src/admin/postgres-egress-audit-sink.ts]; admin egress routes [SRC /v1/admin/egress]; egress authorization process [SRC src/egress-authz-main.ts]; Envoy egress proxy [SRC deploy/egress-proxy/envoy.yaml].
- Secret handling: secret masking [SRC src/security/secret-masking.ts]; redact-secrets in harness [SRC src/harness/redact-secrets.ts]; secret drop pages styled to match the web UI: DOC git:e57c0749 (#1525).
- Prompt-injection screening: security screener [SRC src/security/security-screener.ts]; posture [SRC src/security/security-posture.ts]; SECURITY_SCREEN=off|observe|enforce (simplified from three knobs): DOC git:4901e12f (#1784); classifier model or proxy [SRC src/config.ts SECURITY_SCREEN_CLASSIFIER, SECURITY_SCREEN_PROXY_*]; screening required independently of tool posture: DOC git:d83acb15 (#1602); model screening opt-in: DOC git:44e49c45 (#1201). Baseline: effectful paths can proceed as "unscreened" on proxy errors (src/core/orchestrator/security-screen.ts).
- Quarantine and release: /v1/admin/security/flags and /v1/admin/security/release [SRC src/api/routes/admin/security.ts].
- Postures: HARNESS_SECURITY_POSTURE strict|auto|dangerous [SRC src/config.ts ~1090]; sharing posture HARNESS_SHARING_POSTURE isolated|open [SRC src/config.ts ~1099; src/resolution/sharing-posture.ts].
- Command policy: approval rules and hard denials (recursive deletes, destructive SQL) in every posture [DOC README.md; SRC src/policy/command-policy.ts]; command-form approval compilation fixed: the 2026-08-17 baseline reconciliation cites commit d719f540.
- Auto-flagger: org-level auto-flagger settings and a test route [SRC /v1/admin/scopes/:scope/auto-flagger/test; src/api/routes/admin/auto-flagger-test.ts].
- Moderation of channels: ambient judgments are viewable [SRC /v1/admin/ambient-judgments].
- Admin transcripts and LLM captures: /v1/admin/sessions/:id and /v1/admin/sessions/:id/llm; admin skill [DOC skills-seed/admin/SKILL.md]; captured prompts are retained per the baseline (exact LLM requests; INF on retention).
- Spend export token: DOC git:ed11cb6c (#1798).
- Errors: Postgres error log [SRC src/admin/postgres-error-log.ts; /v1/admin/errors]; error-log moved from a process-local ring buffer to Postgres: SPEC (qm#2140); backend error reporting to Sentry, optional and sanitized: DOC git:73fea1e1 (#1349); browser error reporting: DOC git:919613d6 (#1392); sampled performance tracing: DOC git:62e07918 (#1410) [SRC src/instrument.ts; docs/error-reporting.md].
- Analytics: PostHog, optional and company-scoped: DOC git:68b5f5ba (#1348) [SRC plugins/web-ui posthog-js; src/util/product-analytics.ts].
- Identity and sign-in: portal session and OIDC [SRC plugins/portal/src/*]; password sign-in on the built-in auth broker: DOC git:3459f801 (#1549) [SRC plugins/auth/src/password.ts]; email codes via SMTP or Resend (env RESEND_API_KEY, AUTH_EMAIL_FROM, AUTH_ALLOWED_EMAIL_DOMAIN) [SRC plugins/auth/src/email.ts, smtp.ts; src/config.ts].
- Supply chain: npm audit (production) in CI [SRC .github/workflows/cicd.yml job dependency-audit]; dependency cooldown policy [DOC SECURITY.md "Dependency cooldown"]; CI rejects AI coauthor trailers on new PR commits [DOC AGENTS.md; cicd.yml job coauthor-trailers].
- Process: Wall of shame review in CI: DOC git:ef328f3c (#2151); requirement to review every PR with independent lenses [DOC AGENTS.md; SPEC].

Absent (NEG):
- SCIM: 0 hits.
- Data residency controls: none (regions appear only as deployment settings such as AWS_REGION and MODAL_REGIONS) (INF).
- DLP beyond secret masking and content screening: none found (INF).
- eDiscovery export of a whole org: none found; per-session transcript and LLM views and the spend export only (INF).

### 3.11 Client shell

Present, desktop app (desktop/, Electron 44.4.3):
- Instance chooser on first launch and "Change Instance…" (CmdOrCtrl+,) [SRC desktop/main.mjs:299; desktop/setup.html, setup.js; DOC desktop/README.md]; QM_DESKTOP_URL env sets the instance [DOC desktop/README.md].
- IPC: ipcMain handlers qm:open-browser, qm:current-instance, qm:connect [SRC desktop/main.mjs:255, 276, 280]; events qm:login-status and qm:cycle-tab [SRC main.mjs:87-129, 188]; preload exposes window.qm (preload.cjs) and window.qmDesktop (workspace-preload.cjs, sending qm:open-browser). Remote content is sandboxed with no Node access [DOC desktop/README.md].
- Browser sign-in handoff: the app opens /auth/desktop in the default browser; a two-minute single-use code bound to a proof key and state; redemption through the core claim store [DOC desktop/README.md; SRC plugins/portal/src/desktop-login.ts; DOC git:cef8de05 (#1528); DOC git:73fde174 (#1581)].
- Protocol and single instance: qm-desktop URL scheme registered [SRC desktop/main.mjs:242 setAsDefaultProtocolClient "qm-desktop"]; requestSingleInstanceLock [SRC main.mjs].
- Navigation: View Back (Alt+Left) and Forward (Alt+Right) [SRC main.mjs:320, 328]; off-origin links open in the browser; same-instance links open in a sandboxed preview window sharing the session [DOC desktop/README.md; SRC desktop/url.mjs, shortcuts.mjs].
- Permissions: microphone, camera and notification permissions are denied (SRC main.mjs:157-158); DOC desktop/README.md says "disabled in this prototype".
- Updates and signing: no automatic updates (DOC README); unsigned macOS prototype via npm run package; signed and notarized build via package:release with QM_MAC_SIGN_IDENTITY and QM_MAC_NOTARY_PROFILE (DOC README; SRC desktop/package.mjs).
- Multiview: up to 16 panes and Chrome tab shortcuts in the desktop app: DOC git:518c9e52 (#2153); session shortcuts for the first nine sessions while Cmd is held: DOC desktop/README.md; Cmd/Ctrl+1-9 web hotkeys removed: DOC git:0ba49de5 (#1546).
- Download placement: desktop download placed in Settings: DOC git:f577beae (#1587).
- Native notifications and tray: none (NEG for tray; INF).

Present, web shell and command surfaces:
- Command palette or Cmd+K resource search: plugins/web-ui/src/search.ts, search-resources.ts [SRC]; DOC git:80b87a19 (#1281).
- Keyboard shortcuts and focus management: 26 web and admin files match keyboard|shortcut|hotkey; dialog-focus.ts, grid-nav.ts, pane-focus.ts [SRC plugins/web-ui/src].
- Deep links: deep-link.ts, message-link.ts; in-app session links open in place: DOC git:5b273aed (#1793); the per-user option to open the web UI on a new chat: DOC git:981daeb8 (#2012).
- Multi-account and workspace switching: instance switching in desktop; trusted sign-in switching: DOC git:86264954 (#1232); portal sign-out is local: DOC git:742ed668 (#1808).
- Accessibility: aria attributes in 65 web and admin files; prefers-reduced-motion in 5 files [SRC grep]; focus handling as above.
- Internationalization: NEG (no i18n framework; 0 files match i18next, react-intl, lingui, gettext or the i18n token in the web and admin UI). English copy only (INF).
- Branding: ORG_BRAND_SELF_LABEL, ORG_BRAND_ORG_NAME, ORG_BRAND_ACCENT, ORG_BRAND_MARK [SRC src/config.ts; plugins/chassis/src/branding.ts; src/resolution/branding.ts]; operator-supplied SVG favicons: DOC git:330f2692 (#1538).
- Themes: theme import (importTheme, stripJsonc, parseHex, contrast, themeTokens) [SRC plugins/web-ui/src/theme-import.ts]; JSONC and VS Code-style theme tokens referenced [SRC same file]; app shell colors synced with chat theme: DOC git:51a05cc2 (#1512); density modes [SRC plugins/web-ui/src/density.ts; DOC git:1d4e6f01 (#1723)].
- Dark mode: one file matches prefers-color-scheme or data-theme (INF: limited dark-mode support; low confidence).

### 3.12 UX patterns and layout

Present:
- Navigation model (web): sessions list; Inbox (tabs and saved filters); Browse with skills, crons, webhooks, apps and files; Calendar; Sent mail; Contexts; Settings; admin console [SRC plugins/web-ui/src/inbox.ts, browse.ts, calendar.ts, sent-mail.ts, contexts.ts, settings.ts, shell.ts; DOC git:db4c3a19, git:03a793fa]. Baseline criticised the repeated Browse control patterns (SPEC wall of shame, Mismatched UI).
- Split panes and multiview: split-layout.ts and split.ts on dockview-core; pane kinds [SRC plugins/web-ui/src/pane-kinds.ts]; compact pane header: DOC git:95adea59 (#1550); restored tabs no longer stuck: DOC git:001d1b8d (#1658); pane focus on switch: DOC git:8472a820 (#1714).
- Assistant sidebar and shared AI sidebar in Inbox: [SRC plugins/web-ui/src/assistant-sidebar.ts]; DOC git:db4c3a19.
- Density: density.ts; compact composer default in multiview: DOC git:129f34b9 (#1659); suggestions hug the composer in empty panes: DOC git:67b66747 (#1686).
- Onboarding and empty states: onboarding-welcome.ts and css, onboarding-slack.ts, welcome-session.ts, welcome-ideas.ts [SRC plugins/web-ui/src]; welcome copy refreshed with launch ideas: DOC git:7a23b454 (#1315) (SPEC says YC welcome copy remains, a baseline-level leak to fix); Slack onboarding checklist: DOC git:48894822; cohort welcome: DOC git:4c0ee215.
- Activity and inbox views: inbox.ts and inbox-chat.ts, inbox-coalesce.ts [SRC]; Inbox sender avatars with source badge: DOC git:79843c8c; admin activity page [SRC plugins/admin/ui/activity.ts].
- Thread and subagent panels: subagent card above the composer: DOC git:a5d3358c (#1733); subagent activity in the parent session: DOC git:484bb387 (#1710); subagent cards show delegated task: DOC git:978aa7b9 (#2150).
- Composer design: composer.ts, composer-loadout.ts, embedded-composer.ts; attachments (file-drag.ts, paste-text.ts, folder-drop.ts); model, effort and speed pickers (model-picker.ts, model-options.ts, context-model.ts); emoji completion; drafts (drafts.ts). Context settings reuse the composer model picker: DOC git:81aa332f (#1476).
- Pinned prompt and transcript: pinned prompt condensed when stuck: DOC git:30ddd393 (#1515); transcript paging by turns: DOC git:4d4e260b (#1687); scrolled-up readers stay in place when a turn ends: DOC git:941c4a19 (#1548).
- Information architecture: scope-centred context page (contexts.ts) [SRC]; the baseline calls this the "scope-centered resource UI" and recommends building Space tabs from it.
- Admin design and layout: admin pages redesigned; admin tabs migrated to Lit: DOC git:685b7726 (#1441); admin design page gated by inbox access: DOC (#1439): admin design system gated by inbox access.
- Information architecture changes: "Sessions" label removed from the sidebar: DOC git:d3526537 (#1361); Cmd/Ctrl+1-9 hotkeys removed: DOC git:0ba49de5 (#1546).

Absent (NEG):
- Full-page workspace or Space home view: none; closest is the scope-centred contexts page and Inbox (INF).
- Kanban or board task view: none (see 3.8).

## 4. Extension points (plugin, skill, MCP, API, CLI, webhook surfaces)

- Skills (SKILL.md packages with frontmatter): 25 seeds in skills-seed/*/SKILL.md; one plugin skill in plugins/onboarding/skills/onboarding/SKILL.md; scope-owned skills shareable by grant; admin-gated promotion to the org; skill packs imported from git repositories; explicit skill tool with per-turn skill directories (DOC git:98ac521e); seed install deduplicated at boot (DOC git:b06e8c95). Env: SKILLS_SEED_DIR, SEED_SKILLS, PLUGIN_SKILLS_DIRS, SKILL_SIGNING_SECRET, SKILL_SYNC_POLL_MS. [SRC src/skills/*, src/config.ts; DOC docs/skill-registry.md; DOC README.md "Shared skills"]
- Plugins (repo packages, not a runtime plugin loader): plugins/admin, auth, portal, web-ui are separate packages that call core over signed HTTP; AGENTS.md forbids plugins from importing core, except the shared plugins/chassis package. [SRC plugins/*; DOC AGENTS.md]
- Deployment directory and layers: a deployment directory with qm.config.jsonc; deploy/layers/<org>/ for org-private material; imageOverrides for custom images; typed secret routing; digest evidence; CLI commands check, plan, up, live checks. [DOC docs/deploy-directory.md (headings only read), cli/README.md, README.md; SRC src/deployment/*, cli/src/deployment-layer.ts]
- Source forks and upstream sync: update-qm and upstream-pr workflows (developer skills in .claude/skills and .codex/skills); deploy-qm skill for agents (cli/templates/deployment/SKILL.md; .codex/skills/deploy-qm). [DOC README.md "Customize your instance"; SRC .claude/skills, .codex/skills, cli/templates]
- REST product API: /v1/* (307 distinct path literals in src/api). Admin API: /v1/admin/* (used by the admin UI and by the admin agent tool). Agent self-API catalog: /v1/apis [SRC src/api/agent-api-catalog.ts].
- Admin agent tool: agent-callable admin operations through existing handlers, with authority rechecked per call and audit kept (DOC git:e652241b "Expose native admin actions through existing authenticated handlers (#2208)"). [SRC src/harness/agent-tools.ts near line 3360; src/tools/admin.ts adminOperations; src/api/routes/admin/*]
- CLI "qm" with 22 commands plus help and version (init, setup, dev, check, config, slack, outputs, admin-login, conformance, infra, layer, up, plan, migrate, sandbox, status, logs, down, rollback, doctor, proof, secrets). [SRC cli/src/cli.ts:244-590]
- Connector SDK: bundled into sandbox images before sandbox creation (DOC git:fc055480, a83118e9). [SRC deploy/connector-sdk/build.mjs; src/sandbox/connector-sdk.ts]
- MCP: client to admin-managed MCP servers, with per-user credentials for MCP connectors (DOC git:31f44cde). MCP server exposure: NEG. [SRC src/mcp/*, src/api/routes/admin/mcp-servers.ts]
- Model extension points: custom providers with the Responses protocol (DOC git:b52b1c17; /v1/admin/custom-providers); model gateways (MODEL_GATEWAY_URL, MODEL_GATEWAY_API_KEY, MODEL_GATEWAY_API_KEY_HEADER, MODEL_GATEWAY_MODELS); subscription logins; browser-backed inference. [SRC src/model/*; src/config.ts]
- Harness adapters behind one interface: pi, opencode, codex, claude, mock. [SRC src/harness/harness.ts, harness-router.ts]
- Sandbox backends behind one contract: ten providers plus local Docker (section 3.7). Adding a backend is an open proposal in adrs/agent37-sandbox-backend.md (Agent37 offers to host the agent computer). [SRC src/sandbox/*; DOC adrs/]
- Memory providers: external providers routed by scope; MCP memory provider; strategy plug-ins (agent-only, consolidation, per-turn, scratch-promote). Env MEMORY_PROVIDER_CONFIG, MEMORY_STRATEGY. [SRC src/memory/provider-router.ts, mcp-memory-provider.ts, strategies/*; DOC docs/memory-providers.md]
- Storage and backend switches: SESSION_STORE, RUN_STORE, ARTIFACT_STORE, SNAPSHOT_STORE, TRANSFER_STORE, SECRETS_BACKEND (env or aws), DEPLOY_PROVIDER (docker, aws, fly, porter), SANDBOX_BACKEND, SANDBOX_SECONDARY_BACKEND, SANDBOX_SCOPE_BACKENDS. [SRC src/config.ts]
- Feature flags (9, per-scope, set by an admin): app_annotations, persistent_subagents, responsive_spine, inbox_loops, slack_loading_indicator, external_app_sharing, loop_triage, swarms, team_scopes. [SRC src/feature-flags.ts]
- Client-executed tools in turns: a turn can call tools the caller executes (DOC git:bbd7ee4e; SRC src/core/orchestrator/external-tools.ts).
- Webhooks: inbound with HMAC-SHA256 verification (SRC src/webhooks/verifiers.ts). Gmail Pub/Sub push (SRC src/loops/gmail-push.ts). Slack managed events (SRC /v1/slack/managed/events).
- Branding and themes: ORG_BRAND_SELF_LABEL, ORG_BRAND_ORG_NAME, ORG_BRAND_ACCENT, ORG_BRAND_MARK; operator-supplied SVG favicons (DOC git:330f2692); theme import (SRC plugins/web-ui/src/theme-import.ts).
- Governance extension: Wall of shame lenses are enforced by a CI check that reads review reports (DOC docs/SPEC.md; DOC git:ef328f3c; .github/workflows/wall-of-shame.yml).

## 5. Data and protocol hints

- Primary storage: Postgres when DATABASE_URL is set; SESSION_STORE, RUN_STORE and ARTIFACT_STORE switch to postgres. Without it, sessions, runs and artifacts live in process memory, and config.ts warns about ephemeral stores (SRC src/config.ts ~1233-1238). Pooling: DATABASE_POOL_URL, DATABASE_POOL_MAX, DATABASE_DIRECT_POOL_MAX. TLS roots: DATABASE_CA_CERT, DATABASE_CA_CERT_FILE, DATABASE_POOL_CA_CERT. PgBouncer transaction pooling supported (DOC docs/pgbouncer.md; .github/workflows/pgbouncer.yml). Migrations: src/persistence/* and test/postgres-migrations.test.ts; migrate command in cli.
- Named store entities (DOC or SRC): session_entries and session_tape (dual, with SESSION_TAPE_MODE shadow or serve; DOC SPEC), error_events (DOC SPEC; qm#2140), spend_usage_json (DOC git:24c374e8), audit_log and audit_events (SRC src/admin/postgres-audit-log.ts), run and signal tables (SRC src/runs/postgres-run-store.ts), delivery records (SRC src/delivery/postgres-delivery-store.ts), cron fire and job state (SRC src/cron/fire-store.ts, job-queue.ts), team store (SRC src/teams/teams.ts), feature flags in a durable map (SRC src/feature-flags.ts).
- Object storage: S3 for blobs and snapshots (S3_BUCKET, S3_PREFIX, S3_REGION) and per-provider snapshot buckets (SPRITES_, E2B_, MODAL_, SMOLMACHINES_, DO_AGENTS_ snapshot bucket variables). Litestream smoke script for AWS: scripts/aws-litestream-smoke.ts (INF: SQLite replication path exists for an AWS profile; not traced).
- Auth and signing: surface requests are HMAC-signed and replay-checked (SRC src/auth/source-auth.ts, replay-dedupe.ts). Secrets named in config and .env.example (names only recorded): CORE_SIGNING_SECRET, PORTAL_IDENTITY_SECRET, REQUIRE_SIGNED_PORTAL_IDENTITY, PORTAL_SESSION_SECRET, DEPLOY_APPS_SESSION_SECRET, CAPABILITY_SECRET, CONNECTOR_SECRET_KEY, SKILL_SIGNING_SECRET, DEPLOYMENT_CONTROL_SECRET, MODEL_GATEWAY_API_KEY, SPEND_EXPORT_TOKEN. Capability tokens: SANDBOX_CAPABILITY_TTL_HOURS (default 48h, or 0 or none for non-expiring), CAPABILITY_TOKEN_COMPRESSION (SRC src/auth/capability-token.ts; DOC docs/capability-tokens.md).
- Portal identity and sessions: framed session cookies (DOC git:bbc3e60b (#1617)), trusted OIDC issuer env TRUSTED_OIDC_ADMIN_ISSUER (SRC src/config.ts), desktop browser handoff code, two minutes and single use, bound to a proof key and state (DOC desktop/README.md; SRC plugins/portal/src/desktop-login.ts).
- OAuth: connector OAuth flows with consent links and redemption (SRC src/connectors/oauth.ts, consent-link.ts, oauth-flow-store.ts). Consent mint and redeem routes under /v1/connectors/oauth/consent/*.
- Wire formats: JSON over HTTP (Fastify); Server-Sent Events for run events, session state and loop item events (text/event-stream routes); Slack Web API and Events (HTTP events mode or Socket Mode); Anthropic Messages through @anthropic-ai/claude-agent-sdk; OpenAI Responses API for OpenAI and gateway models (DOC docs/model-gateway.md); chat-completions style route /v1/browser-model/chat/completions (SRC src/api/routes/browser-model.ts); Gmail Pub/Sub push envelopes (SRC gmail-push.ts); MCP client transport (SRC src/mcp/mcp-client.ts; transport type not traced).
- Session model: append-only session tape with exact transcript fields (DOC docs/session-tape-spec.md; DOC git:45be37ee); entries addressable by sequence (/v1/sessions/:id/entries/:seq); run events by run id (/v1/runs/:id/events); per-attempt tool ledger (SRC src/runs/tool-ledger.ts).
- Sync and idempotency: deterministic cron fire keys (SRC src/cron/fire-store.ts; DOC SPEC); idempotency store (SRC src/idempotency/idempotency-store.ts); delivery acknowledgments by key (/v1/deliveries/ack-by-key); Postgres LISTEN/NOTIFY wakeups (SRC src/persistence/postgres-notify-bus.ts); leases and heartbeats (SRC src/runs/postgres-run-store.ts; DOC baseline).
- Egress: HTTP_PROXY, HTTPS_PROXY, ALL_PROXY, NO_PROXY and SOCKS via socks-proxy-agent (SRC src/config.ts; package.json); Envoy egress proxy (SRC deploy/egress-proxy/envoy.yaml); egress authorization process (SRC src/egress-authz-main.ts); per-sandbox egress proxy URL variables (for example E2B_EGRESS_PROXY_URL, DO_AGENTS_EGRESS_PROXY_URL).
- Observability: Sentry DSN and tracing (SRC src/instrument.ts; docs/error-reporting.md), PostHog (POSTHOG_API_KEY, POSTHOG_HOST), Postgres error log, audit sink, egress audit sink, metrics sink (SRC src/admin/*-sink.ts). Names only recorded.
- Secrets at rest: keychain logins are encrypted in the owner's keychain and refreshed centrally (DOC .env.example comments; DOC docs/credential-refresh-handoff.md). Algorithm not traced.
- Export and retention: spend export with a token (DOC git:ed11cb6c, 1e975255); retention module (SRC src/admin/retention.ts); per-session LLM capture view (SRC /v1/admin/sessions/:id/llm).

## 6. Blockers and limits (what I could not see)

- Read-only pass: nothing was built, run, installed or tested; no REA analysis (source only). Git was used only for read-only log, show and grep.
- Enumerations are grep-based and partial: 307 distinct /v1 path literals; agent tool names from the "name:" literals in src/harness/agent-tools.ts; route modules listed by file name only; admin operations (src/tools/admin.ts adminOperations) and the surface-tool schema (src/tools/primitives.ts) not enumerated in full.
- Partial reads: docs/SPEC.md read in full. Headings only for docs/session-tape-spec.md (24KB), docs/deploy-directory.md (31KB), docs/porter.md (30KB), docs/qa/fly-published-apps.md (21KB), docs/sandbox-preservation.md (15KB), docs/swarms.md (16KB), docs/persistent-subagent-sessions.md (7KB), SECURITY.md. Not read beyond names: most src logic, web-ui and admin component bodies, test bodies.
- Verevon baseline: read the QM capability table, the adapt and reject lists, and the 2026-08-17 reconciliation section of CoreSystem/apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md. The 284KB Verevon comparison doc (verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md) was read by heading and in the capability and pattern tables only.
- Negative results are grep-based and can miss renamed code. AG-UI is the clearest example (see 7a).
- Commit-derived claims not verified in code: AG-UI events (DOC git:d42cf2a8); the Slack default events mode (src/config.ts:1492 shows only the HTTP branch); fallback-runtime status (see 7e).
- Stale baseline pointer: the baseline cites pi-tools.ts (lines 82-94 and 261-274). No pi-tools file exists under src/ or plugins/. The behavior may have moved to src/harness/agent-tools.ts or src/tools/primitives.ts (INF; not traced).
- Excluded on purpose: credential, session and user-data stores; no .env file was read. Only .env.example was read, and only key names were recorded; deploy/stacks/acme/.env.example was not read.
- Unknowns: sandbox default in production (config requires SANDBOX_BACKEND to be set explicitly in production, src/config.ts ~1319); whether the monitors have kind values beyond process-id and running (grep found only these literals); whether src/admin/metrics sink is reachable from the admin UI (plugins/admin/ui/metrics.ts exists, no Metrics tab label found).
- Verified negatives (NEG, grep over src, plugins, desktop, cli): audio or video calls and huddles; voice clips; slash commands (no app.command); SCIM; passkeys or WebAuthn; i18n framework; MCP server exposure; scheduled send; saved items or bookmarks; mute or do-not-disturb; outbound webhooks; polls; typing or presence UI.

## 7. Changes since the 2026-08-13 baseline (for the Verevon comparison to correct)

### 7a. New since the baseline (DOC git hashes or PR numbers; SRC where checked)

| Area | What changed | Evidence | Status |
|---|---|---|---|
| Swarms | Durable agent pools, scoped messages, Modal-backed workers, shared board, per-person gating, deployments can disable | DOC git:e0b8686c, 7df97144, 1ecb06c5, e32e9431; docs/swarms.md; flag swarms | Flagged |
| Persistent subagents | Durable coordination, inline badges, tree run cap 50, parent wake | DOC git:1bd53763, 93b2c434, 6392c5c2; docs/persistent-subagent-sessions.md | Flagged (persistent_subagents) |
| Goals and governor | Goal tool, governor steps back and pauses stuck goals, only human or verifier ends a goal | DOC git:dc93931b, 9143874b, 54e04d1f; SRC src/harness/goal.ts, agent-tools.ts 3910-4000 | Shipped |
| Team scopes | Team scope kind in turns; team store | DOC git:242436b9 (2026-10-11); SRC src/teams/teams.ts | Flagged (team_scopes) |
| Inbox Loops | Flagged loops, durable event ingestion, Gmail and Slack sources, triage, ship gate, saved email filters | DOC git:a921fb0c, db4c3a19, 7d7bd51f; SRC src/loops/* | Flagged (inbox_loops) |
| Calendar, Sent mail, email archiving | Calendar tab and sent-mail view | DOC git:03a793fa; SRC plugins/web-ui/src/calendar.ts, sent-mail.ts | Shipped |
| Desktop app | Electron client, browser sign-in code handoff, qm-desktop protocol, multiview up to 16 panes | DOC git:cef8de05, 518c9e52, 73fde174; SRC desktop/main.mjs | Prototype; no auto-update; mic, camera and notifications denied |
| Sandboxes | Superserve, DigitalOcean Managed Agents, per-command machine lock removed, shared computer paused after last turn | DOC git:7df57367, 51385cfd, c616b40f, daeb2deb | Shipped |
| Deploy targets | Porter in-cluster apps, Fly private scale-to-zero routing, AWS isolated deployments on shared foundations | DOC git:2fdc93c2, 5a3a4dc3, 518b68c5 | Shipped |
| Sign-in | Password sign-in on the auth broker; trusted OIDC entry; trusted sign-in switching | DOC git:3459f801, e4b35024, 56bbd92e, 86264954 | Opt-in |
| App sharing | Publishing private by default; General access menu; external sharing off by default; opt-in public sharing | DOC git:a153974f, 75c83bc1, 27501bfe, 0edd2377 | Shipped |
| Credential approvals | Native web approvals; the approval card is the only approval path for credential use | DOC git:f94ad37e, 1906e4af | Shipped |
| Screening | Off, observe, enforce; screening required independently of tool posture | DOC git:4901e12f, d83acb15; SRC src/security/*, src/core/orchestrator/security-screen.ts | Shipped; unscreened path still present |
| Spend | Admin dashboard with breakdowns; token-authenticated export; custom windows; hardened export | DOC (#1614), git:ed11cb6c, 1e975255, 05a62c14 | Shipped |
| Observability | Optional sanitized Sentry backend and browser reporting; sampled tracing; optional PostHog | DOC git:73fea1e1, 919613d6, 62e07918, 68b5f5ba | Optional |
| Client-executed tools | Turns can call tools the caller executes | DOC git:bbd7ee4e; SRC src/core/orchestrator/external-tools.ts | Shipped |
| Run events | Run tool calls and results streamed as events (commit claims AG-UI) | DOC git:d42cf2a8; SRC src/runs/run-stream-events.ts | AG-UI name NOT found in source |
| Ambient channel judge | Judges batches of unaddressed channel messages; channel policy ledger; admin view | SRC src/surface-cache/ambient-judge.ts, channel-policy-store.ts; /v1/admin/ambient-judgments | Present (first commit date not checked) |
| Models | Claude Haiku 5.5 (helper and judge), Sonnet 5.5, claude-opus-5-5, GPT-6 Sol and Luna, GPT-6.1 Sol, Astra Ultrafast, eight presets, custom Responses protocol | DOC git:bca3b27f, 0d59cb20, a0b97066, 8a9b1808, 62f8d9e9, 6c09f179, aa0956ae, b52b1c17 | Shipped per commit |
| Context economy | Stable prompt prefixes, trimmed per-turn context, memory recall once per active context | DOC git:a61db71b, 54c5f16a, ca47f5c8 | Shipped |
| Skills | Explicit skill tool, lazy materialization, shortlist ranking, published sources read without sandboxes | DOC git:98ac521e, 743b5c0d, 6374e44c, dfdde0bc | Shipped |
| Cmd+K | Search over skills, crons, apps and projects | DOC git:80b87a19; SRC src/search/resource-search.ts | Shipped |
| Mermaid | Diagrams in chat and shared conversations | DOC git:e49134f3; SRC plugins/web-ui/src/mermaid-block.ts | Shipped |
| Slack | Opt-in task cards; opt-in external isolation; thread-based conversations restored; sender avatars; hand-off DM skill | DOC git:82342138, cc95b9ef, b875b0a7, 79843c8c, fc54a60b | Shipped |
| Admin | Native admin actions through existing handlers; bounded session list; Lit admin tabs | DOC git:e652241b, df06097f, 685b7726 | Shipped |
| Connectors | Composio through ordinary credentials and skills; provider-managed Slack installation | DOC git:79b0d272, 98ffa809; SRC src/api/routes/composio.ts | Shipped |

### 7b. Removed, reverted or deprecated since the baseline

- Canvas and UI-state skills (ui_canvas): added DOC git:19fdb8d0 (2026-10-09), reverted DOC git:f5e37f63 (2026-10-10). Not shipped.
- Admin Metrics and Retention tabs removed from the UI: DOC git:b7994395 (#1362). The /v1/admin/retention and /v1/admin/metrics routes remain (SRC routes list). plugins/admin/ui has no retention view (NEG). INF: retention is API-only now.
- Cmd/Ctrl+1-9 session-switch hotkeys removed (web): DOC git:0ba49de5 (#1546).
- Per-command exclusive sandbox machine lock removed: DOC git:c616b40f (#1748).
- Legacy sandbox routing retired: DOC git:1ea79127 (#2019). Legacy cron tick job removed: DOC git:b528b8ff (#2007).
- Security screening knobs collapsed to SECURITY_SCREEN: DOC git:4901e12f (#1784). Retired BRAIN_* memory variables (docs/memory-providers.md).
- /v1/keychain/use: SPEC (2026-10-08 entry) lists its deletion PR (qm#2146) as open, but the route is absent from src/, plugins/ and cli/ (NEG). Treat the endpoint as removed; SPEC is stale here (SRC).
- Refusal fallback: the hard-coded ladder still exists (SRC src/harness/pi-harness.ts:1030, REFUSAL_FALLBACK_MODEL_IDS = claude-opus-5, claude-sonnet-5) and an org fallbackRuntime setting exists (SRC src/api/routes/admin/scope-config.ts:277, 378). Commit DOC git:1b2d07e9 moved provider errors to typed errors, so the regex-matching part may be gone (INF; not traced).

### 7c. Items the baseline did not cover or under-weighted

1. Swarms (durable agent pools with a shared board and Modal-backed workers): absent from the baseline capability table.
2. Persistent subagents and subagent tree controls: the baseline names background process sessions, not subagent coordination.
3. Goals with a governor (agent-set objectives; only a human or an independent verifier can end one): absent.
4. Team scopes (flagged): the baseline assumes person, room, project and org scopes.
5. Inbox Loops (email and Slack triage queue with ship gate and grants): the baseline covers scheduled runs and watches, not this queue.
6. Desktop app (Electron, browser sign-in handoff): not mentioned.
7. Calendar tab and sent-mail view: not mentioned.
8. Composio and the connect-apps consent-link flow: the baseline mentions connectors broadly, not the consent-link mint and redeem path.
9. Ambient channel judge and channel policy ledger: the baseline calls ambient behavior a QM lead, without the batch judge design.
10. Client-executed tools in turns: absent.
11. Admin spend dashboard and token-authenticated export: the baseline mentions budgets only.
12. Model fast tier (Ultrafast) and saved presets: absent.
13. QM process governance: the Wall of shame with CI review check, the no-comments rule, "delete before you add", "fix every instance", and the screenshot ban (SPEC, AGENTS.md). Candidates for Verevon's engineering process.
14. Stale baseline pointer: pi-tools.ts (see section 6).

### 7d. Baseline strengths still present in code (SRC, not deeply re-verified)

- Scope resolver: src/resolution/resolution-service.ts.
- Audience floor and context filter: src/resolution/audience-floor.ts, context-filter.ts.
- Interrupted-effect handling on resume: src/core/turn-resume.ts; src/harness/context-compaction.ts; interrupted tool results: DOC git:32046dc5.
- Durable run ownership and leases: src/runs/postgres-run-store.ts.
- Typed harness adapters: src/harness/harness-router.ts.
- Signed surface requests: plugins/chassis/src/core-client.ts; src/auth/source-auth.ts.
- Substrate capability profile: src/sandbox/sandbox.ts.
- Watches: src/monitors/*.
- Deterministic cron fire keys: src/cron/fire-store.ts.
- Memory revision store: src/memory/postgres-memory-service.ts (baseline cites append-only revisions; not re-verified).
- Scope-centred UI: plugins/web-ui/src/contexts.ts.

### 7e. Baseline reject items, current state (SRC where noted)

- Bearer public app links as default: changed. Publishing is private by default; public sharing is opt-in (DOC git:a153974f, 0edd2377).
- Fail-open screening: still present in part. src/core/orchestrator/security-screen.ts defines an unscreened verdict (line 21-23) and src/core/attachments.ts keeps an unscreened list (lines 341-397). Enforcement semantics were not traced.
- Audit writes that log and drop on database failure: postgres-audit-log.ts record() queues writes in a pending set (lines 53-70 region); failure handling was not traced (INF).
- Plaintext long-lived sandbox credentials and development fallbacks that mix signing secrets: not re-checked.
- Second Postgres work queue: pg-boss is used for crons (src/cron/job-queue.ts) and the Postgres run store remains; the baseline's concern about a second queue applies to Verevon, not to QM.

End of inventory.
