# Implementation spec: "Forward an email to create a task" (Manus Mail parity)

Scope: mail-task item from `CHAT_PAGE_GAP_PLAN_2026-09-28.md` (Wave 2, T0). Read-only research; no files changed.

## 1. What exists today (file:line evidence)

**There is no personal-inbound-mail-address capability anywhere in the repo.** Everything that looks adjacent is actually two *different*, unrelated pipelines:

**A. Inbound-mail-to-Support-Inbox (existing, unrelated to chat)**
- `apps/Ingestion Plane/integration-corev2/internal/emailsync/emailsync.go:1-26` — polls **already OAuth-connected** Gmail/Outlook mailboxes (plus Teams/Slack/X/Discord as chat sources) via `providerPlans` (`:453-460`) and posts normalized messages to a bridge.
- `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs:146-193, 199-289` — `/internal/ingest/email`, HMAC-delegation-gated (`x-service-id: integration-email-worker`), forwards to conversation-core-go.
- `apps/Application Plane/conversation-core/conversation-core-go/internal/conversation/service.go:126-180` (`IngestEvent`) and `types.go:806-833` (`InboundEvent`) — this **creates/updates a Support-Plane omnichannel ticket/conversation** (the `src/features/inbox/*` "Verevon Inbox" / `TicketQueue` product), confirmed by the doc comment at `types.go:841-848`: "turns it into a normal inbound conversation... reviews it in the org's own Inbox."
- This whole path requires a pre-existing **connection** (`store.Connection`). There is no code path where an arbitrary external sender's email reaches the system without one.

**B. Webhook receivers that exist (also unrelated — all connection-scoped)**
- `integration-corev2/internal/api/server.go:889-1029` — generic `/api/v1/webhooks/:provider`, with signature checks only for Stripe/Slack/GitHub/Shopify (`server.go:2415-2523`).
- `integration-corev2/internal/webhookorg/resolver.go:1-25` — resolves org for Meta/Slack **account-wide** callbacks by matching provider account ids against an **existing connection's** `provider_account_id`/`provider_context`.
- **No SMTP/Mailgun/Postmark/SendGrid inbound-parse endpoint exists.** Grep for `mailgun|postmark|sendgrid|inbound.?mail|smtp` across the whole repo returns nothing but the two pipelines above.
- **No per-user alias/personal-address concept exists** (no plus-addressing, no alias table, no receiving domain) anywhere in Control Plane, Ingestion Plane, or Frontend Plane.

**C. Chat-page threads (the actual target surface) — different data model again**
- Chat-page threads are owned by **session-core**, not conversation-core. `session-core/src/grpc.rs:2128`: `THREAD_ORIGINS = ["chat", "space", "agent_run", "support", "system"]` (added by migration `session-core/migrations/0034_thread_origin.sql`).
- `resolve_thread_origin` (`grpc.rs:2130-2170`) derives origin from an explicit field or a session-key convention (`support_` prefix ⇒ `"support"`, `grpc.rs:2192`).
- The gateway relays this to the SPA: `apps/gateway/src/domains/chat/history.rs:41-46` — `origin` field on `ChatThreadSummary`, with a comment that today "nothing on the client reads it."
- The existing `support_`-origin mechanism (`apps/gateway/src/domains/chat/support.rs:132-160`) is **not** an auto-drafting pipeline — it only injects a permission-filtered, read-only ticket/conversation summary into Global Chat's prompt when a *team member* opens a thread whose id starts with `support_`. It is unrelated to end-user personal inboxes.
- `"agent_run"`-origin threads back the existing **durable background-run surface** (`src/features/agents/components/AgentRunConsole.tsx`), i.e. a run the user is not live-typing into. The plan itself notes this already lacks a completion-notification binding (gap matrix row "Background task with completion notice").
- Thread creation is idempotent on `session_key` (evidenced by the retry test at `grpc.rs:7130-7184` and the `support_` convention re-using the same key), and it validates `origin` against a fixed enum with a consistency check for `space_id` (`grpc.rs:2148-2170`).

**D. The only existing "start a run with no live user" mechanism**
- `apps/Model Plane/go/services/capability-core/internal/taskexec/workflow_dispatcher.go:44-96` + `internal/cron/*` — capability-core's cron sweeper starts `ScheduledRunSupervision` via orchestrator-core, using `PrepareScheduledRunThreadRequest` (`proto/model_plane/v1/sessions.proto:384-410`). This RPC is **tightly coupled to Space authority** (`space_id`, `space_decision_ref`, `recipient_audience_ref`, `resource_authorization_ref`, `control_decision_token`, …) — built for scheduled *Space* work, not a personal mailbox task. Not directly reusable without dragging in Space semantics.

**E. Reusable building blocks**
- Attachment extraction (ephemeral, no storage): `imports-core/app/main.py:204-212` → `POST /api/v1/import/extract`, backed by `imports-core/app/chat_extract.py:12-30` (`ChatExtractRequest{filename, content_base64 ≤1.4MB, content_type}`, 1MB decoded cap, `HTTPException(415)` for unsupported types). Allow-list: `imports-core/app/config.py:88` = `pdf,docx,txt,md,csv,json,html,htm` (**no xlsx/pptx** — same known limit as Wave 1).
- Notification delivery: **not** an HTTP "create notification" endpoint — notification-core's only HTTP routes are `/health`, `/ready`, and a Novu delivery callback (`notification-core/internal/http/server.go:59-69`). Real integration pattern is a **NATS consumer**: `notification-core/internal/consumers/conversation_followed_message.go:1-70` subscribes to `verevon.application.conversation.message.received` on JetStream stream `VEREVON_APPLICATION` and turns it into a content-free, preference-gated notification via `notification.Service` (`NotificationAccepter`).
- The gateway already holds a NATS client (`apps/gateway/src/gdpr_nats.rs`, wired in `apps/gateway/src/main.rs`) — so it, not Model Plane, is the natural publisher for a new completion event.
- Sender identity: `email_verified` exists on the Control-Plane user record (`auth-core`/`user-core`, e.g. `auth-core/auth-schema.ts`, `user-core/internal/users/types.go`) — usable as the anti-spoofing anchor.
- Effectful-turn lock (`src/shared/chat/effect-class.ts:15`) and inline plan/tool approval rungs (`ChatMessages.tsx:1791`, `:1531-1568`) already gate any write-capable tool call behind explicit user approval, independent of whether a live SSE session exists (run receipts are durable). This is the mechanism that gives us "draft-only" for free, if the mail-task run goes through the same execution-core path as any other turn.
- HMAC service-to-service delegation pattern to copy for every new internal endpoint: `conversation-ingest-rs/src/lib.rs:164-193, 308-367, 381-462` (canonical string, nonce+timestamp window, replay cache).

## 2. Design decision this evidence forces

Do **not** try to extend the Support-Inbox emailsync pipeline (A) — it is the wrong product surface (tickets, not chat threads) and is hard-wired to existing OAuth connections. Build a new, small, parallel pipeline that:
1. Maps a personal alias → `(org_id, user_id)` (new).
2. Receives real inbound mail via a provider inbound-parse webhook (new).
3. Verifies the sender, extracts attachments (reuse imports-core `/api/v1/import/extract`).
4. Creates a session-core thread with a **new** origin `"mail_task"` (extends the existing enum, does not touch the other four) and posts the email as the first user turn, then triggers exactly one execution-core turn — using the same tool/approval machinery as live chat, so write-capable tools already come out as pending approvals, not silent actions.
5. Publishes a NATS completion event the gateway already has plumbing for; a new notification-core consumer (mirrors the existing follower-notification consumer) turns it into an in-app/email notification deep-linking to the thread.

## 3. Exact change list

### 3.1 Control Plane — `user-core` (personal address, new package `internal/mailtask/`)
- **New file** `internal/mailtask/types.go`:
  ```go
  type Address struct {
      Alias     string     // opaque token, e.g. base32(12 random bytes), globally unique
      OrgID     string
      UserID    string
      CreatedAt time.Time
      RevokedAt *time.Time
  }
  ```
- **New file** `internal/mailtask/service.go`: `GenerateAlias(ctx, orgID, userID) (Address, error)` (idempotent — returns the existing live alias if one exists per `(org_id,user_id)`), `ResolveAlias(ctx, alias) (Address, error)` (`ErrNotFound`/`ErrRevoked`), `RevokeAlias(ctx, orgID, userID) error` (also mints a fresh alias so "regenerate" in Settings is one call).
- **New migration** `NNN_mail_task_addresses.up.sql`: `mail_task_addresses(alias TEXT PRIMARY KEY, org_id TEXT NOT NULL, user_id TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), revoked_at TIMESTAMPTZ)`, partial unique index `(org_id, user_id) WHERE revoked_at IS NULL`.
- **Edit** `internal/http/handlers.go`: add self-service `GET/POST/DELETE /api/v1/users/me/mail-task-address` (session-authenticated, returns `{address: "<alias>@<receiving-domain>"}`), and an internal `GET /internal/mail-task/resolve?alias=` protected the same way other internal routes in this file are (service-token / HMAC — match the existing convention in this file rather than inventing a new one).

### 3.2 Ingestion Plane — `integration-corev2` (new package `internal/mailtask/`)
- **New file** `internal/mailtask/receiver.go`: per-provider payload parsers → common shape:
  ```go
  type InboundMailTaskEvent struct {
      Provider, ProviderMessageID string
      Alias                        string // parsed from To:
      FromEmail, FromName          string
      AuthResult                   string // "pass"|"fail"|"none" derived from provider's SPF/DKIM/DMARC fields
      Subject, TextBody, HTMLBody  string
      Attachments []struct{ Filename, ContentType string; Content []byte }
  }
  ```
  Postmark: JSON body, `Headers[]` contains `Authentication-Results`; SendGrid Inbound Parse: multipart form with `SPF`, `dkim`, `envelope`, `attachment1..N`; Mailgun: multipart form with `X-Mailgun-*` auth fields, `attachment-1..N`. Each provider needs its own small parser behind the common interface.
- **New file** `internal/mailtask/verify.go`: provider webhook signature/basic-auth check (mirror the signature helpers already in `internal/api/server.go:2415-2523`); reject (200 + drop, never leak *why* to the sender) unless `AuthResult == "pass"` **and** `strings.EqualFold(FromEmail, resolvedAddress.VerifiedEmail)`.
- **New file** `internal/mailtask/attachments.go`: for each attachment whose extension is in `{pdf,docx,txt,md,csv,json,html,htm}` and ≤1MB, call imports-core `POST /api/v1/import/extract` (base64 inline) and keep the returned text; others are recorded by filename only ("attachment not extracted: report.xlsx") — never silently dropped, never fails the whole message.
- **Edit** `internal/api/server.go` (near the existing webhook block, `:889`): register `app.Post("/api/v1/webhooks/mail-task/:provider", rateLimited, mailtaskHandler)`.
- **Edit** `internal/config/config.go`: add `MAIL_TASK_PROVIDER` (postmark|sendgrid|mailgun), `MAIL_TASK_WEBHOOK_SECRET`, `MAIL_TASK_USER_CORE_URL`, `MAIL_TASK_GATEWAY_INGEST_URL`, `MAIL_TASK_SERVICE_TOKEN`, `MAIL_TASK_IMPORTS_CORE_URL`.
- Idempotency: derive a dedup key from the provider's message-id header and drop repeats seen within a rolling window (same shape as `emailsync.go`'s cursor idea, simpler — a small TTL table or in-memory+DB fallback is enough since this is at-least-once webhook delivery, not a poll loop).

### 3.3 Model Plane — `session-core` (new thread origin)
- **Edit** `session-core/src/grpc.rs:2128`: `THREAD_ORIGINS = ["chat","space","agent_run","support","system","mail_task"]`.
- **Edit** `resolve_thread_origin` (`grpc.rs:2148-2170`): accept the caller-declared `"mail_task"` origin like `"chat"`/`"system"` (no `space_id` requirement).
- **New migration** `00xx_mail_task_thread_origin.sql`: extend the CHECK constraint added by `0034_thread_origin.sql` to include `'mail_task'`.
- No other session-core change needed — thread creation, the `session_key`-based idempotency (`grpc.rs:2076`, proven by the retry test at `:7130-7184`), and message-append all already work generically per origin.

### 3.4 Frontend Plane — `apps/gateway` (new orchestration endpoint, new file `src/domains/mail_task.rs`)
- `POST /internal/mail-task/ingest`, gated by the same HMAC-delegation pattern as `conversation-ingest-rs` (`x-service-id: integration-mail-task-worker`), body:
  ```jsonc
  {
    "org_id": "...", "user_id": "...",
    "provider_message_id": "...",           // → deterministic session_key
    "subject": "...", "body_text": "...",
    "attachments": [{"filename": "...", "extracted_text": "...", "extracted": true}],
    "occurred_at": "2026-09-28T..."
  }
  ```
  Steps: (1) call session-core `CreateThread{org_id, user_id, origin:"mail_task", session_key: "mail_task/"+sha256(org_id+provider_message_id), title: subject}`; (2) append the synthesized first user turn (subject + body + attachment blocks, each block wrapped the same way the SPA already delimits an attachment for the model) via the existing message-append call the gateway uses for a normal turn; (3) invoke execution-core for exactly one turn on that thread through the same path a live chat send uses, so plan/tool approval rungs (`ChatMessages.tsx:1791`) apply unchanged — no new "draft mode" flag needed; (4) on completion, publish `verevon.application.mail_task.drafted` `{id, type, org_id, user_id, thread_id, occurred_at}` on stream `VEREVON_APPLICATION` via the gateway's existing NATS client (`gdpr_nats.rs`) — content-free, matching the `conversationLifecycleEvent` shape at `notification-core/internal/consumers/conversation_followed_message.go:45-53`.
- **Edit** `apps/gateway/src/config.rs`: add the mail-task ingest HMAC secret + allowed-origin config.
- **Edit** the top-level router (wherever `chat`, `spaces`, etc. are `.merge()`d — same place `chat.rs` is wired) to mount `mail_task::router()`.

### 3.5 Application Plane — `notification-core` (new consumer)
- **New file** `internal/consumers/mail_task_drafted.go`, mirroring `conversation_followed_message.go` line for line: subscribe to `verevon.application.mail_task.drafted`, durable `notification-core-mail-task-drafted`, single recipient = the event's `user_id` (not a follower fan-out), notification type key `chat.mail_task_drafted` (in-app default on, email default off — implementer registers this key wherever `inbox.conversation_followed_message`'s default preference is registered, e.g. the migration analogous to notification-core's "009").
- **Edit** `internal/consumers` wiring (wherever `NewConversationFollowedMessageSubscriber` is constructed at startup) to also construct and start the new subscriber.

### 3.6 Frontend Plane — SPA
- **New file** `src/features/settings/components/MailTaskAddressSection.tsx`, pattern-matched on `CronSchedulesSection.tsx` (uses `useI18n()` / `tr('norsk','English')`, `createResource`): shows `<alias>@<domain>`, copy button, "Generer på nytt / Regenerate" (calls user-core's `DELETE` then re-`GET`s), a one-line explanation that only mail sent from the account's own verified address is accepted.
- **Edit** wherever the thread-history rail renders `ChatThreadSummary` (the file `history.rs:41-46`'s comment points at — needs a quick implementer grep for the SPA's history-rail row component, not traced here) to show a small "Fra e-post / From email" badge when `origin === "mail_task"`, exactly the affordance that comment already anticipated.
- **Edit** the chat message renderer: the synthesized first turn should render with a distinct "Opprettet fra e-post" header (sender, subject, attachment list with extraction status) instead of looking like a normal typed message — reuse whatever component already renders a Support-origin banner if one exists, otherwise a small new presentational component.

## 4. Tests to add

- `user-core`: `internal/mailtask/service_test.go` — alias generation is idempotent per `(org,user)`; revoke+regenerate invalidates the old alias immediately; resolve returns `ErrRevoked` distinctly from `ErrNotFound` (must not leak which case it is to an unauthenticated caller).
- `integration-corev2`: `internal/mailtask/receiver_test.go` — one fixture per provider (Postmark JSON, SendGrid multipart, Mailgun multipart) parses to the same `InboundMailTaskEvent`; `internal/mailtask/verify_test.go` — rejects when `AuthResult != "pass"`; rejects when `From` doesn't case-fold-match the resolved verified email; accepts the matching case; replay of the same `provider_message_id` is a no-op (asserted via a spy ingest client called exactly once).
- `attachments_test.go` — allow-listed type under 1MB gets `extracted:true` with text; xlsx/pptx gets `extracted:false` with filename preserved; oversized attachment gets `extracted:false`, never errors the whole message.
- `session-core`: extend the existing origin test table (pattern at `grpc.rs:7404-7495`) with `origin_accepts_mail_task_without_space_id` and a CHECK-constraint migration test (pattern at `0034`'s own test) confirming `'mail_task'` is now legal and an arbitrary 7th value still isn't.
- `apps/gateway`: `src/domains/mail_task.rs` unit tests for the HMAC-delegation gate (same four cases as `conversation-ingest-rs/src/lib.rs:857-887`: missing auth, legacy shared-key auth, valid-then-replay), plus an integration test (fake session-core + fake execution-core) asserting: (a) a write-capable tool call surfaces as a pending approval, never an executed side effect; (b) a retried `provider_message_id` resolves to the same `thread_id`; (c) the NATS publish payload is content-free (no subject/body).
- `notification-core`: `internal/consumers/mail_task_drafted_test.go`, mirroring `conversation_followed_message_test.go`'s structure — single recipient (not follower fan-out), respects the user's notification preference for the new type key, malformed event is logged and acked (never blocks the consumer).
- SPA: `MailTaskAddressSection.test.tsx` (copy/regenerate happy path + Norwegian strings), a `chat-history` rendering test for the new `origin==="mail_task"` badge.
- End-to-end (documented, not automated in CI given the live-latency-series constraint noted for this session): send a real email through the sandbox provider account → thread appears in `/chat` within the provider's typical webhook latency → drafted answer present → notification arrives → forged `From` with the alias is dropped with no thread created.

## 5. Rollout

- Services needing rebuild: `user-core` (new migration + endpoints), `integration-corev2` (new package + route), `session-core` **and** `model-gateway` (proto/enum touches session-core's schema, and the project's own precedent — the `correct_memory` PATCH — shows a session-core-adjacent change requires the downstream Rust service that embeds the generated proto to rebuild too), `apps/gateway` (new domain + NATS publish), `notification-core` (new consumer), frontend (`verevonv3` SPA).
- Use the project launcher for these rebuilds, not hand-built `docker compose build` (per this repo's own build convention — a manual compose build misses the Control Plane env chain and drifts).
- Flags: gate the whole feature behind one config/feature flag per org (e.g. `mail_task_enabled`) so it can ship dark and be enabled per pilot org before the mail-provider decision (§6.1) is finalized product-wide.
- No data migration beyond the two new tables/columns above; no backfill needed (feature is opt-in per user via the Settings section).

## 6. Risks and edge cases

- **Spoofing**: SPF/DKIM/DMARC "pass" alone is not sufficient if the sending domain itself is untrustworthy (e.g., attacker's own domain can pass SPF/DKIM for itself). The `From == account's verified email` check is the real anti-spoofing control; without it, anyone who discovers an alias could get a drafted answer to run against the org's own tools using an org member's identity. This is the single most important control in the design — flag it clearly to the implementer as non-optional for v1.
- **Alias discovery / enumeration**: aliases must be high-entropy (≥12 random bytes) and the resolve endpoint must return an identical response for "not found" and "revoked" to avoid confirming which aliases are live.
- **Provider retry storms**: inbound-parse webhooks are at-least-once; without the `session_key`-based idempotency this creates a duplicate thread (and a duplicate drafted-answer run) per retry — this is handled by session-core's existing session_key semantics, but the *execution-core turn* must also be made idempotent on the same key so a retry doesn't produce two draft answers in one thread.
- **Large or malformed MIME**: cap total message size early in the receiver (before any parsing) to avoid a memory-exhaustion vector from an inbound webhook that is, by definition, reachable by anyone who has the alias.
- **Runaway agent turn**: because this is an unattended trigger, any tool that is normally "safe to auto-run" for a live user (e.g. reads) still runs; if the model over-reaches (e.g., tries `mail.send`) it must hit the existing approval rung, not execute — this needs an explicit test (§4) rather than an assumption, since the approval gate's coupling to "there is a live user watching" has not been verified false in this codebase, only inferred from the durable-receipt design.
- **Cost**: an attacker who obtains a valid alias (or a user who forwards huge volumes of mail) can drive unbounded execution-core spend; needs a per-user rate limit (e.g., N mail-task runs / hour) in the gateway ingest endpoint.
- **Attachment content classification**: extracted attachment text flows into the model prompt exactly like the plan's other attachment path — should reuse whatever ZDR/residency handling already wraps a normal chat attachment (inference-core's provider/tier routing, `inference-core/src/provider/mod.rs:144,489-591`) rather than bypassing it; this should fall out naturally from routing the synthesized turn through the same execution path as any other turn, but the implementer should verify no residency/ZDR field is silently defaulted wrong for a system-synthesized turn (e.g. org-level ZDR setting must still be looked up from `org_id`, not assumed).
- **Multi-recipient / CC mail**: if a user CCs the alias on an email to a third party, the design as specified still creates a thread (From is the user's own verified address) — acceptable, but worth calling out since it differs from "forward" (a copy is created even though the user didn't only forward-for-a-task).
- **Known limit carried over unmodified**: xlsx/pptx attachments are not extracted (same imports-core allow-list gap as Wave 1); filenames are still surfaced so the user knows an attachment existed.

## 7. Genuine owner decisions

1. **Mail provider and receiving domain.** The system has zero existing inbound-mail infrastructure; this is a real, unavoidable choice (Postmark inbound, SendGrid Inbound Parse, Mailgun routes, or self-hosted SMTP) plus a domain to receive on (e.g. `tasks.verevon.ai`) — DNS/MX changes and a vendor contract are outside code. Recommendation given the codebase's existing HMAC-delegation-everywhere style and per-provider signature-verification precedent already in `server.go`: Postmark (simplest inbound-parse JSON contract, built-in spam/auth scoring) — but this is the owner's call.
2. **Sender-verification strictness.** v1 as specified requires `From` to exactly match the account's own verified email (self-forward only). If the product wants "anyone can email a colleague's task address" (closer to Manus Mail's literal wording "forwards or sends"), that needs a per-user "trusted senders" allow-list — a real scope increase (new data model, new Settings UI, new spoofing surface) that should be an explicit fast-follow decision, not bundled into v1.
3. **Attachment persistence.** v1 as specified only threads the *extracted text* through (ephemeral, matching imports-core's existing `/api/v1/import/extract` semantics) and does not persist the raw file for later download from the thread. Persisting the original file requires wiring into whatever blob store the chat composer's own attachment upload already uses — not traced in this pass — and is a reasonable v1.1 addition once that call site is confirmed.
4. **New origin vs. reusing `"agent_run"`.** This spec adds a 6th thread origin (`"mail_task"`) rather than overloading `"agent_run"`, so the client can show a distinct "from email" badge and so mail-task threads can get their own notification default independent of the general agent-run notification gap (Wave 3). If the team ships the general "background task with completion notice" binding first, this spec's scoped notification consumer (§3.5) becomes redundant and should be deleted in favor of the general one — call this out at implementation time so it isn't built twice.

## 8. Size and dependencies

- **Size: M.** Six services touch (user-core, integration-corev2, session-core, model-gateway rebuild, gateway, notification-core) plus SPA, but each change is small and additive (no schema rewrite, no existing endpoint's contract changes). Comparable to or slightly larger than "Schedule this from chat" (also Wave 2, also reuses existing cron/session-core plumbing).
- **Dependencies on other Wave-2 items:** none hard-blocking. Soft overlaps:
  - Shares the "trigger an unattended run + notify on completion" shape with **"Schedule this" from chat** and with the Wave-3 **"background-task notifications"** item — sequencing mail-task *after* whichever of those lands first would let it reuse a general completion-notification binding instead of the scoped consumer in §3.5.
  - Needs **owner decision #1** (mail provider) resolved before any Ingestion Plane code is written; everything else (alias table, session-core origin, gateway orchestration) can be built and tested against synthetic webhook fixtures in parallel.