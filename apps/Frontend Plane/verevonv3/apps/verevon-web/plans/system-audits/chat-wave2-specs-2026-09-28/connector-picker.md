# Implementation spec: attach-from-connected-source picker in the composer

Repo root `C:/dev/CoresSystem`. Read-only research; no files were changed. Citations are `path:line`.

---

## 1. What exists today

**Composer entry point — read-only, not an attach flow.**
`apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx`
- The gear ("Innstillinger") menu has a `SettingsView` state machine (`"main" | "skills" | "connectors"`, line 191) opened via `openSettingsPanel` (1818‑1845), portaled at 3374‑3392.
- Its `"main"` view lists four rows (4356‑4404): **Legg til filer eller bilder** (`onAddFiles`, opens the hidden `<input type="file">` at 2832‑2847 whose `accept` list is `image/*,.pdf,.docx,.xlsx,.pptx,.txt,.md,.markdown,.csv,.json,.html,.htm`, 2838), **Ta et skjermbilde**, **Ferdigheter**, and **Koblinger** (4394‑4404) which switches to `view()==="connectors"`.
- The `"connectors"` view (4430‑4450) renders `RemoteSettingsList` (4497‑4591) against `GET /api/v1/integrations/providers` — a **status list only** (name + "Klar"/"Åpne" per `RemoteSettingsItemRow`, 4593‑4619). No item is clickable, nothing can be picked or attached from it. This is exactly the gap the plan cites at `DashboardComposer.tsx:4394-4449`.
- "Connector" already means something else in this file: `ComposerActiveAction.kind === "connector"` (256‑260, rendered 3004‑3038) is an *activated tool* chip for the agent loop, unrelated to attaching a document. The new picker must not reuse this name/type.

**Local-file attachment pipeline (the shape a connector attachment must fit into).**
- `ComposerFile` (153‑159: `id,name,size,type,url`) is the composer's in-memory shape; `addFiles` (1482‑1510) turns a browser `File` into one via `createFilePreviewUrl` (blob: URL) and pushes it onto `files` (signal at 828).
- `DashboardComposerAttachment` (246‑253) adds `extractedText?`; `ComposerFile` and it are structurally the same shape used both for the chip list (`AttachmentPreview`, 3839‑3930) and the submit payload (`DashboardComposerSubmitPayload.attachments`, 262‑301).
- On submit, `submitComposer()` calls `prepareChatAttachments` (`src/features/chat/lib/prepare-chat-attachments.ts:6-36`), which re-reads every attachment: rejects anything `>1_000_000` bytes (line 13), and for `.pdf|.docx|.xlsx|.pptx` **unconditionally** calls `extractChatDocument` (line 22) — it does not check whether `extractedText` is already set.
- `extractChatDocument` (`src/shared/api/chat-client.ts:20-24`) posts `{filename, content_base64, content_type}` to `POST /api/v1/chat/documents/extract`, which the gateway proxies (`apps/gateway/src/domains/chat/documents.rs:14-31`) to imports-core's `POST /api/v1/import/extract`. That handler (`apps/Ingestion Plane/imports-core/app/main.py:204-212` → `app/chat_extract.py:12-36`) is a hard, unconditional ceiling: **1,000,000 bytes** of decoded content (line 23), **60,000 chars** of extracted text (line 34), extension-dispatched (not MIME-dispatched), ephemeral (`"persisted": false`, `Cache-Control: no-store`).
- `handleComposerSubmit` (`src/features/chat/components/use-chat-controller.ts:2730-2772`) then calls `toStreamAttachments` and `inlineTextAttachments` (`chat-normalizers.ts:1173-1196`, `:1128-1153`). Images become `data_base64` on the wire (`ChatAttachment`, `chat-client.ts:44-49`); everything else with usable text is **inlined into the message body** as a fenced block (`appendInlinedAttachments`, `chat-normalizers.ts:1160-1163`: `--- VEDLEGG: name ---\n…\n--- SLUTT PÅ VEDLEGG ---`), because the async retrieval route (`POST /api/v1/chat/documents`) is asynchronous and would show the model 0-1 chunks of the file in the very turn that attached it (comment at `use-chat-controller.ts:2734-2739`). `textIngestibleAttachments` (`chat-normalizers.ts:1090-1102`) treats anything `type.startsWith('text/')` or with a recognized text extension as ingestible with **no extraction call needed** — this matters for the design below.
- The turn's attachments are persisted as part of the ordinary turn record and re-rendered by `AttachmentChips`/`AttachmentItem` (`src/features/chat/components/ChatMessages.tsx:2163-2224`) and by the composer's own `TurnReceipt` (`DashboardComposer.tsx:3940-3979`, "N vedlegg lagt til").

**Server-side listing already exists (agent-loop path only, not composer-reachable).**
`apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:99-115` catalogs, per the plan, `mail.messages` (Microsoft), `gmail.messages`, `drive.files` (both providers) as read-only agent tools, calling `integration-corev2` server-to-server. This is a **separate call path** from the composer: execution-core → integration-corev2, driven by the model's own tool choice, not by a person clicking a file in a picker. The gateway (BFF) currently exposes **no browser-reachable route at all** into `integration-corev2`'s action executor — `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations.rs:16-90` proxies only `providers`, `connect-sessions`, `connections` (metadata/sync/disconnect), and `sync-jobs`. There is no `.../connections/{id}/actions` passthrough for the browser today.

**integration-corev2's actual read operations** (`apps/Ingestion Plane/integration-corev2/internal/actions/service.go`):
- Microsoft (`executeMicrosoft`, 96‑125): `mail.messages` (104‑109, Graph `/me/messages` with `$select=id,subject,from,receivedDateTime,webLink` — **no body**), `drive.files` (110‑115, Graph `/me/drive/root/children` with `$select=id,name,webUrl,file,folder,lastModifiedDateTime` — **root folder only, no search, no size field**), `calendar.events` (101‑103, Graph `/me/events`, **no `$select` trim → full event body/attendees/location already included**).
- Google (`executeGoogle`, 167‑193): `gmail.messages` (172‑174, Gmail `users.messages.list` — **id + threadId only**, Gmail's list endpoint never returns a body regardless of params), `drive.files` (184‑189, Drive `files.list` with `fields=files(id,name,mimeType,webViewLink,modifiedTime)` — **no `q` search param, no size field**), `calendar.events` (181‑183, Calendar `events.list`, **full event resource, no trimming**).
- All of these go through `POST /api/v1/connections/{id}/actions` (`internal/api/server.go:506-516`), gated by `executeConnectionAction` (2923‑3091): `auth.AssertOrgAccess` (2932), `requireActionCapability` (2935 → 3159‑3173, looks up `requiredCapabilityForOperation`, 3266‑3325), write-approval + attestation only when `actionRequiresApproval` (2939‑2944 → 3455‑3517) — **all of `mail.messages`/`gmail.messages`/`drive.files`/`calendar.events` are unlisted there and so classify as reads, no approval needed** (confirmed by the existing test `mailread_operations_are_catalogued_as_reads_never_writes`, `integration_tools.rs:852-869`, and mirrored server-side). Every call (read or write) gets an audit event (`connection.action.executed`, 3079‑3088).

**Scopes already granted — no new consent screen needed for content.** `internal/providers/catalog.go`:
- Microsoft `sharepoint.read` (228‑232) grants `Files.Read.All`+`Sites.Read.All` — already required for today's `drive.files` listing (`requiredCapabilityForOperation` maps `drive.files`→`sharepoint.read`, `server.go:3277-3278`) and already sufficient to download OneDrive/SharePoint file **content**, not just metadata.
- Microsoft `mail.read` (254‑259, `Mail.Read`) is already required for `mail.messages` and is sufficient for reading a full message body too.
- Google `drive.read` (424‑429, `drive.readonly`) is already required for `drive.files` (`server.go:3303-3304`) and is sufficient for `files.get?alt=media` content download and `files.export` for native Docs/Sheets/Slides.
- Google `gmail.read` (431‑436, `gmail.readonly`) is already required for `gmail.messages` and covers `users.messages.get?format=full`.
- **This is the key finding that separates this item from the plan's "calendar write / Excel / contacts" bucket: reading file/message content needs zero new OAuth scopes and zero admin-consent step.** The only two things missing are (a) code that calls the content/detail endpoints, and (b) a browser-reachable route to trigger it.

**Prior art for full-body fetch already lives in this repo, in a different subsystem.** `internal/emailsync/` is the async inbox-sync pipeline (`inbox-sync`/`inbox-history` routes) and already fetches full bodies: Graph `graph.go:50` selects `...,body`; Gmail `gmail.go:226` calls `format=full`; HTML bodies are turned into plain text by `stripHTMLTags` (`internal/emailsync/teams.go:646`, unexported — same module, different package, cannot be imported by `internal/actions` without exporting it). This is the pattern to mirror for the new operations below, not the code to import directly (different lifecycle/auth: emailsync runs as a background sync, not a per-request executor).

**No binary-content plumbing exists yet.** `service.go`'s only HTTP helpers (`getBearer` 1097‑1108, `postBearer` 1147‑1163, `doJSON` 1291‑…) assume a JSON response and read the whole body unbounded. Graph's `/content` and Drive's `alt=media`/`files.export` return raw bytes, not JSON — this is genuinely new code, not a rewire.

**A concrete transport hazard for OneDrive/SharePoint content:** `NewService` builds its `httpClient` via `egress.SafeClient(egress.ClientConfig{RequestTimeout: defaultActionTimeout})` (`service.go:47-55`) — the **zero-value config**, and `egress.SafeClient`'s zero value sets `AllowRedirects: false` (`internal/egress/client.go:36-41, 90-100`), so `CheckRedirect` returns `http.ErrUseLastResponse` and 3xx responses are **not followed**. Microsoft Graph's `/me/drive/items/{id}/content` frequently answers with a `302` to a pre-authenticated download URL on a different host (`*.sharepoint.com` / a CDN). With today's client this would silently return an empty/redirect body instead of the file. Google's `alt=media` and `files.export` do not redirect, so this risk is Microsoft-specific.

---

## 2. Design and exact change list

### 2.1 Scope decision baked into the design
Build: **OneDrive (personal) file content, Google Drive (My Drive) file content including native Docs/Sheets/Slides export, Outlook/Gmail message content, Google/Outlook calendar event attach.** Explicitly **not** included: SharePoint *team-site* document libraries. Reason: `sharepoint.read`'s scopes (`Sites.Read.All`) cover it, but today's code has **no site/library listing at all** — only `/me/drive/root/children`. Site enumeration + per-site drive browsing is materially more work (new Graph surface, a site picker before a file picker) and belongs in a follow-up, not this Wave‑2 item. This is called out again as an owner decision in §6.

### 2.2 integration-corev2 (Go) — new read operations, same executor
File: `internal/actions/service.go`.

1. **New raw-content helper**, alongside `getBearer`/`doJSON`:
   ```go
   func (s *Service) getBearerBinaryBase64(ctx, token, endpoint string, maxBytes int64, headers map[string]string) (contentB64 string, mimeType string, size int64, truncated bool, err error)
   ```
   - Builds the request exactly like `getBearer` (1097‑1108) but does **not** call `doJSON`.
   - Uses a **second** `egress.SafeClient` built with `AllowRedirects: true, MaxRedirects: 3` (new field on `IntegrationActionsClient`, e.g. `content_http`), used *only* by these new operations — leaves the existing `httpClient` (used by every write/list op) untouched, smallest possible blast radius.
   - Checks `resp.ContentLength` first; if it's set and `> maxBytes`, abort before reading (`truncated=true`, no body read).
   - Otherwise reads via `io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))`; if the result is `maxBytes+1` long, treat as truncated/oversized and return an error the caller renders as "too large."
   - `maxBytes = 1_000_000` (matches imports-core's existing ceiling, `chat_extract.py:23`, and the frontend's existing `prepare-chat-attachments.ts:13` check — one ceiling, stated once, referenced everywhere).
   - `mimeType` from `Content-Type` header (Microsoft) or from the caller-supplied `mimeType` param already known from the listing step (Google, since `alt=media` doesn't reliably echo it back).

2. **New operations in `executeMicrosoft`** (after 115):
   - `"drive.file.content", "microsoft.drive.file.content"` — params: `itemId` (required). Calls `getBearerBinaryBase64(ctx, token, base+"/me/drive/items/"+url.PathEscape(itemId)+"/content", 1_000_000, nil)`. Returns `{content_base64, mime_type, size, truncated}` as the JSON `result` (base64 keeps it inside the existing JSON-only response envelope `ExecuteResult{Result: any}`, so nothing downstream of `Execute` needs to change shape).
   - `"mail.messages.get", "microsoft.mail.messages.get"` — params: `messageId`. Calls `getBearer(ctx, token, base+"/me/messages/"+url.PathEscape(messageId)+"?$select=id,subject,from,receivedDateTime,webLink,body", nil)`, then HTML-strips `body.content` (see 2.4) into a `bodyText` field added to the returned JSON before it goes back to the caller.
   - Extend the existing `drive.files` `$select` (112‑113) to add `,size` — one-line change, needed so the picker can show a size badge and grey out an item over 1 MB *before* the user clicks it.

3. **New operations in `executeGoogle`** (after 189):
   - `"drive.file.content", "google.drive.file.content"` — params: `fileId`, `mimeType` (from the prior listing call, since Drive's list response already carries it). If `mimeType` starts with `application/vnd.google-apps.` (a native Doc/Sheet/Slide, which has no binary form and no `size`), call `files/{id}/export?mimeType=<office-equivalent>` (`.docx`/`.xlsx`/`.pptx` MIME) instead of `alt=media`, and have the caller append the matching extension to the synthetic filename so imports-core's extension-based dispatch (`chat_extract.py:26`) still works. Otherwise call `files/{id}?alt=media`.
   - `"gmail.messages.get", "google.gmail.messages.get"` — params: `id`. Calls `users/me/messages/{id}?format=full`, extracts the `Subject`/`From`/`Date` headers and the text/plain (or HTML-stripped text/html) MIME part, mirroring `emailsync/gmail.go`'s existing shape without importing that package.
   - Extend `drive.files`'s `fields` (187) to add `,size`; add an optional `q` builder from `params.query` (`name contains '<escaped>' and trashed=false`) and `params.folderId` (`'<id>' in parents`) — Drive already supports `q` server-side, this file just never passed one through.
   - Extend Microsoft's `drive.files` the same way: support `params.folderId` by switching the endpoint to `/me/drive/items/{folderId}/children`, and `params.query` by switching to `/me/drive/root/search(q='<escaped>')` when present.
   - Extend `calendar.events` (both providers) to accept `params.query`: Google's `events.list` already supports `q` natively (168‑183, just pass it through); Microsoft needs a `$search="<escaped>"` header (Graph requires `ConsistencyLevel: eventual` + ODATA `$search` rather than a query param — one extra header on the request).

4. **HTML-to-text reuse**: export `stripHTMLTags` from `internal/emailsync/teams.go:646` (rename to `StripHTMLTags`, or lift it into a tiny new `internal/htmlstrip` package imported by both `emailsync` and `actions`) so `actions/service.go` doesn't duplicate the parsing logic mail already solved.

5. **Capability gate** — `internal/api/server.go`, `requiredCapabilityForOperation` (3266‑3325): add case arms for every new operation string, mapping to the **same** capability the existing sibling listing operation already requires (no new capability keys, no new consent):
   - `"drive.file.content", "microsoft.drive.file.content"` → `("sharepoint.read", false)`
   - `"mail.messages.get", "microsoft.mail.messages.get"` → `("mail.read", true)`
   - `"drive.file.content", "google.drive.file.content"` → `("drive.read", false)`
   - `"gmail.messages.get", "google.gmail.messages.get"` → `("gmail.read", true)`
   Without this, `requireActionCapability` (3159‑3173) returns `errActionOperationUnsupported` for every one of these calls — this is not optional.
6. **`actionRequiresApproval`** (3455‑3517): no change needed — these operations are absent from every provider's switch, so they default to `false` (read, no attestation), same as `mail.messages`/`drive.files` today.

### 2.3 Gateway BFF (Rust) — the new browser-facing surface
Today the browser has **no** route into `integration-corev2`'s action executor at all (only execution-core does, server-to-server). New file `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations/browse.rs`, registered in `integrations.rs` (16‑90) alongside the existing routes, under the same `require_session` layer (89):

```rust
POST /api/v1/integrations/connections/{id}/browse    // list/search
POST /api/v1/integrations/connections/{id}/content   // fetch one item's content
```

Both handlers follow the existing `proxy_for_user` pattern (`integrations/shared.rs:17-39`) — mint the `ingestion` audience token for the signed-in user, then `proxy_integration_json(state, Method::POST, format!("{base}/api/v1/connections/{id}/actions"), Some(body), Some(&token), &user.user_id)` — **but first validate `body["operation"]` against a fixed allowlist** before forwarding:

```rust
const READ_ONLY_BROWSE_OPS: &[&str] = &[
    "mail.messages", "gmail.messages", "drive.files", "calendar.events",
];
const READ_ONLY_CONTENT_OPS: &[&str] = &[
    "drive.file.content", "microsoft.drive.file.content", "google.drive.file.content",
    "mail.messages.get", "microsoft.mail.messages.get", "google.gmail.messages.get", "gmail.messages.get",
];
```
Reject (400) anything else. This is defense-in-depth on top of integration-corev2's own capability/approval gate (§2.2): it is the first time the browser can reach this endpoint at all, and the allowlist means a browser client can never smuggle a write operation through even if a future catalog change mis-classifies one — it simply never reaches integration-corev2 unless the string is in this list.

`browse` request body: `{ operation: string, params: { query?, folderId?, pageToken?, maxResults? } }` (thin passthrough — the frontend already knows which `operation` string to send per provider+kind, see §2.4). Response: passthrough of integration-corev2's existing `{ action: { providerKey, operation, result } }` envelope, unchanged.

`content` request body: `{ operation: string, params: { itemId | messageId | fileId | id, mimeType? } }`. Same passthrough shape; the frontend already knows how to read `content_base64`/`bodyText` back out.

### 2.4 Frontend — composer picker UI
`apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx`

**Entry point.** Add a fifth row to the `"main"` settings-menu view (after `Koblinger`, 4394‑4404): **"Fra en tilkoblet kilde" / "From a connected source"**, opening a new `SettingsView` member `"attach-source"` (extend the union at line 191). Keep `Koblinger`/`"connectors"` exactly as-is (connection status/management is a different job from picking a file).

**New sub-view** (sibling of `SettingsSubView`/`RemoteSettingsList`, 4476‑4591), a new component `ConnectorAttachPicker`:
1. Reads `GET /api/v1/integrations/connections` (already proxied, `connections.rs:13-20`) via `createResource`, same pattern as `RemoteSettingsList` (4505‑4508), to get connected providers + their `connectionId`s. Only `microsoft`/`google` connections with `status==="active"` render a tab.
2. A segmented control per active provider tab: **Filer** (`drive.files`) / **E-post** (`mail.messages`/`gmail.messages`) / **Kalender** (`calendar.events`).
3. A debounced search input (reuse the existing debounce helper the composer already has for autocomplete, if one exists, else a 300ms `setTimeout` guard) posting to the new `POST /api/v1/integrations/connections/{id}/browse` with `{operation, params:{query}}`.
4. A scrollable result list; each row shows name/subject/title, a provider icon, and (for files) a size badge — greyed out with a tooltip when `size > 1_000_000`.
5. Selecting a row calls `POST /api/v1/integrations/connections/{id}/content` (skipped entirely for calendar — the browse step already returned the full event, see §1's `calendar.events` finding), then synthesizes a `ComposerFile`-shaped object and pushes it onto the same `files` signal (828) used by local uploads, so `AttachmentPreview` (3839‑3930), `removeFile` (1512‑1518), and the whole submit pipeline are reused unchanged. Concretely:
   - **File (binary doc)**: `new Blob([...])` from the decoded `content_base64`, `URL.createObjectURL(blob)` (same helper `createFilePreviewUrl` expects a `File`; add an overload/sibling `createFilePreviewUrlFromBlob`), `type` from `mime_type`, `extractedText` **already populated** by calling `extractChatDocument(name, content_base64, mime_type)` right here at pick-time (not deferred to submit) — because the network fetch already happened, extracting immediately is free and lets the picker show "couldn't read this file" *before* it becomes a chip, instead of failing silently at send time the way a bad local upload does today.
   - **File (image)**: same, but skip extraction — `url` is a `data:` URL built from the base64 directly (matches `toStreamAttachments`'s expectation of a `data:` URL or fetchable blob).
   - **Mail message / calendar event**: no binary content at all. Render a plain-text block client-side (`Emne: …\nFra: …\nDato: …\n\n<body>` / `Møte: …\nTid: …\nSted: …\nDeltakere: …\n\n<description>`), wrap it in `new Blob([text], {type:'text/plain'})`, name it e.g. `E-post – <subject>.txt` / `Kalender – <title>.txt`. **This needs zero changes to `inlineTextAttachments`/`textIngestibleAttachments`** — `type.startsWith('text/')` (`chat-normalizers.ts:1099`) already routes it through the existing inline-text path.
6. New provenance fields on the attachment object: `source: "onedrive"|"gmail"|"outlook"|"gdrive"|"gcal"|"outlook-cal"`, `sourceItemId: string`, `connectionId: string`, `fetchedAt: string (ISO)`. These ride as additive optional fields on `DashboardComposerAttachment` (246‑253) and `ChatTurnAttachment` (`chat-types.ts:39`) — no breaking change to either type or to any function that destructures a subset of these fields.

**Guard against redundant re-extraction at submit.** One-line change to `prepare-chat-attachments.ts:18-22`:
```ts
if (extractedText === undefined) {
  const parsed = parseDataUrl(url)
  if (!parsed) throw new Error('unreadable')
  extractedText = (await extractChatDocument(file.name, parsed.base64, file.type)).content
}
```
(currently unconditional). Everything else in that function (the 1 MB / 60,000-char checks) stays and still applies uniformly to connector-sourced attachments, since they went through the exact same `Blob`/data-URL construction as a local file.

**ZDR gate.** Mirror the existing pattern at `DashboardComposer.tsx:2999` (`knowledgeImportDisabled={temporaryChat() || ...}`): disable the new picker's attach action (not just knowledge-import) whenever `temporaryChat()` is true, with the same "Ikke tilgjengelig i midlertidig chat" messaging. Rationale in §4.

**Receipt / transcript surface.**
- `TurnReceipt` (`DashboardComposer.tsx:3940-3979`): extend the "N vedlegg lagt til" line to add a per-source breakdown when any attachment carries `source` (e.g. "3 vedlegg lagt til (1 fra OneDrive, 1 fra Outlook)").
- `AttachmentItem` (`ChatMessages.tsx:2179-2224`): when `attachment.source` is set, add a small provider badge/icon and extend the `title` tooltip (currently just "Åpne i arbeidsflate", 2207) to "Fra OneDrive · hentet 14:32".
- No new backend "receipt" system is needed: the attachment (with its new provenance fields) is already part of the ordinary turn record that flows through the same persistence as every other attachment (`use-chat-controller.ts` `displayAttachments`, e.g. 2218, 3404, 3489, 3524, 3585) and gets an integration-corev2 audit event for free (`connection.action.executed`, `server.go:3079-3088`) on every browse/content call.

### 2.5 What deliberately needs no change
- **Model routing, privacy tier, residency, ZDR enforcement** (`inference-core/src/provider/mod.rs:140-144`, `fallback.rs`, `grpc.rs`) — once connector content is inlined into `modelContent` via `appendInlinedAttachments`, it is indistinguishable from a locally-typed message and rides the existing fail-closed tier/residency/ZDR machinery untouched. This is a real simplification worth stating explicitly so nobody re-implements a parallel gate.
- **`execution-core/src/integration_tools.rs`** — untouched by this item. The composer picker is a person picking a specific item, not the model calling a tool; wiring these new read operations into the agent's own tool catalog (so the model can decide to open a file mid-conversation) is a related but separate, smaller follow-up (§6).

---

## 3. Tests to add

**Go — `internal/actions/service_test.go`** (alongside the existing `TestExecuteMicrosoftMailSendTranslatesVerevonEnvelope`-style tests):
- `TestExecuteMicrosoftDriveFileContentBase64EncodesResponse` — mock server returns bytes + `Content-Type`; assert `content_base64`/`mime_type`/`size` in the result.
- `TestExecuteMicrosoftDriveFileContentRejectsOverCap` — mock `Content-Length` above 1,000,000; assert an error, no body read.
- `TestExecuteMicrosoftDriveFileContentFollowsGraphRedirect` — mock a 302 to a second test server; assert the final bytes are returned (regression test for the `egress.SafeClient` redirect gap in §1).
- `TestExecuteMicrosoftMailMessagesGetStripsHTMLBody`.
- `TestExecuteGoogleDriveFileContentUsesExportForNativeDocs` — `mimeType` starting `application/vnd.google-apps.document` routes to `/export?mimeType=...`, not `alt=media`.
- `TestExecuteGoogleDriveFilesHonorsQueryAndFolderParams`.
- `TestExecuteGoogleGmailMessagesGetParsesFullFormat`.

**Go — `internal/api/server_test.go`**:
- `TestRequireActionCapabilityAllowsNewContentReadOperations` (table test alongside the existing capability tests) — one case per new operation string, asserting the same capability as its listing sibling and `sensitive` unchanged.
- `TestExecuteConnectionActionNewReadOpsNeverRequireApproval` — asserts `actionRequiresApproval` stays `false` for all seven new operation strings (guards against a future accidental reclassification as a write).

**Rust — a new `apps/gateway/src/domains/integrations/browse.rs` test module** (or `#[cfg(test)]` block colocated, matching this codebase's Rust test convention seen elsewhere in `domains/chat/*.rs`):
- rejects an operation not in `READ_ONLY_BROWSE_OPS`/`READ_ONLY_CONTENT_OPS` with 400 before any upstream call is made (assert on a mock upstream never being hit).
- forwards an allowlisted operation and passes the upstream response through unchanged.

**Frontend — `src/features/chat/lib/prepare-chat-attachments.test.ts`**:
- add a case: an attachment with `extractedText` already set is **not** re-sent to `extractChatDocument` (spy assertion), while one without it still is — pins the one-line guard in §2.4.

**Frontend — new `src/features/dashboard/home/DashboardComposer.connector-attach.test.tsx`** (mirror `DashboardComposer.paste-attachment.test.tsx`'s render/mock harness, lines 1‑44):
- mocks `GET /api/v1/integrations/connections` with one active Microsoft connection; opens the new picker row; types a query; asserts the debounced `POST .../browse` call shape.
- picks a result; mocks `POST .../content`; asserts a chip appears in `AttachmentPreview` with the file's name and that the object carries `source:"onedrive"`.
- picks a result while `temporaryChat()` is true; asserts the picker is disabled with the ZDR message (mirrors the existing `knowledgeImportDisabled` assertions in the composer's other tests).
- an over-cap file (`size` in the browse result `>1_000_000`) renders greyed out and is not clickable.

**Frontend — `chat-normalizers.wave2.test.ts`** (or a new sibling): a mail/calendar synthetic `.txt` attachment flows through `textIngestibleAttachments`/`inlineTextAttachments` with no change to those functions — a regression pin, not new logic.

---

## 4. Rollout

- **Rebuild required**: `integration-corev2` (Go — the actions executor and its capability table changed) and the `gateway` Rust binary (new routes). Per the house rule in `project-build-images-via-launcher.md`, use the launcher (`--from model` style rebuild, matching how Wave 1 was shipped per the plan's §4) rather than a hand-built `docker compose build`, which misses the Control Plane env chain.
- The frontend (`verevonv3`) needs a rebuild/restart too, but per the plan's own note (§4, "Wave 1"), the frontend container runs Vite dev over a bind mount with watching disabled, so a restart (not a rebuild) is what actually ships a source change.
- `execution-core`/`model-gateway`/`session-core` do **not** need a rebuild for this item — this feature never touches the agent runtime or the model-gateway invoke path.
- **No feature flag is strictly required** — the new gateway routes are additive and unreachable until the frontend ships the new menu row, and the new integration-corev2 operations are unreachable until something sends that operation string (allowlisted only at the new gateway routes). If a staged rollout is wanted anyway, gate the new "Fra en tilkoblet kilde" menu row behind the same mechanism Wave 1 items used (check `apps/verevon-web/plans/chat-workspace/` or the composer's existing feature-flag plumbing, if any — none was found wired into `DashboardComposer.tsx` today, so this would be a new flag if required).
- **No new OAuth consent screen / re-connect flow** for existing Microsoft/Google connections (§1) — this is a meaningful rollout simplification versus the plan's other Wave‑2/3 items.

---

## 5. Risks and edge cases

1. **Graph redirect on `/content`** (§1, §2.2‑1): must use a redirect-following client scoped only to the new content operations, or OneDrive downloads intermittently fail with an empty body depending on tenant/file storage backend. This is the single highest-value concrete bug to pre-empt; it's easy to miss because every *existing* call in this file happens to be a same-host JSON endpoint that never redirects.
2. **Native Google files have no `size`.** Docs/Sheets/Slides return `size: null` from Drive's `files.list` — the picker must not crash or show "NaN KB"; fall back to "—" and rely on the *exported* file's actual byte count (checked post-export, since the export can still exceed 1 MB even for a modest-looking Doc).
3. **Gmail's list endpoint can never carry a body** regardless of what params are sent — the existing catalog comment "No server-side query/search filter is implemented yet" is about search, not body; don't conflate the two when implementing `gmail.messages.get`.
4. **Truncation is a hard reject, not a partial attach.** Silently truncating a spreadsheet or long email at 1 MB / 60,000 chars would let the model reason confidently over an incomplete document. Match the existing local-upload UX exactly: refuse with the same class of message `prepare-chat-attachments.ts:31` already uses, don't invent a new "partial" state.
5. **Rate limiting.** The new `browse` endpoint is naturally chattier than the existing action calls (a user typing a search query) — confirm the existing `rateLimited` middleware (`server.go:506`) ceiling is generous enough for debounced keystroke-driven search, or the picker will visibly stall; if not, this is a config change, not a code change.
6. **Stale picks.** A file/message picked from search results can be deleted/moved between the `browse` and `content` calls (seconds later, but real) — the `content` call must surface Graph/Drive's 404 as a clean "no longer available, try search again" rather than an opaque failure, since the item is no longer in the list to retry against.
7. **Cross-tenant / cross-account confusion.** If an org has both a personal Google connection and a Google Workspace connection, or multiple Microsoft tenants, the picker's provider tabs must key off `connectionId`, not `providerKey` — reuse each connection's `displayName` (already returned by `/connections`) as the tab label so two Microsoft connections are distinguishable.
8. **HTML-stripping fidelity.** `stripHTMLTags` (teams.go:646) was written for chat/mail body rendering into a knowledge-search corpus, not for producing a model-readable inline block; spot-check that tables/lists in a rich HTML email don't collapse into unreadable run-on text before reusing it verbatim — may need a scoped tweak (e.g. preserve line breaks on `<br>`/`<p>`/`<li>`), which is why §2.2‑4 recommends exporting/sharing the function rather than assuming it's drop-in perfect.
9. **Caching.** Per the item's explicit ask: (a) listing/search results are **not** persisted anywhere server-side and are cached client-side only for the lifetime of the picker's open state (a short in-memory TTL, e.g. via `createResource`'s natural caching, is enough — no `localStorage`, since these are personal-mailbox/drive result rows); (b) fetched content is never cached at any layer beyond the ephemeral extraction call already covered by `Cache-Control: no-store` (`documents.rs:29`) — re-picking the same file re-fetches it, keeping `fetchedAt` honest for the provenance badge; (c) the audit event on every read call (`server.go:3079-3088`) is the durable trail, not a cache.
10. **ZDR interaction.** Confirmed no existing residency/ZDR gate applies to integration-corev2 reads today (§1) — this feature is the first place a temporary-chat turn could pull in durable-account content without any existing block, so the composer-side ZDR guard in §2.4 is load-bearing, not decorative; it should fail closed (disable the row) rather than fail open.

---

## 6. Genuine owner decisions

1. **SharePoint scope for this wave.** Ship OneDrive-personal + Google-Drive-My-Drive only now (size **M**, as designed above), and treat SharePoint team-site libraries as a separate follow-up that needs new site-listing/site-drive-browsing code — or pull SharePoint in now and accept size **L**? (Recommendation embedded above: defer it — the plan's own Wave‑2 "moderate" framing fits the M‑scope version; a full Perplexity/Copilot-parity SharePoint browser is a materially bigger, separately-schedulable slice.)
2. **Pre-flight readability check UX.** The design above extracts/validates a document's text *at pick time* (so a chip only ever appears once it's confirmed readable) rather than deferring to submit time the way local uploads do today. This is a deliberate UX improvement, not free — it means every click on a search result does a network round-trip before becoming a chip. Confirm this is the wanted behavior versus matching local-upload's "attach now, discover failure at send" pattern exactly.
3. **Should content-read become agent-callable too?** Once `drive.file.content`/`mail.messages.get`/`gmail.messages.get` exist in `service.go`, adding them to `execution-core/src/integration_tools.rs`'s `CATALOG` (mirroring Wave 1's pattern exactly) is a small additive change that would let the *model* decide to open a file mid-conversation, not only the user via the picker. In scope for this item, or a distinct decision because it changes what the agent can autonomously read without a person's explicit pick?

---

## 7. Size and dependencies

- **Overall size: M** (matches the plan's own estimate), assuming the SharePoint-site-library exclusion in §6‑1 is accepted; **L** if SharePoint team sites must ship in the same wave.
- **Depends on nothing else in Wave 2** — it doesn't need the source-scope filter, persona instructions, the editable plan gate, or the raised subagent ceiling. It reuses Wave 1's already-shipped groundwork (mail/drive read ops in the agent catalog exist as *sibling* infrastructure, confirming the OAuth scopes and audit/capability plumbing are already proven in production) but does not require it to be re-touched.
- **Soft dependency on "forward an email to create a task"** (another Wave‑2 item): if that item also needs full Outlook/Gmail message-body content, it should reuse the exact same `mail.messages.get`/`gmail.messages.get` operations built here rather than growing a second, slightly different message-fetch path — worth sequencing this item first if both are being picked up in the same wave.
- **No dependency on inline citation chips, background-task notifications, or share links** — those touch the SSE/transcript layer and durable-run layer respectively; this item never leaves the composer/attachment pipeline plus one new integration-corev2 read surface.