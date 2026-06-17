# Wave 10 — Multi-channel agent deploy prompt

Self-contained brief for landing per-agent deployment to WhatsApp Business, Slack, and Messenger / Instagram inboxes.

---

## Context

Velion ships per-agent embed widgets (Wave 9) but agents are not yet reachable from external messaging channels. The closest competitive surface is Intercom Fin (Messenger/WhatsApp/email native) and Chatbase's "Connect" tab (WhatsApp/Slack/Messenger/Instagram).

Reference the gap doc at `apps/Frontend Plane/velion/docs/ui-ux-velion-gap.md` — especially §19 (Chatbase comparison) and the `integration:` tool namespace at `apps/Model Plane/rust/services/model-gateway/src/tool_registry.rs::make_integration_placeholder`.

### What's already in place

| Capability | Where |
|---|---|
| Agent record with `tools`, `model`, `systemPrompt`, `publicSecret` | `apps/Application Plane/convex-core/convex/agents.ts` + `schema.ts` |
| Public message-send (anonymous visitor → agent → SSE) | `apps/Frontend Plane/velion/src/app/api/embed/[agentId]/stream/route.ts` |
| Internal JWT mint for org-scoped service-to-service calls | `apps/Frontend Plane/velion/src/lib/model-plane/auth-token.ts::getModelPlaneTokenInternal` |
| Existing Ingestion Plane connector runtime | `apps/Ingestion Plane/docker-compose.yml` — `connector-runtime-engine` (port 3003) + Nango bridge |
| Integration namespace stub | `tool_registry.rs::make_integration_placeholder` (`integration:{connector}.{operation}`) returns clear "not yet wired" error |
| Webhook receiver pattern | `apps/Frontend Plane/velion/src/app/api/external/zammad/[...path]/route.ts` (Zammad support webhook) |

### What's NOT in place

- No `agent.channels` field on the agent record.
- No per-channel inbound webhook routes in velion.
- No outbound message senders (WhatsApp send-message, Slack chat.postMessage, Messenger Send API).
- No subscriber identity model (the embed widget uses a browser-side UUID; channels supply real platform user ids — phone numbers, Slack user ids, etc.).
- No threading model — channels carry "thread" semantics that we'd need to map onto our `session_key` shape.

---

## What to build

### 1. Convex schema additions

Add to the `agents` table (mirror Wave 9's pattern):

```ts
channels: v.optional(v.object({
  whatsapp: v.optional(v.object({
    phoneNumberId: v.string(),     // Meta WhatsApp Business Phone Number ID
    accessToken: v.string(),       // Meta Graph access token (write-only, never echoed back)
    verifyToken: v.string(),       // Generated per agent — sent to Meta in webhook setup
  })),
  slack: v.optional(v.object({
    teamId: v.string(),
    botToken: v.string(),          // xoxb- token from OAuth install
    signingSecret: v.string(),     // for webhook payload verification
    appId: v.string(),
  })),
  messenger: v.optional(v.object({
    pageId: v.string(),
    pageAccessToken: v.string(),
    appSecret: v.string(),
    verifyToken: v.string(),
  })),
  instagram: v.optional(v.object({
    igUserId: v.string(),
    pageAccessToken: v.string(),
  })),
})),
```

Plus a new top-level table `channelConversations`:

```ts
channelConversations: defineTable({
  agentId: v.id("agents"),
  orgId: v.id("organizations"),
  channel: v.union(v.literal("whatsapp"), v.literal("slack"), v.literal("messenger"), v.literal("instagram")),
  // External identity — phone number, Slack user, Messenger PSID, etc.
  externalUserId: v.string(),
  // The platform's thread/channel id (Slack channel, Messenger conversation, WhatsApp phone)
  externalThreadId: v.string(),
  // Our internal session_key the gateway uses for context threading
  sessionKey: v.string(),
  lastMessageAt: v.number(),
  createdAt: v.number(),
})
  .index("by_agent_channel_thread", ["agentId", "channel", "externalThreadId"])
  .index("by_org", ["orgId"]);
```

The `sessionKey` is `channel:{channel}:{agentId}:{externalThreadId}` — derived deterministically so the gateway's session-core keeps continuity across redeploys.

### 2. Inbound webhook routes (velion)

One route per channel under `apps/Frontend Plane/velion/src/app/api/channels/{channel}/webhook/route.ts`:

| Route | Verification | Body shape |
|---|---|---|
| `GET /api/channels/whatsapp/webhook` | Meta hub.verify_token challenge — echo `hub.challenge` if `hub.verify_token` matches the agent's `verifyToken` | n/a |
| `POST /api/channels/whatsapp/webhook` | HMAC-SHA256 of body against agent's `appSecret` (in `X-Hub-Signature-256`) | `entry[].changes[].value.messages[]` shape |
| `POST /api/channels/slack/events` | `x-slack-signature` HMAC verify | Events API envelope (`event.type === "message"`) |
| `GET /api/channels/messenger/webhook` | Same hub.verify_token as WhatsApp | n/a |
| `POST /api/channels/messenger/webhook` | `X-Hub-Signature-256` against page app secret | `entry[].messaging[].message` |
| `POST /api/channels/instagram/webhook` | Same as Messenger (uses same Meta webhook plumbing) | `entry[].messaging[].message` |

Per-webhook flow (same for all four):
1. Verify the signature → 403 on mismatch.
2. Look up which agent owns this channel binding (`agents:findByChannelTarget` — needs a new Convex query keyed on the `phoneNumberId` / `teamId` / `pageId`).
3. Find or create a `channelConversations` row → get `sessionKey`.
4. Mint an internal JWT scoped to the agent's org.
5. POST to model-gateway `/v1/invoke` with `{ content, session_key, system_prompt, model, tools, browse_web: true }` exactly like the embed flow does.
6. Take the response `content` and call the channel's send-message API:
   - WhatsApp: `POST https://graph.facebook.com/v18.0/{phoneNumberId}/messages` with `{messaging_product:"whatsapp", to, type:"text", text:{body}}`
   - Slack: `POST https://slack.com/api/chat.postMessage` with `{channel, text}`
   - Messenger: `POST https://graph.facebook.com/v18.0/{pageId}/messages?access_token=...` with `{recipient:{id:PSID}, message:{text}}`
   - Instagram: Same as Messenger but on the Instagram-graph path.

Total: 4 route files, each ~150 lines.

### 3. Outbound senders (shared helper)

Create `apps/Frontend Plane/velion/src/lib/channels/{whatsapp,slack,messenger,instagram}.ts`. Each exposes:

```ts
export async function sendMessage(
  binding: ChannelBinding,
  externalUserId: string,
  text: string,
): Promise<{ messageId: string }>
```

Use `fetch` + signed request shape per channel; throw on non-200 so the webhook handler can surface failures in the gap doc / metrics.

### 4. Per-agent UI: new "Channels" tab in `AgentWorkspaceView`

Pattern matches `EmbedTab` (Wave 9):
- 4 cards (WhatsApp / Slack / Messenger / Instagram), each with "Connect" button.
- Connect opens an OAuth flow (Slack + Messenger/Instagram use OAuth; WhatsApp uses a manual access-token paste).
- Connected card shows the bound channel id + a "Disconnect" button.
- "Test message" affordance per channel — sends "Connected to Velion ✓" through the channel as a smoke check.

Backend: new `useAgentChannels` hook calling `/api/agents/[id]/channels` (CRUD on `agent.channels`).

### 5. OAuth landing pages

Slack + Meta both need redirect URIs:
- `apps/Frontend Plane/velion/src/app/(dashboard)/agents/[agentId]/channels/oauth/[provider]/callback/page.tsx`
- Exchanges code → token, calls `/api/agents/[id]/channels` to persist.

### 6. Observability

- Emit `mp.v1.channel.{channel}.message.{inbound,outbound}` NATS events from each webhook handler — convex-subscriber already bridges model-plane-nats per W4-2, so we get free Convex mirroring.
- New `channelMessages` Convex table (mirror) → powers an Analytics tab row per channel.

### 7. Cost / security guards

- `CHANNELS_ENABLED` env in velion (master kill switch).
- Rate-limit inbound webhooks per-channel-per-minute (Cloudflare or in-process token bucket).
- WhatsApp: respect 24-hour customer-service window (after 24h with no inbound, the agent can only send "approved templates"). Reject outbound text-message attempts past the window with a clear UI error.

---

## Estimated scope

- Convex schema + new tables: 2 hours
- Inbound webhook routes (4 channels): 1.5 days
- Outbound senders (4 channels): 1 day
- OAuth flows (Slack + Meta): 1 day
- Channels tab UI + hook: 1 day
- Observability + Convex mirror: half day
- Cost/security guards + test: half day
- **Total**: 5-6 focused days.

## Constraints

- WhatsApp Business requires a Meta App + WhatsApp Business Account + verified phone number. The verification step takes 2-5 days at Meta's end; ship the code without waiting, document the WhatsApp onboarding in a separate runbook.
- Slack apps need Distribution approval if you want them in the public App Directory. Single-workspace install works immediately.
- Messenger/Instagram graph webhooks share infrastructure but each needs its own Facebook Page and app permissions.
- All four require a publicly accessible HTTPS endpoint — local dev needs ngrok or cloudflared.

## When done

- Update gap doc with `§20 Multi-channel deploy — closed` mirroring §19's shape.
- Rename this file to `wave10-channels.closed.md`.
- Add a §20 row to the tally.
