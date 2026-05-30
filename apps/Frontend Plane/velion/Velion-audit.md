# Velion Performance & UX Audit

Date: 2026-05-25  
Scope: `/Volumes/Lagring/Triodelab/CoreSystem/apps/Frontend Plane/velion`  
Method: CodeGraph structural scan, targeted source reads, production build, and official benchmark references. CodeGraph index was healthy at audit time: 2,953 files, 50,799 symbols, 116,951 edges.

Build measurement: `pnpm build` completed with Next.js 16.1.6 / Turbopack. Route client-reference payloads, computed from `.next/server/app/**/page_client-reference-manifest.js`: `/chat` 295 KB gzipped, `/inbox` 284 KB gzipped, `/knowledge` 271 KB gzipped. `.next/static` was 32 MB; several chunks were 3.4 MB raw. No Lighthouse/LCP browser trace was run in this pass.

## Top 7 Remediation Wins

| Rank | Win | Impact / effort | Primary files |
|---:|---|---|---|
| 1 | Keep chat composer state local and throttle draft persistence. | Highest INP/input-latency win; small refactor. | `src/components/chat/providers/ChatWorkspaceProvider.tsx`, `src/components/chat/components/ChatPage.tsx`, `src/components/chat/components/ChatInput.tsx` |
| 2 | Virtualize chat history, chat sidebar, and inbox ticket/article lists. | Major scroll and memory win; isolated UI work. | `src/components/chat/components/ChatView.tsx`, `src/components/core/sidebar/components/MessagesSidebarPanel.tsx`, `src/components/inbox/InboxWorkspacePage.tsx` |
| 3 | Add durable event IDs/resume for chat/embed/inbox SSE. | Prevents lost streams and reconnect gaps; medium backend work. | `src/app/api/chat/stream/route.ts`, `src/components/chat/api/orpc/chat.ts`, `public/embed.js`, `src/app/api/embed/[agentId]/stream/route.ts` |
| 4 | Replace content-based optimistic reconciliation with `clientId` persisted through Convex/Zammad. | Fixes duplicate/lost messages and slow perceived sends; medium contract work. | `src/components/chat/providers/ChatProvider.tsx`, `src/app/api/chat/stream/route.ts`, `src/components/inbox/InboxWorkspacePage.tsx` |
| 5 | Split dashboard-global providers and lazy-load heavy route surfaces. | Cuts JS on every dashboard route; high leverage after current build shows shared 271-295 KB gz payloads. | `src/app/(dashboard)/layout.tsx`, `src/app/layout.tsx`, `src/components/dashboard/GlobalSearchModal.tsx` |
| 6 | Add first-class error and stale-state banners instead of silent catch blocks. | Trust/UX win; low implementation cost. | `src/lib/api-client.ts`, `src/components/inbox/InboxWorkspacePage.tsx`, `src/components/core/navbar/hooks/useCorebar.ts`, `src/lib/api/search-api.ts` |
| 7 | Instrument Web Vitals, long tasks, SSE latency, and failed mutations. | Makes regressions fixable; low-to-medium effort. | `instrumentation.ts`, `src/lib/telemetry/client.ts`, `src/app/api/telemetry/events/route.ts` |

## Source-Backed Benchmark Expansion

Public benchmark note: the named products do not publish every internal UI implementation detail. Where an internal claim is not publicly documented, the benchmark below uses official product docs plus official platform/browser docs to define the observable product capability and the implementation pattern Velion should match.

| Dimension | Source-backed benchmark detail |
|---:|---|
| 1 | **Linear** publicly positions the keyboard as the fastest interaction path, with command-menu, J/K navigation, bulk selection, and view switching available from the keyboard. That raises the bar for Velion's hot path: composer keystrokes should stay local and not invalidate global dashboard state. The browser-side measurement standard is **INP**, which captures the time from user input until the next paint, and web.dev calls out delayed feedback as the core UX failure. Sources: [Linear concepts](https://linear.app/docs/conceptual-model), [Linear search](https://linear.app/docs/search), [web.dev INP](https://web.dev/inp/). |
| 2 | **React** now documents optimistic UI as a first-class hook with `useOptimistic`, and **Stripe** documents idempotency keys for safe retries on create/update calls. Together they define the expected pattern Velion lacks in places: immediate UI, stable client request IDs, reconciliation, and retry without duplicate side effects. Sources: [React useOptimistic](https://react.dev/reference/react/useOptimistic), [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests). |
| 3 | **OpenAI** streams Responses API output as typed server-sent events, including lifecycle events and text deltas, rather than replaying a completed answer. **Manus API** documents tasks as asynchronous runs where clients poll message/progress state, which is the durable-agent pattern Velion needs for tool/deep runs. **Intercom Fin** documents inline source links while answers are presented and configured human handoff behavior. Sources: [OpenAI streaming responses](https://platform.openai.com/docs/guides/streaming-responses), [OpenAI streaming events](https://platform.openai.com/docs/api-reference/responses-streaming), [Manus task.create](https://open.manus.ai/docs/api-reference/create-task), [Intercom Fin conversational experience](https://www.intercom.com/help/en/articles/11433030-conversational-fin-experience), [Intercom Fin handover](https://www.intercom.com/help/en/articles/10032299-use-fin-ai-agent-in-workflows). |
| 4 | **MDN** documents SSE `id:` frames and EventSource's last-event-id behavior as the native resume primitive. **Stripe**'s retry/idempotency docs show the adjacent server contract: clients must be able to safely retry without duplicating mutations. Velion needs both for streams and sends. Sources: [MDN using server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events), [MDN Server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events), [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests). |
| 5 | **Linear** publicly describes its realtime sync engine as central to the app experience, and its docs show repeated action patterns through command menu, keyboard, context menus, and bulk actions. That benchmark implies state updates must be scoped and selector-like; global context invalidation on hot surfaces is below the bar. Source: [Scaling the Linear Sync Engine](https://linear.app/blog/scaling-the-linear-sync-engine), [Linear conceptual model](https://linear.app/docs/conceptual-model). |
| 6 | **TanStack Virtual** documents headless virtualization for long vertical, horizontal, and grid-like lists while preserving implementation control. This is the concrete implementation benchmark behind GitHub/Stripe/Zendesk-scale list UX: mount only visible rows, not every message/ticket/session. Source: [TanStack Virtual introduction](https://tanstack.com/virtual/v2/docs). |
| 7 | **Next.js** documents automatic route chunking and prefetching, including hover-triggered prefetch patterns. **Airbnb Engineering** describes tracking real-user performance metrics rather than only load time, which supports prefetch/lazy-load decisions based on perceived navigation speed. Source: [Next.js prefetching guide](https://nextjs.org/docs/app/guides/prefetching), [Airbnb web performance measurement](https://medium.com/airbnb-engineering/measuring-web-performance-at-airbnb-122da8d3ea3f). |
| 8 | **web.dev** and **MDN** both document the animation budget rule Velion currently violates in long lists: animate `transform` and `opacity`, avoid layout/paint properties, and prefer browser-optimizable CSS where possible. This matches the Linear/Intercom style of snappy motion without per-row layout animation cost. Sources: [web.dev high-performance CSS animations](https://web.dev/articles/animations-guide), [web.dev animations and performance](https://web.dev/animations-and-performance/), [MDN CSS animation performance](https://developer.mozilla.org/en-US/docs/Web/Performance/Guides/CSS_JavaScript_animation_performance). |
| 9 | **MDN IntersectionObserver** explicitly lists lazy-loading and deciding whether to perform animation work based on visibility as use cases. **Three.js** documents explicit renderer/resource disposal. **Mapbox GL JS** documents a single `Map` object as the rendering surface for dynamic WebGL maps. The benchmark is visibility-gated WebGL with a small context count and cleanup. Sources: [MDN Intersection Observer API](https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API), [Three.js WebGLRenderer](https://threejs.org/docs/pages/WebGLRenderer.html), [Three.js cleanup](https://threejs.org/manual/en/cleanup.html), [Mapbox GL JS Map API](https://docs.mapbox.com/mapbox-gl-js/api/map/). |
| 10 | **Notion** now documents offline pages, automatic downloads for recent/favorite pages on paid plans, background updates, and offline editing for downloaded pages. **TanStack Query** documents persisted query caches. Velion's equivalent benchmark is cached-first paint with stale indicators and background refresh. Sources: [Notion offline pages](https://www.notion.com/help/use-pages-offline), [Notion offline guide](https://www.notion.com/en-gb/help/guides/working-offline-in-notion-everything-you-need-to-know), [TanStack Query persistence](https://tanstack.com/query/v4/docs/framework/persistQueryClient). |
| 11 | **Next.js** documents route-based JS splitting and background prefetch; **Intercom** publishes a Messenger install model that injects an async widget script; **Stripe Dashboard** documents keyboard-accessible dashboard navigation and integration health views. Velion's benchmark is not just "does it build," but route-local chunks, tiny embeds, and measured payload budgets. Sources: [Next.js prefetching](https://nextjs.org/docs/app/guides/prefetching), [Intercom JavaScript installation](https://developers.intercom.com/installing-intercom/docs/intercom-javascript), [Stripe Dashboard basics](https://docs.stripe.com/dashboard/basics). |
| 12 | **Stripe** documents Health Alerts for request failures, transaction trends, and latency; **Zendesk Agent Workspace** documents active conversation markers, chat-ended markers, and SLA visibility in the conversation header. The user-facing benchmark is explicit stale/error/reconnecting state instead of blank or silent failure. Sources: [Stripe health alerts](https://docs.stripe.com/health-alerts), [Zendesk managing conversations](https://support.zendesk.com/hc/en-us/articles/4408823962906-Managing-conversations-in-the-Zendesk-Agent-Workspace). |
| 13 | **GitHub**, **Linear**, and **Notion** all document command palettes or keyboard-first navigation. **GitHub** exposes site-wide, issue-list, PR, notification, and code-browsing shortcuts; **Linear** documents J/K navigation and command-menu actions; **Notion** documents slash commands and Cmd/Ctrl-K search. Velion should treat keyboard operation as a primary UI surface. Sources: [GitHub keyboard shortcuts](https://docs.github.com/get-started/using-github/keyboard-shortcuts), [GitHub Command Palette](https://docs.github.com/en/enterprise-cloud@latest/get-started/accessibility/github-command-palette), [Linear concepts](https://linear.app/docs/conceptual-model), [Notion keyboard shortcuts](https://www.notion.com/help/keyboard-shortcuts). |
| 14 | **Vercel Speed Insights** documents Core Web Vitals monitoring from real user data, and Vercel's Next.js docs show `useReportWebVitals` for custom analytics. **Chatbase** documents analytics for chats, messages, thumbs up/down, topics, and sentiment. Velion needs equivalent per-route INP/LCP/CLS plus AI/chat-specific latency and outcome telemetry. Sources: [Vercel Speed Insights](https://vercel.com/docs/speed-insights), [Next.js on Vercel Web Vitals](https://vercel.com/docs/concepts/next.js/overview), [Chatbase analytics](https://www.chatbase.co/docs/user-guides/chatbot/analytics). |
| 15 | **Next.js/Vercel** docs distinguish production build/runtime behavior, route chunking, and Core Web Vitals measurement. For parity, Velion needs a repeatable production-mode local perf command and type validation that matches deployment rather than webpack-only dev behavior. Sources: [Next.js prefetching/code splitting](https://nextjs.org/docs/app/guides/prefetching), [Vercel Speed Insights configuration](https://vercel.com/docs/speed-insights/package). |
| 16 | **Zendesk Agent Workspace** documents a unified ticket interface across email, chat, voice, social, and web messaging, with ticket tabs, notifications, routing, customer context, and SLA header visibility. **Intercom Inbox** publicly emphasizes configurable shared inbox, keyboard-first access, and macros. Velion's inbox should be the main multi-conversation work surface, not a single selected ticket view behind many placeholder routes. Sources: [Zendesk Agent Workspace overview](https://support.zendesk.com/hc/en-us/articles/360024218473), [Zendesk Agent Workspace resources](https://support.zendesk.com/hc/en-us/articles/4408827107226-Documentation-resources-for-the-Zendesk-Agent-Workspace), [Intercom Inbox](https://www.intercom.com/helpdesk/inbox). |
| 17 | **Zendesk** documents customer context on the right side of Agent Workspace. **Gorgias** documents Shopify customer/order data in the ticket view, Shopify actions such as refunds/cancellations/returns, and customer notes in the ticket sidebar. **Intercom** documents Shopify data sync and customer events. Velion's panel should be cached, identity-normalized, and commerce-aware. Sources: [Zendesk Agent Workspace overview](https://support.zendesk.com/hc/en-us/articles/360024218473), [Gorgias Shopify app](https://www.gorgias.com/apps/shopify), [Gorgias Shopify actions](https://docs.gorgias.com/en-US/shopify-actions-461552), [Gorgias customer notes](https://docs.gorgias.com/en-US/customer-notes-207758), [Intercom Shopify app](https://www.intercom.com/help/en/articles/16188-getting-started-with-the-intercom-shopify-app). |
| 18 | **Gorgias** documents macros that can insert standardized responses and perform Shopify actions, plus variables like `{{ticket.customer.firstname}}` and ecommerce fields. **Zendesk** documents slash-style shortcuts for common phrases. **Intercom Inbox** advertises powerful macros. Velion's macro benchmark is searchable insertion, variables, action side effects, and AI-suggested replies with feedback capture. Sources: [Gorgias macros](https://docs.gorgias.com/en-US/macros-101-81846), [Gorgias macro variables](https://docs.gorgias.com/en-US/macro-variables-101-81845), [Zendesk shortcuts](https://support.zendesk.com/hc/en-us/articles/4408832184346-Inserting-common-phrases-with-shortcuts), [Intercom Inbox](https://www.intercom.com/helpdesk/inbox). |
| 19 | **Intercom** documents an async Messenger script and a JavaScript `boot` method that lets SPAs control when Messenger initializes. **Chatbase** documents widget/iframe embedding, identity verification with HMAC, and per-user conversation continuation. Velion's embed benchmark is tiny loader, deferred boot/config/history, single auth handshake, true streaming, and safe conversation resume. Sources: [Intercom JavaScript installation](https://developers.intercom.com/installing-intercom/docs/intercom-javascript), [Intercom JavaScript methods](https://developers.intercom.com/installing-intercom/web/methods/), [Chatbase connect/embed](https://chatbase.mintlify.dev/docs/user-guides/chatbot/connect), [Chatbase user conversations](https://www.chatbase.co/docs/api-v2/user-conversations). |
| 20 | **Chatbase** documents website crawling, sitemap submission, individual URL ingestion, and 24-hour auto retrain on supported plans. **Intercom** documents AI Agent knowledge sources and source links in Fin answers. The RAG benchmark is source lifecycle management plus paragraph/article-level citations and escalation when confidence is insufficient. Sources: [Chatbase data sources](https://www.chatbase.co/docs/user-guides/chatbot/data-sources), [Intercom knowledge sources](https://www.intercom.com/help/en/articles/9440354-knowledge-sources-to-power-ai-agents-and-self-serve-support), [Intercom Fin conversational experience](https://www.intercom.com/help/en/articles/11433030-conversational-fin-experience). |
| 21 | **Zendesk** documents Agent Workspace as one interface for email, chat, voice, social messaging, and web messaging, with omnichannel routing and unified agent status. **Gorgias** positions its ecommerce helpdesk around not switching tabs and bringing customer/order data into the ticket view. Velion needs a canonical conversation aggregate across channel messages, not just Zammad-ticket proxies. Sources: [Zendesk Agent Workspace overview](https://support.zendesk.com/hc/en-us/articles/360024218473), [Zendesk omnichannel routing](https://support.zendesk.com/hc/en-us/articles/4409149119514-About-omnichannel-routing), [Gorgias Shopify helpdesk](https://www.gorgias.com/apps/shopify). |
| 22 | **Intercom** documents CSAT reports across teammates, AI agents, chatbots, ratings, response rate, and topics driving dissatisfaction; it also documents Fin outcomes and deflection-like resolution metrics. **Zendesk** documents CSAT ratings for solved tickets and satisfaction-rating APIs. **Chatbase** documents chat analytics, thumbs up/down, topics, and sentiment. Velion needs real conversation outcome events, ratings, and bot/handoff analytics instead of mock cards. Sources: [Intercom CSAT reporting](https://www.intercom.com/help/en/articles/10244420-customer-satisfaction-reporting), [Intercom reporting metrics](https://www.intercom.com/help/en/articles/7022438-reporting-metrics-attributes), [Intercom Fin outcomes](https://www.intercom.com/help/es/articles/8205718), [Zendesk CSAT resources](https://support.zendesk.com/hc/en-us/articles/4416863344026-CSAT-resources), [Zendesk Satisfaction Ratings API](https://developer.zendesk.com/api-reference/ticketing/ticket-management/satisfaction_ratings/), [Chatbase analytics](https://www.chatbase.co/docs/user-guides/chatbot/analytics). |

## Dimension Gap List

### 1. Input Latency (sub-16ms)

- **Verdict:** ❌ gap
- **Velion evidence:** Composer text is stored in dashboard-global context. `ChatPage` reads `message = getDraft(draftKey)` and calls `setDraft` on every keystroke (`src/components/chat/components/ChatPage.tsx:52-131`). `ChatWorkspaceProvider` updates a global `drafts` object for each keystroke and rebuilds the context value (`src/components/chat/providers/ChatWorkspaceProvider.tsx:71-79`, `171-236`). `ChatInput` also runs auto-resize, autocomplete trigger detection, debounced fetch setup, and entity pruning per keystroke (`src/components/chat/components/ChatInput.tsx:106-220`).
- **Reference:** Linear keeps hot input paths local and selector-scoped. ChatGPT and Intercom keep composer state independent from message-list/ticket-thread providers so typing does not invalidate the whole thread.
- **Remediation:**
  1. Move composer value into `ChatInput` local state; pass only `onSubmit(content, options)` upward.
  2. Persist drafts on blur, submit, route change, or a 300-500 ms idle debounce.
  3. Move autocomplete into a `useDeferredValue`/debounced hook and keep global context out of the keystroke loop.

### 2. Optimistic UI

- **Verdict:** ⚠️ partial
- **Velion evidence:** Chat has optimistic bubbles, but first-message send waits for `ensureSession()` to create a server session before rendering the optimistic message (`src/components/chat/providers/ChatProvider.tsx:432-471`). Reconciliation is by message content, not a durable client/server ID (`src/components/chat/providers/ChatProvider.tsx:314-319`, `340-349`). Session create/delete/title updates wait for server round trips (`src/components/chat/providers/ChatProvider.tsx:394-430`). Inbox replies are appended only after the Zammad POST returns (`src/components/inbox/InboxWorkspacePage.tsx:689-702`), while send failures are swallowed (`706-708`). Tag edits are locally optimistic but have no rollback/status (`736-750`).
- **Reference:** Notion renders edits before save and reconciles later. GitHub comments appear immediately with visible retry/error state. Intercom posts replies instantly with delivery status.
- **Remediation:**
  1. Generate local session/message IDs before network calls; render immediately.
  2. Persist `clientId` through `/api/chat/stream`, Convex messages, and support replies; reconcile by `clientId -> serverId`.
  3. Add per-message delivery states: sending, sent, failed, retry.

### 3. Real-Time Streaming

- **Verdict:** ⚠️ partial
- **Velion evidence:** Good: chat SSE is decoupled from generation; `safeEnqueue` swallows broken pipes and Convex writes continue (`src/app/api/chat/stream/route.ts:75-109`). Plain chat uses real gateway token deltas and batches Convex writes every 60 ms or 24 chars (`121-188`). Gap: tool/deep paths call a unary orchestrator, split a completed answer into fake chunks, then sleep 12 ms per chunk (`194-233`). Tool trace/run id are SSE-only and explicitly not persisted (`244-270`). Embed stream is not streaming: it waits for `/v1/invoke`, then emits one `answer_chunk` (`src/app/api/embed/[agentId]/stream/route.ts:135-184`). Inbox AI draft also buffers a unary answer, then replays words at 10 ms (`src/app/api/inbox/draft/route.ts:147-168`).
- **Reference:** ChatGPT keeps generation server-side past disconnect. Manus.ai streams durable step events through a bus. Intercom Fin streams answer tokens and citations as they form.
- **Remediation:**
  1. Make tool/deep/agent runs durable records with event rows: token, step, tool_call, citation, done, error.
  2. Stream orchestrator steps directly instead of word-replaying finished answers.
  3. Persist tool trace/run metadata enough for refresh/resume, with redaction for sensitive args.

### 4. Reconnect Resilience

- **Verdict:** ⚠️ partial
- **Velion evidence:** ChatProvider keeps last-known Convex payloads to avoid blanking during WebSocket reconnect (`src/components/chat/providers/ChatProvider.tsx:256-285`, `353-371`). Notification WS has exponential backoff and polling fallback (`src/lib/notifications/ws.ts:17-25`, `69-79`). Gaps: chat SSE responses have no `id:` frames or resume cursor (`src/app/api/chat/stream/route.ts:305-310`). Chat client stream parsing splits each decoded chunk by newline and does not preserve partial SSE frames (`src/components/chat/api/orpc/chat.ts:263-299`). Embed has the same no-resume model and a parser bug: `if (!line.indexOf('data: ') === 0)` is wrong operator precedence (`public/embed.js:210-215`).
- **Reference:** Stripe keeps tables populated during socket churn. Uber keeps last-known map state visible. Intercom inbox stays populated during reconnects.
- **Remediation:**
  1. Add monotonically increasing event IDs and `Last-Event-ID` resume for chat/embed streams.
  2. Store stream events in Convex/Postgres and replay from cursor on reconnect.
  3. Replace ad hoc parsers with a shared SSE parser that handles split frames.

### 5. State Management & Re-Render Scope

- **Verdict:** ❌ gap
- **Velion evidence:** The entire dashboard is a client layout that mounts `ChatProvider`, `ChatWorkspaceProvider`, global search, sidebar, top nav, and warmup on every dashboard route (`src/app/(dashboard)/layout.tsx:1-15`, `89-97`). `ChatProvider` exposes one large context value containing the whole `state` object (`src/components/chat/providers/ChatProvider.tsx:504-532`). `ChatPage` maps messages into new objects on every render (`src/components/chat/components/ChatPage.tsx:205-218`). `ChatView` maps all messages under `AnimatePresence` and `layout` wrappers (`src/components/chat/components/ChatView.tsx:992-1009`).
- **Reference:** Linear uses atom/selector stores. GitHub segments providers by region. Intercom splits inbox-list, ticket-thread, composer, and customer-panel stores.
- **Remediation:**
  1. Split stores into session list, active thread, composer, actor, and inbox/customer-context domains.
  2. Use selector-based subscriptions or a small external store for hot paths.
  3. Memoize message rows and pass primitive props; stop recreating message objects in `ChatPage`.

### 6. List Virtualization

- **Verdict:** ❌ gap
- **Velion evidence:** No virtualization package is present; `rg` found no `react-window`, `@tanstack/react-virtual`, `virtua`, or `useVirtual`. Chat history renders every message (`src/components/chat/components/ChatView.tsx:992-1009`). Sidebar chat history renders every filtered session (`src/components/core/sidebar/components/MessagesSidebarPanel.tsx:140-187`). Inbox ticket list and article thread render all rows (`src/components/inbox/InboxWorkspacePage.tsx:848-908`, `1056-1106`). Chat session list API returns all sessions with `totalCount: sessions.length` and no cursor (`src/app/api/chat/sessions/route.ts:13-24`; `src/app/api/chat/_lib/session-store.ts:654-660`).
- **Reference:** GitHub Issues, Stripe transactions, Zendesk Agent Workspace, and Intercom inbox virtualize large lists.
- **Remediation:**
  1. Add `@tanstack/react-virtual` or `virtua`.
  2. Virtualize message history, chat session list, inbox ticket list, and article thread.
  3. Add cursor pagination to `/api/chat/sessions` and `/api/support/tickets`.

### 7. Route Prefetching & Code Splitting

- **Verdict:** ⚠️ partial
- **Velion evidence:** `DashboardRouteWarmup` prefetches a fixed route set on idle (`src/components/dashboard/DashboardRouteWarmup.tsx:6-14`, `24-45`). Sidebar navigation only pushes on click; no hover/focus prefetch or query prefetch is present (`src/components/core/sidebar/index.tsx:138-160`). The dashboard layout imports chat, global search, sidebar, top nav, and connector/trust surfaces eagerly (`src/app/(dashboard)/layout.tsx:3-15`).
- **Reference:** Airbnb prefetches detail routes on card hover. GitHub prefetches likely PR/issues navigation. Zendesk prefetches conversation history on inbox-row intent.
- **Remediation:**
  1. Add `router.prefetch(item.href)` on sidebar item hover/focus and inbox row hover.
  2. Pair route prefetch with first data prefetch through TanStack Query/Convex.
  3. Lazy-import global search, chat providers, planner/knowledge/admin panels only where needed.

### 8. Animation Budget

- **Verdict:** ❌ gap
- **Velion evidence:** Each chat message is wrapped in Framer Motion `m.div layout` (`src/components/chat/components/ChatView.tsx:992-1009`), and assistant rows are `m.article` (`522-528`). Sidebar uses `transition-all` and width transitions (`src/components/core/sidebar/index.tsx:181-190`; `src/app/(dashboard)/layout.tsx:55-58`). `MotionProvider` is at root for all routes (`src/components/providers/MotionProvider.tsx:23-27`), which is useful for sharing config but makes motion a global concern.
- **Reference:** Linear animates only transform/opacity on hot UI. Intercom Messenger uses CSS transform/opacity for 60 fps slide-in.
- **Remediation:**
  1. Remove `layout` animations from long lists.
  2. Restrict transitions to `transform` and `opacity`; avoid width/layout animations in sidebars.
  3. Keep row hover states CSS-only and respect `prefers-reduced-motion`.

### 9. Asset & WebGL Discipline

- **Verdict:** ⚠️ partial
- **Velion evidence:** Good: repeated message avatars force CSS fallback to avoid many WebGL contexts (`src/components/chat/components/ChatView.tsx:531-536`). Three is lazy-imported (`src/components/chat/three/ThreeJSOrb.tsx:101-113`). Gap: each live orb creates its own canvas/renderer (`130-156`) and animates 900 particles every frame (`233-240`, `309-335`) with no IntersectionObserver visibility pause or central context budget.
- **Reference:** Airbnb Mapbox shares one map/WebGL context. ChatGPT voice mode owns the primary active canvas rather than spawning repeated contexts.
- **Remediation:**
  1. Add a central WebGL budget/provider and cap live orb instances.
  2. Pause RAF when offscreen or hidden using IntersectionObserver and `visibilitychange`.
  3. Prefer CSS fallback for typing indicators under low-power/reduced-motion.

### 10. Cache Hierarchy

- **Verdict:** ⚠️ partial
- **Velion evidence:** TanStack Query is configured with stale time, GC time, retry, and reconnect behavior (`src/components/auth/providers/QueryProvider.tsx:14-44`). Gap: no persisted cache/hydration plugin is configured (`63-92`). Knowledge hooks fetch with `cache: 'no-store'` and 30s stale only (`src/components/knowledge/hooks/useKnowledgeData.ts:11-27`, `29-67`). Chat HTTP endpoints are dynamic/no-store (`src/app/api/chat/sessions/route.ts:10-23`; `src/app/api/chat/sessions/[sessionId]/route.ts:13-41`). Convex provides live cache for chat but not a broader local-first cache.
- **Reference:** Notion opens pages from local cache. GitHub uses stale-while-revalidate patterns. Intercom opens cached conversations before refresh.
- **Remediation:**
  1. Add persisted TanStack Query cache for knowledge/inbox/settings using IndexedDB.
  2. Seed pages from cache immediately and show stale indicators while revalidating.
  3. Normalize support tickets/customers/articles into keyed query caches.

### 11. Bundle Size & Initial Load

- **Verdict:** ⚠️ partial
- **Velion evidence:** Production build succeeded. Client-reference payloads: `/chat` 295 KB gzipped, `/inbox` 284 KB gzipped, `/knowledge` 271 KB gzipped. `.next/static` was 32 MB, with three 3.4 MB raw chunks. Dependencies include heavy libraries: BlockSuite packages (`package.json:22-29`), `@react-three/fiber`/`three` (`51-52`, `75`), `framer-motion` and `motion` (`61`, `64`), `gsap` (`62`), Monaco (`32`), and wavesurfer (`76`). Dashboard-global client layout imports broad surfaces eagerly (`src/app/(dashboard)/layout.tsx:1-15`). Build skips TypeScript validation (`next.config.ts:5-7`).
- **Reference:** Linear targets very small initial JS for fast interactions. Stripe Dashboard code-splits by route. Intercom Messenger keeps the embed loader tiny.
- **Remediation:**
  1. Add a bundle analyzer/perf budget script to CI.
  2. Lazy-load BlockSuite, Monaco, Three, GSAP, wavesurfer, and chat-only providers.
  3. Remove duplicate `framer-motion`/`motion` dependency paths where possible and import per-symbol.

### 12. Error Surface & Graceful Degradation

- **Verdict:** ⚠️ partial
- **Velion evidence:** Generic `apiClient` has no default timeout or AbortSignal (`src/lib/api-client.ts:29-40`). Several UI paths swallow failures: inbox reply send (`src/components/inbox/InboxWorkspacePage.tsx:706-708`), ticket refresh/patch (`672-684`, `715-731`), quick reply generation (`255-270`), customer context (`422-426`), support agents/groups (`615-624`), upload fallback sends without attachments (`src/components/chat/components/ChatInput.tsx:370-390`). Notification/support routes often degrade to empty arrays (`src/app/api/support/notifications/route.ts:48-68`).
- **Reference:** Uber shows last-known data with stale status. Stripe swaps to retry cards. Zendesk keeps ticket open with reconnecting badges.
- **Remediation:**
  1. Add `fetchWithTimeout` and standardized retry/error envelope.
  2. Replace silent catches with inline banners, toast+retry, or stale badges.
  3. Preserve unsent inbox/chat drafts and failed attachments with retry controls.

### 13. Keyboard & Accessibility

- **Verdict:** ⚠️ partial
- **Velion evidence:** Global search supports Cmd/Ctrl-K and `/` outside editable targets (`src/components/dashboard/DashboardSearchContext.tsx:60-93`). Chat also binds Cmd/Ctrl-K and `/` to focus composer (`src/components/chat/components/ChatPage.tsx:64-85`), which conflicts conceptually with global search. Inbox reply supports Cmd/Ctrl-Enter send (`src/components/inbox/InboxWorkspacePage.tsx:1134-1143`). There is no full fuzzy command palette with nav/actions, no J/K/R/M inbox triage shortcuts, and mobile menu overlay uses a `div role="button"` instead of a semantic dialog/backdrop (`src/components/core/sidebar/index.tsx:170-178`).
- **Reference:** Linear, GitHub, and Notion center command palettes. Intercom supports keyboard inbox triage and fast reply workflows.
- **Remediation:**
  1. Convert global search into a command palette with fuzzy navigation and actions.
  2. Add inbox shortcuts: J/K next ticket, R reply, M macros, E close/escalate.
  3. Audit focus traps, labels, and semantic buttons/dialogs for dashboard overlays.

### 14. Observability

- **Verdict:** ❌ gap
- **Velion evidence:** `instrumentation.ts` only conditionally imports ORPC server code; no Web Vitals or tracing setup is present (`instrumentation.ts:1-13`). Client telemetry is limited to five auth/onboarding/dashboard events (`src/lib/telemetry/client.ts:40-46`) and posts to a logging sink (`src/app/api/telemetry/events/route.ts:37-43`, `71-85`). No INP/LCP/CLS, long-task observer, route timing, SSE first-token latency, failed mutation telemetry, or embed latency is recorded.
- **Reference:** Stripe and Vercel Analytics track per-route Web Vitals. Chatbase instruments embed load-to-interactive and end-to-end answer latency.
- **Remediation:**
  1. Add Web Vitals reporting for INP, LCP, CLS, TTFB, and route ID.
  2. Add PerformanceObserver for long tasks and event-timing outliers.
  3. Log SSE first byte, first token, completion, abort, retry, failed mutations, and embed TTFI.

### 15. Dev-Environment Parity

- **Verdict:** ⚠️ partial
- **Velion evidence:** Local dev forces webpack even though Next 16 defaults to Turbopack (`package.json:8`). Production build exists and succeeded (`package.json:9`), but TypeScript errors are ignored in production (`next.config.ts:5-7`). Build output explicitly skipped type validation. `next.config.ts` disables `optimizeCss` and webpack build worker (`19-22`). There is no perf script for production-mode local validation or Lighthouse.
- **Reference:** Vercel/Next production validation normally uses `next build && next start` plus bundle/perf checks close to production behavior.
- **Remediation:**
  1. Add `perf:build`, `perf:start`, and Lighthouse/WebPageTest scripts with fixed env.
  2. Remove `ignoreBuildErrors` or gate it behind an emergency env only.
  3. Use Turbopack dev by default unless a documented webpack-only blocker exists.

### 16. Inbox / Multi-Conversation Surface

- **Verdict:** ⚠️ partial
- **Velion evidence:** There is a dedicated inbox route (`src/app/(dashboard)/inbox/[[...slug]]/page.tsx:8-15`) and a workspace component. Filters are only `all/open/pending/solved` (`src/components/inbox/InboxWorkspacePage.tsx:131-139`, `817-831`), while sidebar nav advertises many routes/channels such as WhatsApp/email/unassigned/AI states (`src/components/core/sidebar/config/nav-items.ts:337-466`) that still all render the same client page (`src/app/(dashboard)/inbox/[[...slug]]/InboxSectionPageClient.tsx:13-21`). The workspace holds one `selectedTicket` and one `replyText` in local component state (`src/components/inbox/InboxWorkspacePage.tsx:568-577`), so there are no multi-ticket tabs/panes. Ticket order is fetched page 1 only and not updated from a realtime ticket event (`590-611`).
- **Reference:** Intercom and Zendesk Agent Workspace make inbox the home screen with assigned/unassigned/mine/closed/snoozed queues, multi-pane workflows, and stable scroll on live updates.
- **Remediation:**
  1. Implement route-aware filters for mine/unassigned/closed/snoozed/channel/AI states.
  2. Model open tickets as tab/pane state keyed by ticket ID.
  3. Subscribe to ticket events and update list order without resetting scroll.

### 17. Customer / Context Side-Panel

- **Verdict:** ⚠️ partial
- **Velion evidence:** Customer panel exists and shows email/name, Shopify recent orders, and Stripe subscription (`src/components/inbox/InboxWorkspacePage.tsx:406-550`). It lazy-fetches per ticket (`410-429`) and the server pulls Shopify/Stripe with timeouts (`src/app/api/support/customers/[customerId]/context/route.ts:40-105`). Gaps: no TanStack Query cache, no past conversations/tags/properties/events, customer ID is Zammad customer ID but Stripe branch treats it as email or `cus_` ID (`70-87`), so Stripe lookup is likely wrong for many customers.
- **Reference:** Gorgias shows orders/refunds/returns inline. Intercom shows user attributes and event history. Zendesk has a persistent user sidebar.
- **Remediation:**
  1. Move context fetch into `useQuery(['customerContext', customerId, orgId])`.
  2. Add past conversations, tags, properties, lifecycle events, refunds/returns.
  3. Store canonical customer identity mappings across Zammad, Shopify, Stripe, email.

### 18. Macros, Shortcuts, Quick Replies

- **Verdict:** ⚠️ partial
- **Velion evidence:** `MacrosPanel` loads and executes Zammad macros (`src/components/inbox/MacrosPanel.tsx:24-69`, `105-121`). AI quick replies are generated manually and inserted on click (`src/components/inbox/InboxWorkspacePage.tsx:247-345`, `1183-1201`). AI draft streams into the reply box (`src/components/inbox/useAiDraft.ts:56-121`). Gaps: no slash/hotkey macro insertion, no `{{customer.first_name}}` variable preview/resolution in Velion, no macro search, no visible retry/error for quick-reply failures (`266-268`), and draft feedback route exists but is not wired from send flow in the evidence read.
- **Reference:** Intercom and Zendesk macros support fast insertion and dynamic content. Gorgias combines templates with automation rules and customer/order context.
- **Remediation:**
  1. Add `/macro` or Cmd/Ctrl-M palette in the reply composer.
  2. Resolve template variables client-side before send and show preview.
  3. Persist accepted AI drafts through `/api/inbox/draft/feedback` on send.

### 19. Embed Widget Performance & Install Size

- **Verdict:** ⚠️ partial
- **Velion evidence:** Public widget is a single vanilla JS file, 10,702 bytes raw and 3,843 bytes gzipped (`public/embed.js`). It fetches config immediately on script load (`public/embed.js:83-98`), mounts a shadow DOM bubble/panel (`100-156`), and sends messages to `/api/embed/{agentId}/stream` (`181-236`). The install snippet uses one deferred script (`src/components/agents/hooks/useAgentEmbed.ts:107-117`). Gaps: config/auth handshake happens at page load instead of first open, no history hydration, no per-visitor rate limit yet (`src/app/api/embed/[agentId]/stream/route.ts:30-31`), stream endpoint is unary replay (`135-184`), and parser bug at `public/embed.js:214`.
- **Reference:** Chatbase and Intercom Messenger keep loaders tiny and lazy-load conversation pane/history only when opened.
- **Remediation:**
  1. Mount only the button at script load; fetch config/history on first open or idle.
  2. Fix parser condition to `if (!line.startsWith('data: ')) continue`.
  3. Add per-origin/per-visitor rate limits, history hydration on open, and true streaming endpoint.

### 20. Knowledge-Base Ingestion & RAG Quality

- **Verdict:** ⚠️ partial
- **Velion evidence:** File/text ingestion exists (`src/app/api/knowledge/documents/route.ts:37-80`). Website crawl supports discover and commit flow (`src/app/api/ingestion/crawl/route.ts:23-100`; `src/app/api/ingestion/crawl/[jobId]/commit/route.ts:14-64`). Manual reindex/retrain exists for dirty docs (`src/app/api/knowledge/retrain/route.ts:15-56`; `src/components/knowledge/KnowledgeShell.tsx:169-183`). Q&A exists but comments say retrieval pickup is future work (`src/components/knowledge/modals/QnAModal.tsx:14-19`). Search uses model-gateway web search, not clearly org RAG, and emits one chunk with no source list (`src/app/api/ai/search/route.ts:60-99`). `CitationCard` can display source cards (`src/components/search/CitationCard.tsx:22-75`), but search API fallback can generate mock sources (`src/lib/api/search-api.ts:71-112`). Confidence is normalized to `0` in reasoning envelopes (`src/lib/model-plane/reasoning.ts:123-130`, `290-299`); no answer confidence threshold or "I don't know/escalate" fallback is implemented in the audited RAG path.
- **Reference:** Chatbase auto-crawls/sitemaps and reindexes sources. Intercom Fin cites the exact help-center material behind the answer and falls back when confidence is low.
- **Remediation:**
  1. Separate org-knowledge retrieval from public web search and return paragraph-level citations.
  2. Add confidence thresholds and an explicit escalate-to-human fallback.
  3. Add scheduled/change-detection reindex for crawled/integration sources, not only manual dirty retrain.

### 21. Multi-Channel Unification

- **Verdict:** ❌ gap
- **Velion evidence:** Sidebar advertises Messenger, Instagram, WhatsApp, email, Twitter/X, SMS routes (`src/components/core/sidebar/config/nav-items.ts:361-367`), but inbox page ignores the slug and renders the same Zammad ticket workspace (`src/app/(dashboard)/inbox/[[...slug]]/InboxSectionPageClient.tsx:21`). Support ticket APIs proxy Zammad tickets/articles/macros only (`src/app/api/support/tickets/route.ts:43-61`; `src/app/api/support/tickets/[id]/articles/route.ts:12-62`). There is no audited unified conversation identity model tying web chat, email, WhatsApp, Slack, etc. to one customer/thread.
- **Reference:** Zendesk and Gorgias unify channel events under one customer/ticket timeline. Intercom Messenger and email replies can live in one conversation thread.
- **Remediation:**
  1. Define a `conversation` aggregate with participants, customer identity, channel messages, and external IDs.
  2. Add channel-specific renderers for email signatures, WhatsApp attachments/templates, social comments, and web-chat messages.
  3. Route Zammad tickets into the aggregate instead of treating Zammad as the only inbox model.

### 22. Deflection, Analytics, CSAT

- **Verdict:** ❌ gap
- **Velion evidence:** Support reports compute open/pending/solved/total and average response time, but hard-code `csatScore: null` (`src/app/api/support/reports/route.ts:139-147`). AI summary treats CSAT as possibly unavailable (`src/app/api/support/reports/ai-summary/route.ts:21-38`). Product-section cards show mock deflection/CSAT values (`src/components/dashboard/product-section-pages.tsx:420-442`). Chat feedback is local UI state/localStorage-like behavior in `ChatView` and agent playground rating exists for model runs, but no per-conversation customer rating widget feeding support analytics was found. `rg` found no implemented CSAT/NPS/deflection outcome pipeline beyond mock/report placeholders.
- **Reference:** Chatbase dashboards conversation outcomes. Intercom and Zendesk ship CSAT/deflection/response-time analytics out of the box.
- **Remediation:**
  1. Add conversation outcome events: bot_resolved, handed_off, abandoned, reopened.
  2. Add post-conversation CSAT widget and persist score/comment to support analytics.
  3. Build reports from real outcome/CSAT events and remove mock deflection cards.

## Reference Links

Core React/Next/browser performance:

- React optimistic UI: https://react.dev/reference/react/useOptimistic
- Next.js prefetching and route chunking: https://nextjs.org/docs/app/guides/prefetching
- MDN Server-Sent Events: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events
- MDN using Server-Sent Events, including event IDs: https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events
- web.dev INP: https://web.dev/inp/
- web.dev high-performance CSS animations: https://web.dev/articles/animations-guide
- web.dev animations and performance: https://web.dev/animations-and-performance/
- MDN CSS/JavaScript animation performance: https://developer.mozilla.org/en-US/docs/Web/Performance/Guides/CSS_JavaScript_animation_performance
- MDN Intersection Observer API: https://developer.mozilla.org/en-US/docs/Web/API/Intersection_Observer_API
- web.dev browser-level image lazy loading: https://web.dev/articles/lazy-loading

Data, cache, virtualization, observability:

- TanStack Virtual: https://tanstack.com/virtual/v2/docs
- TanStack Query persistence: https://tanstack.com/query/v4/docs/framework/persistQueryClient
- Vercel Speed Insights: https://vercel.com/docs/speed-insights
- Vercel Speed Insights package: https://vercel.com/docs/speed-insights/package
- Vercel / Next.js Web Vitals reporting: https://vercel.com/docs/concepts/next.js/overview

AI/chat streaming and durable agent patterns:

- OpenAI streaming responses: https://platform.openai.com/docs/guides/streaming-responses
- OpenAI Responses streaming events: https://platform.openai.com/docs/api-reference/responses-streaming
- Vercel AI SDK chat persistence: https://ai-sdk.dev/docs/ai-sdk-ui/chatbot-message-persistence
- Vercel AI SDK resume streams: https://ai-sdk.dev/docs/ai-sdk-ui/chatbot-resume-streams
- Manus async task API: https://open.manus.ai/docs/api-reference/create-task
- Manus API introduction: https://open.manus.im/docs

Keyboard-first and local-first product benchmarks:

- Linear conceptual model and command menu: https://linear.app/docs/conceptual-model
- Linear search and keyboard shortcuts: https://linear.app/docs/search
- Linear issue selection and J/K navigation: https://linear.app/docs/select-issues
- Linear sync engine: https://linear.app/blog/scaling-the-linear-sync-engine
- Notion offline pages: https://www.notion.com/help/use-pages-offline
- Notion offline guide: https://www.notion.com/en-gb/help/guides/working-offline-in-notion-everything-you-need-to-know
- Notion keyboard shortcuts and slash commands: https://www.notion.com/help/keyboard-shortcuts
- GitHub keyboard shortcuts: https://docs.github.com/get-started/using-github/keyboard-shortcuts
- GitHub Command Palette: https://docs.github.com/en/enterprise-cloud@latest/get-started/accessibility/github-command-palette
- GitHub code navigation/search: https://docs.github.com/repositories/working-with-files/using-files/navigating-code-on-github

Dashboard/reliability/product operations benchmarks:

- Stripe Dashboard basics and Workbench visibility: https://docs.stripe.com/dashboard/basics
- Stripe idempotent requests: https://docs.stripe.com/api/idempotent_requests
- Stripe health alerts: https://docs.stripe.com/health-alerts
- Stripe Apps viewports: https://docs.stripe.com/stripe-apps/reference/viewports
- Airbnb web performance measurement: https://medium.com/airbnb-engineering/measuring-web-performance-at-airbnb-122da8d3ea3f
- Uber Engineering real-time marketplace overview: https://eng.uber.com/

Customer-support and chatbot benchmarks:

- Intercom JavaScript Messenger installation: https://developers.intercom.com/installing-intercom/docs/intercom-javascript
- Intercom JavaScript methods and boot control: https://developers.intercom.com/installing-intercom/web/methods/
- Intercom Inbox: https://www.intercom.com/helpdesk/inbox
- Intercom Fin conversational experience and sources: https://www.intercom.com/help/en/articles/11433030-conversational-fin-experience
- Intercom Fin handover in workflows: https://www.intercom.com/help/en/articles/10032299-use-fin-ai-agent-in-workflows
- Intercom Fin procedures handoff/escalation: https://www.intercom.com/help/en/articles/13449439-building-fin-procedures
- Intercom knowledge sources: https://www.intercom.com/help/en/articles/9440354-knowledge-sources-to-power-ai-agents-and-self-serve-support
- Intercom Shopify app/customer data: https://www.intercom.com/help/en/articles/16188-getting-started-with-the-intercom-shopify-app
- Intercom CSAT reporting: https://www.intercom.com/help/en/articles/10244420-customer-satisfaction-reporting
- Intercom reporting metrics and deflection/CSAT definitions: https://www.intercom.com/help/en/articles/7022438-reporting-metrics-attributes
- Intercom Fin outcomes: https://www.intercom.com/help/es/articles/8205718
- Zendesk Agent Workspace overview: https://support.zendesk.com/hc/en-us/articles/360024218473
- Zendesk Agent Workspace resources: https://support.zendesk.com/hc/en-us/articles/4408827107226-Documentation-resources-for-the-Zendesk-Agent-Workspace
- Zendesk managing conversations in Agent Workspace: https://support.zendesk.com/hc/en-us/articles/4408823962906-Managing-conversations-in-the-Zendesk-Agent-Workspace
- Zendesk Agent Workspace composer: https://support.zendesk.com/hc/en-us/articles/4408831849882-Composing-messages-in-the-Zendesk-Agent-Workspace
- Zendesk omnichannel routing: https://support.zendesk.com/hc/en-us/articles/4409149119514-About-omnichannel-routing
- Zendesk shortcuts/common phrases: https://support.zendesk.com/hc/en-us/articles/4408832184346-Inserting-common-phrases-with-shortcuts
- Zendesk CSAT resources: https://support.zendesk.com/hc/en-us/articles/4416863344026-CSAT-resources
- Zendesk Satisfaction Ratings API: https://developer.zendesk.com/api-reference/ticketing/ticket-management/satisfaction_ratings/
- Gorgias Shopify helpdesk/customer context: https://www.gorgias.com/apps/shopify
- Gorgias macros: https://docs.gorgias.com/en-US/macros-101-81846
- Gorgias macro variables: https://docs.gorgias.com/en-US/macro-variables-101-81845
- Gorgias Shopify actions: https://docs.gorgias.com/en-US/shopify-actions-461552
- Gorgias return flow: https://docs.gorgias.com/en-US/configure-return-flow-%28automatic-response%29-368442
- Gorgias customer notes: https://docs.gorgias.com/en-US/customer-notes-207758
- Chatbase data sources, crawling, sitemap, auto retrain: https://www.chatbase.co/docs/user-guides/chatbot/data-sources
- Chatbase connect/embed: https://chatbase.mintlify.dev/docs/user-guides/chatbot/connect
- Chatbase user conversations/history: https://www.chatbase.co/docs/api-v2/user-conversations
- Chatbase analytics: https://www.chatbase.co/docs/user-guides/chatbot/analytics

WebGL/map discipline:

- Three.js WebGLRenderer: https://threejs.org/docs/pages/WebGLRenderer.html
- Three.js cleanup/dispose: https://threejs.org/manual/en/cleanup.html
- Mapbox GL JS: https://docs.mapbox.com/mapbox-gl-js/
- Mapbox GL JS Map API: https://docs.mapbox.com/mapbox-gl-js/api/map/
