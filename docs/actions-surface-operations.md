# Actions Surface — Operation Contract (integration-corev2)

**Status: FROZEN CONTRACT.** This document enumerates every operation accepted by the
`Execute` switch in `apps/Ingestion Plane/integration-corev2/internal/actions/service.go`
and the capability/approval rules enforced by
`apps/Ingestion Plane/integration-corev2/internal/api/server.go`.

Snapshot: working tree as of 2026-07-05 (`internal/actions/service.go` md5 `52b484d2b9da373241027ff25526035d`,
1317 lines). Line numbers below refer to that state of `service.go` unless prefixed `server.go`.

---

## 1. HTTP contract

```
POST /api/v1/actions/execute          (server.go:402)
POST /api/v1/connections/:id/actions  (server.go:390 — same semantics, connectionId in the path)
```

Both routes are rate-limited (`rateLimited` chain) and authenticated by
`internalOrBearerAuth` = `auth.InternalOrBearer` (`internal/auth/middleware.go:100`):

- **Bearer**: `Authorization: Bearer <token>`, verified against auth-core (`TokenVerifier`).
  The principal must carry a non-empty `UserID` and `OrganizationID`; org scoping is then
  enforced per-request with `auth.AssertOrgAccess(c, connection.OrganizationID)`
  (server.go:2299) — a bearer caller can only execute actions on connections in its own org.
- **Internal**: `X-Internal-API-Key: <key>` header (header name configurable via
  `InternalAPIKeyHeader`, default `X-Internal-API-Key`; constant-time compare). Internal
  callers get a synthetic principal and bypass org scoping — they are trusted to pass the
  right `connectionId`.

Request body (`executeActionBody`, server.go:1054):

```json
{
  "connectionId": "conn_…",        // required (path param on the /connections/:id/actions form)
  "operation": "whatsapp.messages.send",
  "params": { "phoneNumberId": "…" },   // URL/query/path inputs, provider-specific
  "body":   { "to": "…", "type": "text", "text": { "body": "…" } }  // request payload for writes
}
```

Success response envelope — verified against `success()` (server.go:2839) and the route
handler (server.go:402-419):

```json
{
  "success": true,
  "data": { "action": { "providerKey": "…", "operation": "…", "result": <provider JSON> } },
  "meta": { "requestId": "…" }      // present when a request id exists
}
```

`result` is the provider's response decoded as-is (`any`); a `204 No Content` or empty 2xx
body becomes `{"ok": true}` (plus `"id"` from the `x-restli-id` header for LinkedIn creates,
service.go:1072-1088). Errors use the standard envelope
`{"success": false, "error": {"code", "message"}}` via `apiError`/`actionError`
(server.go:2603: `connection_not_found` 404, `actions_unavailable` 503,
`capability_required`/`approval_required` 403, `action_failed` 502 for provider errors).

Every successful execution records an audit event `connection.action.executed`
(server.go:2319).

## 2. Resolving a connectionId first

Callers must resolve a connection before executing:

```
GET /api/v1/connections?organizationId=<org>&providerKey=<key>     (server.go:204)
```

Same `internalOrBearerAuth`. For bearer callers `organizationId` is **overridden** by the
principal's org (server.go:206-210) — only internal callers can select an arbitrary org.
Optional filters: `connectorType`, `userId`, `category`/`providerCategory`. `providerKey`
is normalized (`providers.NormalizeKey`), so aliases like `twitter` → `x` resolve.
Response: `{"success": true, "data": {"connections": [ … ]}}`; each connection carries
`id`, `providerKey`, `status`, `capabilities`, `scopes`, etc. Use the `id` as
`connectionId`.

## 3. Enforcement semantics (read this before adding callers)

`requireActionCapability` (server.go:2332) runs before token resolution:

1. `requiredCapabilityForOperation(providerKey, operation)` (server.go:2371) maps
   operation → (capability, sensitive). Operation matching here is **lowercased** for all
   providers.
2. **If the operation has no mapping, the function returns `nil` immediately** — no
   capability check *and no approval check*. Unmapped operations are unguarded, not
   uncallable (see inconsistencies, §6).
3. Capability passes if the connection's `capabilities` list contains the string, **or**
   the connection has an *empty* capabilities list and the operation is not marked
   sensitive (server.go:2337). Sensitive operations always require the explicit capability.
4. Write operations listed in `actionRequiresApproval` (server.go:2535) additionally
   require approval metadata: `approvalId` or `approvalRef` in `params` **or** `body`
   (server.go:2355). Missing → 403 `approval_required`.

In the tables below, "Sensitive" is the boolean from step 3; "Approval" is step 4.
Every operation also accepts a provider-prefixed alias (e.g. `mail.send` ≡
`microsoft.mail.send`) — aliases are listed inline. `service.go` matches operation strings
**case-sensitively** for microsoft/slack/google/github/notion/shopify/stripe/okta, and
lowercases them for linkedin and the meta family — send lowercase operation strings always.

Param notation: `name*` = required; `(d=X, max=Y)` = default/clamp applied server-side.

---

## 4. Operations by provider family

### microsoft (5 ops) — Graph `…/v1.0`, Bearer token

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `profile` / `microsoft.profile` | GET `/me?$select=id,displayName,mail,userPrincipalName` | — | ignored | `profile.read` | no | no | 87 |
| `calendar.events` / `microsoft.calendar.events` | GET `/me/events?$top=` | `maxResults` (d=10, max=100) | ignored | `calendar.read` | yes | no | 89 |
| `mail.messages` / `microsoft.mail.messages` | GET `/me/messages?$top=&$select=id,subject,from,receivedDateTime,webLink` | `maxResults` (d=10, max=100) | ignored | `mail.read` | yes | no | 92 |
| `drive.files` / `microsoft.drive.files` | GET `/me/drive/root/children?$top=&$select=…` | `maxResults` (d=10, max=100) | ignored | `sharepoint.read` | no | no | 98 |
| `mail.send` / `microsoft.mail.send` | POST `/me/sendMail` | — | JSON, forwarded verbatim; **required** (Graph `sendMail` shape) | `mail.send` | yes | **yes** | 104 |

### slack (5 ops) — `SlackAPIBaseURL`, Bearer token

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `channels.list` / `slack.channels.list` | GET `/conversations.list?types=&limit=&exclude_archived=true` | `types` (d=`public_channel,private_channel`), `limit` (d=100, max=1000) | ignored | `channels.read` | no | no | 117 |
| `user` / `users.info` (+ `slack.` aliases) | GET `/users.info?user=` | `user`* (or `userId`) | ignored | `users.read` | no | no | 124 |
| `users.list` / `slack.users.list` | GET `/users.list?limit=` | `limit` (d=100, max=1000) | ignored | `users.read` | no | no | 134 |
| `messages.list` / `slack.messages.list` | GET `/conversations.history?channel=&limit=` | `channel`*, `limit` (d=50, max=200) | ignored | `channels.history` | yes | no | 137 |
| `message.send` / `slack.message.send` | POST `/chat.postMessage` | — | JSON, forwarded verbatim; **required** (`channel`, `text`/`blocks`) | `messages.write` | yes | **yes** | 144 |

### google (5 ops) — `GoogleAPIBaseURL`, Bearer token

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `profile` / `google.profile` | GET `/oauth2/v3/userinfo` | — | ignored | `profile.read` | no | no | 157 |
| `gmail.messages` / `google.gmail.messages` | GET `/gmail/v1/users/me/messages?maxResults=` | `maxResults` (d=10, max=100) | ignored | `gmail.read` | yes | no | 159 |
| `gmail.send` / `google.gmail.send` | POST `/gmail/v1/users/me/messages/send` | — | JSON, forwarded verbatim; **required** (`{"raw": <base64url RFC822>}`) | `gmail.send` | yes | **yes** | 162 |
| `calendar.events` / `google.calendar.events` | GET `/calendar/v3/calendars/primary/events?maxResults=` | `maxResults` (d=10, max=100) | ignored | `calendar.read` | yes | no | 167 |
| `drive.files` / `google.drive.files` | GET `/drive/v3/files?pageSize=&fields=files(id,name,mimeType,webViewLink,modifiedTime)` | `maxResults` (d=10, max=100) | ignored | `drive.read` | no | no | 170 |

### github (15 ops) — `GitHubAPIBaseURL`, Bearer token, headers `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `user` / `github.user` | GET `/user` | — | ignored | `profile.read` | no | no | 188 |
| `emails` / `github.emails` | GET `/user/emails?per_page=` | `perPage` (d=30, max=100) | ignored | `profile.read` | no | no | 190 |
| `orgs` / `github.orgs` | GET `/user/orgs?per_page=` | `perPage` (d=30, max=100) | ignored | `org.read` | no | no | 193 |
| `teams` / `github.teams` | GET `/orgs/{org}/teams?per_page=` | `org`*, `perPage` | ignored | `org.read` | no | no | 196 |
| `repos` / `github.repos` | GET `/user/repos` or `/orgs/{org}/repos` `?per_page=&sort=&type=` | `perPage`, `sort` (d=`updated`), `type` (d=`owner`), `org` (opt → org listing) | ignored | `repo.public.read` | no | no | 203 |
| `repo` / `github.repo` | GET `/repos/{owner}/{repo}` | `owner`*, `repo`* | ignored | `repo.public.read` | no | no | 213 |
| `contents.get` / `github.contents.get` | GET `/repos/{owner}/{repo}/contents/{path}?ref=` | `owner`*, `repo`*, `path`* (validated, no `.`/`..`), `ref` (opt) | ignored | `repo.contents.read` | yes | no | 220 |
| `readme.get` / `github.readme.get` | GET `/repos/{owner}/{repo}/readme?ref=` | `owner`*, `repo`*, `ref` (opt) | ignored | `repo.contents.read` | yes | no | 238 |
| `branches` / `github.branches` | GET `/repos/{owner}/{repo}/branches?per_page=` | `owner`*, `repo`*, `perPage` | ignored | `repo.contents.read` | yes | no | 252 |
| `commits` / `github.commits` | GET `/repos/{owner}/{repo}/commits?per_page=&sha=` | `owner`*, `repo`*, `perPage`, `sha` (opt) | ignored | `commits.read` | yes | no | 259 |
| `pulls` / `pulls.list` (+ `github.` aliases) | GET `/repos/{owner}/{repo}/pulls?per_page=&state=` | `owner`*, `repo`*, `perPage`, `state` (d=`open`) | ignored | `pulls.read` | yes | no | 269 |
| `issues` / `issues.list` (+ `github.` aliases) | GET `/repos/{owner}/{repo}/issues?per_page=&state=` | `owner`*, `repo`*, `perPage`, `state` (d=`open`) | ignored | `issues.read` | yes | no | 279 |
| `issues.create` / `github.issues.create` | POST `/repos/{owner}/{repo}/issues` | `owner`*, `repo`* | JSON, forwarded verbatim; **required** (`title`, …) | `issues.write` | yes | **yes** | 289 |
| `issues.update` / `github.issues.update` | **PATCH** `/repos/{owner}/{repo}/issues/{issueNumber}` | `owner`*, `repo`*, `issueNumber`* | JSON, forwarded verbatim; **required** | `issues.write` | yes | **yes** | 298 |
| `issues.comment.create` / `github.issues.comment.create` | POST `/repos/{owner}/{repo}/issues/{issueNumber}/comments` | `owner`*, `repo`*, `issueNumber`* | JSON, forwarded verbatim; **required** (`body`) | `issues.write` | yes | **yes** | 311 |

### notion (3 ops) — `NotionAPIBaseURL`, Bearer token, header `Notion-Version: 2022-06-28`

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `user` / `notion.user` | GET `/v1/users/me` | — | ignored | `workspace.read` | no | no | 333 |
| `databases` / `notion.databases` | POST `/v1/search` (fixed payload `{filter:{value:"database",property:"object"}}`) | — | ignored (payload is fixed) | `content.read` | yes | no | 335 |
| `pages` / `notion.pages` | with `databaseId`: POST `/v1/databases/{databaseId}/query`; else POST `/v1/search` (fixed page filter) | `databaseId` (opt) | when `databaseId` set: body is the query payload (empty `{}` if omitted); else ignored | `content.read` | yes | no | 338 |

### shopify (3 ops) — `https://{shop}/admin/api/2026-01` (shop from connection `providerContext`), auth via `X-Shopify-Access-Token` header

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `shop` / `shopify.shop` | GET `/shop.json` | — | ignored | `store.read` | no | no | 361 |
| `products` / `shopify.products` | GET `/products.json?limit=` | `limit` (d=50, max=250) | ignored | `products.read` | no | no | 363 |
| `orders` / `shopify.orders` | GET `/orders.json?limit=&status=` | `limit` (d=50, max=250), `status` (d=`any`) | ignored | `orders.read` | yes | no | 366 |

### stripe (4 ops) — `StripeAPIBaseURL`, Bearer token

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `account` / `stripe.account` | GET `/v1/account` | — | ignored | `account.read` | no | no | 380 |
| `customers` / `stripe.customers` | GET `/v1/customers?limit=` | `limit` (d=10, max=100) | ignored | `customers.read` | yes | no | 382 |
| `subscriptions` / `stripe.subscriptions` | GET `/v1/subscriptions?limit=` | `limit` (d=10, max=100) | ignored | `billing.read` | yes | no | 385 |
| `invoices` / `stripe.invoices` | GET `/v1/invoices?limit=` | `limit` (d=10, max=100) | ignored | `billing.read` | yes | no | 388 |

### okta (5 ops) — `OktaAPIBaseURL`/`OktaDomain`; auth `SSWS <token>` when the token equals the configured `OktaAPIToken`, otherwise `Bearer <token>`

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `org` / `okta.org` | GET `/api/v1/org` | — | ignored | `tenant.read` | no | no | 878 |
| `users` / `okta.users` | GET `/api/v1/users?limit=` | `limit` (d=50, max=200) | ignored | `directory.read` | yes | no | 880 |
| `groups` / `okta.groups` | GET `/api/v1/groups?limit=` | `limit` (d=50, max=200) | ignored | `directory.read` | yes | no | 883 |
| `user.suspend` / `okta.user.suspend` | POST `/api/v1/users/{userId}/lifecycle/suspend` (empty body) | `userId`* | ignored | `directory.write` | yes | **yes** | 886 |
| `user.activate` / `okta.user.activate` | POST `/api/v1/users/{userId}/lifecycle/activate?sendEmail=` (empty body) | `userId`*, `sendEmail` (d=`false`) | ignored | `directory.write` | yes | **yes** | 892 |

### linkedin (19 ops) — `LinkedInAPIBaseURL`, Bearer token; `/rest/*` calls send `LinkedIn-Version` (d=`202606`) + `X-Restli-Protocol-Version: 2.0.0`; operation strings lowercased before matching

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `profile` / `linkedin.profile` | GET `/v2/userinfo` (no versioned headers) | — | ignored | `social.profile.read` | no | no | 400 |
| `identity` / `linkedin.identity` | GET `/rest/identityMe` | — | ignored | `social.profile.verify` | yes | no | 402 |
| `verification.report` / `linkedin.verification.report` | GET `/rest/verificationReport?verificationCriteria=…` | `verificationCriteria` (opt, string/list/CSV) | ignored | `social.verification.read` | yes | no | 404 |
| `organization.acls` / `organizations` / `linkedin.organization.acls` | GET `/rest/organizationAcls?q=roleAssignee&count=&start=(&role=&state=)` | `count` (d=10, max=100), `start` (d=0), `role`/`state` (opt) | ignored | `social.organization.read` | yes | no | 414 |
| `posts.list` / `linkedin.posts.list` | GET `/rest/posts?q=author&author=&count=&start=` | `author`* (URN), `count`, `start` | ignored | `social.post.read` | yes | no | 427 |
| `posts.create` / `linkedin.posts.create` | POST `/rest/posts` | — | JSON, forwarded verbatim; **required** (Posts API shape); create id returned via `x-restli-id` | `social.post.write` | yes | **yes** | 439 |
| `events.create` / `linkedin.events.create` | POST `/rest/events` | — | JSON, forwarded verbatim; **required** | `social.events.manage` | yes | **yes** | 444 |
| `events.get` / `linkedin.events.get` | GET `/rest/events/{eventId}` | `eventId`* | ignored | `social.organization.read` | yes | no | 449 |
| `events.update` / `linkedin.events.update` | POST `/rest/events/{eventId}` + header `X-RestLi-Method: partial_update` | `eventId`* | JSON partial-update patch; **required** | `social.events.manage` | yes | **yes** | 455 |
| `ads.accounts` / `linkedin.ads.accounts` | GET `/rest/adAccounts?pageSize=(&pageToken=)` | `pageSize` (d=10, max=100), `pageToken` (opt) | ignored | `social.ads.read` | yes | no | 466 |
| `ads.account` / `linkedin.ads.account` | GET `/rest/adAccounts/{accountId}` | `accountId`* | ignored | `social.ads.read` | yes | no | 472 |
| `ads.campaigns` / `linkedin.ads.campaigns` | GET `/rest/adAccounts/{accountId}/adCampaigns?pageSize=(&q=search&search=&sortOrder=&pageToken=)` | `accountId`*, `pageSize`, `search`/`sortOrder`/`pageToken` (opt) | ignored | `social.ads.read` | yes | no | 478 |
| `ads.campaign` / `linkedin.ads.campaign` | GET `/rest/adAccounts/{accountId}/adCampaigns/{campaignId}` | `accountId`*, `campaignId`* | ignored | `social.ads.read` | yes | no | 495 |
| `ads.campaign.create` / `linkedin.ads.campaign.create` | POST `/rest/adAccounts/{accountId}/adCampaigns` | `accountId`* | JSON, forwarded verbatim; **required** | `social.ads.manage` | yes | **yes** | 505 |
| `ads.campaign.update` / `linkedin.ads.campaign.update` | POST `/rest/adAccounts/{accountId}/adCampaigns/{campaignId}` + `X-RestLi-Method: partial_update` | `accountId`*, `campaignId`* | JSON partial-update patch; **required** | `social.ads.manage` | yes | **yes** | 514 |
| `conversions.list` / `linkedin.conversions.list` | GET `/rest/conversions?q=account&account=&count=&start=` | `account`* (sponsoredAccount URN), `count`, `start` | ignored | `social.ads.read` | yes | no | 529 |
| `conversions.create` / `linkedin.conversions.create` | POST `/rest/conversions(?autoAssociationType=)` | `autoAssociationType` (opt) | JSON, forwarded verbatim; **required** | `social.conversions.manage` | yes | **yes** | 541 |
| `lead.forms` / `linkedin.lead.forms` | GET `/rest/leadForms?q=owner&owner=&count=&start=` | `owner`* (URN), `count`, `start` | ignored | `social.leads.read` | yes | no | 554 |
| `lead.responses` / `linkedin.lead.responses` | GET `/rest/leadFormResponses?q=leadForm&leadForm=&count=&start=` | `leadForm`* (URN), `count`, `start` | ignored | `social.leads.read` | yes | no | 566 |

### meta family (37 ops) — provider keys `meta`, `facebook`, `instagram`, `whatsapp`, `meta-ads` all route to the same executor (service.go:71). Graph base = `FacebookAPIBaseURL`/`InstagramAPIBaseURL` (default `https://graph.facebook.com/v25.0`); Threads base = `MetaThreadsAPIBaseURL` (default `https://graph.threads.net/v1.0`). Operation strings lowercased before matching. "form" body semantics = `body` map flattened into `application/x-www-form-urlencoded` fields (`valuesFromMap`, non-scalar values JSON-encoded). "page token" = user token exchanged for the Page access token first via GET `/{pageId}?fields=access_token,name` (service.go:1013)

| Operation (aliases) | Method + Endpoint | Required params | Body semantics | Capability | Sensitive | Approval | service.go |
|---|---|---|---|---|---|---|---|
| `profile` / `meta.profile` / `facebook.profile` | GET `/me?fields=id,name` | — | ignored | `social.profile.read` | no | no | 591 |
| `pages.list` / `facebook.pages` / `meta.pages` | GET `/me/accounts?fields=id,name,category,tasks,instagram_business_account{…},connected_instagram_account{…}&limit=` | `limit` (d=25, max=100) | ignored | `social.profile.read` | no | no | 593 |
| `pages.post` / `facebook.page.post` | POST `/{pageId}/feed` (page token) | `pageId`* | form (`message`, `link`, …) | `social.post.write` | yes | **yes** | 599 |
| `pages.photo` / `facebook.page.photo` | POST `/{pageId}/photos` (page token) | `pageId`* | form (`url`/`caption`, …) | `social.media.upload` | yes | **yes** | 609 |
| `live.create` / `facebook.live.create` | POST `/{pageId}/live_videos` (page token) | `pageId`* | form (`title`, `description`, …) | `social.live.manage` | yes | **yes** | 619 |
| `live.list` / `facebook.live.list` | GET `/{pageId}/live_videos?fields=id,title,status,creation_time,permalink_url&limit=` (page token) | `pageId`*, `limit` | ignored | `social.live.manage` | yes | no | 629 |
| `live.get` / `facebook.live.get` | GET `/{liveVideoId}?fields=id,title,status,stream_url,secure_stream_url,embed_html,permalink_url` | `liveVideoId`* | ignored | `social.live.manage` | yes | no | 643 |
| `instagram.accounts` / `meta.instagram.accounts` | GET `/me/accounts?fields=id,name,instagram_business_account{id,username,name,profile_picture_url}&limit=` | `limit` | ignored | `social.instagram.read` | yes | no | 650 |
| `instagram.media.create` | POST `/{igUserId}/media` | `igUserId`* | form (`image_url`/`video_url`, `caption`, …) | `social.media.upload` | yes | **yes** | 656 |
| `instagram.media.publish` | POST `/{igUserId}/media_publish` | `igUserId`* | form (`creation_id`) | `social.media.upload` | yes | **yes** | 662 |
| `instagram.media.status` | GET `/{creationId}?fields=id,status_code,status` | `creationId`* | ignored | `social.instagram.read` | yes | no | 668 |
| `instagram.insights` | GET `/{mediaId}/insights?metric=` | `mediaId`*, `metric` (d=`impressions,reach,likes,comments,saved,shares`) | ignored | `social.instagram.read` | yes | no | 674 |
| `whatsapp.business_accounts` / `whatsapp.accounts` | GET `/me/businesses?fields=id,name,owned_whatsapp_business_accounts{…}&limit=` | `limit` | ignored | `social.whatsapp.manage` | yes | no | 681 |
| `whatsapp.phone_numbers` | GET `/{wabaId}/phone_numbers?fields=id,display_phone_number,verified_name,quality_rating&limit=` | `wabaId`*, `limit` | ignored | `social.whatsapp.manage` | yes | no | 687 |
| `whatsapp.templates` | GET `/{wabaId}/message_templates?limit=` | `wabaId`*, `limit` | ignored | `social.whatsapp.manage` | yes | no | 697 |
| `whatsapp.messages.send` | POST `/{phoneNumberId}/messages` (JSON) | `phoneNumberId`* | JSON Cloud-API message; **required**; `to`* and `type`* enforced, `messaging_product` defaulted to `whatsapp` (service.go:1123) | `social.whatsapp.manage` | yes | **yes** | 703 |
| `messenger.messages.send` | POST `/{pageId}/messages` (JSON, page token) | `pageId`* | JSON Send-API payload (`recipient`, `message`); **required** | `social.messenger.manage` | yes | **yes** | 716 |
| `messenger.subscribed_apps` | POST `/{pageId}/subscribed_apps` (form, page token) | `pageId`*, `subscribedFields` (d=`messages,messaging_postbacks,messaging_optins,message_deliveries,message_reads`) | ignored (fields come from params) | `social.messenger.manage` | yes | **yes** | 729 |
| `ads.businesses` / `meta.businesses` | GET `/me/businesses?fields=id,name,verification_status&limit=` | `limit` | ignored | `social.profile.read` | no | no | 740 |
| `audience_network.apps` / `meta.audience_network.apps` | GET `/me/applications?fields=id,name,namespace,link&limit=` | `limit` | ignored | `social.audience_network.read` | yes | no | 742 |
| `ads.adaccounts` / `meta.adaccounts` | GET `/me/adaccounts?fields=id,account_id,name,account_status,currency,business&limit=` | `limit` | ignored | `social.ads.manage` | yes | no | 744 |
| `ads.campaigns` | GET `/act_{adAccountId}/campaigns?fields=…&limit=` | `adAccountId`* (or `accountId`; `act_` prefix normalized), `limit` | ignored | `social.ads.manage` | yes | no | 750 |
| `ads.campaign.create` / `app_ads.campaign.create` | POST `/act_{adAccountId}/campaigns` | `adAccountId`* | form (`name`, `objective`, `status`, `special_ad_categories`, …) | `social.ads.manage` | yes | **yes** | 760 |
| `ads.adsets` | GET `/act_{adAccountId}/adsets?fields=…&limit=` | `adAccountId`*, `limit` | ignored | `social.ads.manage` | yes | no | 766 |
| `ads.ads` | GET `/act_{adAccountId}/ads?fields=…&limit=` | `adAccountId`*, `limit` | ignored | `social.ads.manage` | yes | no | 772 |
| `ads.creatives` | GET `/act_{adAccountId}/adcreatives?fields=…&limit=` | `adAccountId`*, `limit` | ignored | `social.ads.manage` | yes | no | 778 |
| `ads.insights` | GET `/act_{adAccountId}/insights?fields=…&limit=(&date_preset=)` | `adAccountId`*, `limit`, `datePreset` (opt) | ignored | `social.analytics.read` | yes | no | 784 |
| `catalogs.list` / `catalog.list` | GET `/{businessId}/owned_product_catalogs?fields=id,name,vertical,product_count&limit=` | `businessId`*, `limit` | ignored | `social.catalog.manage` | yes | no | 797 |
| `catalog.products` | GET `/{catalogId}/products?fields=…&limit=` | `catalogId`*, `limit` | ignored | `social.catalog.manage` | yes | no | 807 |
| `catalog.product.upsert` | POST `/{catalogId}/products` | `catalogId`* | form (retailer fields) | `social.catalog.manage` | yes | **yes** | 817 |
| `catalog.batch` | POST `/{catalogId}/batch` (JSON) | `catalogId`* | JSON batch payload, forwarded verbatim | `social.catalog.manage` | yes | **yes** | 823 |
| `threads.profile` | GET `{threads}/me?fields=id,username,name,threads_profile_picture_url,threads_biography` | — | ignored | `social.threads.manage` | yes | no | 829 |
| `threads.container.create` | POST `{threads}/me/threads` | — | form (`media_type`, `text`, `image_url`, …) | `social.threads.manage` | yes | **yes** | 831 |
| `threads.container.status` | GET `{threads}/{creationId}?fields=id,status,error_message` | `creationId`* | ignored | **none mapped** (see §6) | — | no | 833 |
| `threads.publish` | POST `{threads}/me/threads_publish` | — | form (`creation_id`) | `social.threads.manage` | yes | **yes** | 845 |
| `threads.insights` | GET `{threads}/me/threads_insights?metric=` | `metric` (d=`views,likes,replies,reposts,quotes`) | ignored | `social.analytics.read` | yes | no | 847 |
| `oembed` / `meta.oembed` | GET `/{oembed_post\|oembed_video\|oembed_page\|instagram_oembed\|threads_oembed}?url=(&maxwidth=)` | `url`* (params or body), `kind` (d=`post`; `video`/`live`, `page`, `instagram`, `threads`), `maxWidth` (opt) | `url` fallback only | `social.oembed.read` | no | no | 849 |

Row totals: microsoft 5, slack 5, google 5, github 15, notion 3, shopify 3, stripe 4, okta 5, linkedin 19, meta family 37 — **101 operations**.

---

## 5. Providers with OAuth support but ZERO actions — not yet implemented

These providers exist in the OAuth catalog (`internal/providers/catalog.go`) and can hold
live connections, but have **no branch in the Execute switch** (service.go:54-77). Any
`operation` against them returns `actions are not implemented for provider <key>`.
**Do not guess operation names for these** — none exist yet:

- `tiktok` (catalog.go:1488)
- `snapchat` (catalog.go:1551)
- `discord` (catalog.go:1207)
- `x` (Twitter/X, catalog.go:891; aliases `twitter`, `twitter-x`, `x-twitter` normalize to `x`)

The same default error applies to every other providerKey not listed in §4 (e.g. pure
knowledge connectors).

## 6. Inconsistencies found between the capability map and the Execute switch

What the code enforces, verbatim — flagged so callers and future edits don't trip on them:

1. **`threads.container.status` (service.go:833) has no entry in
   `requiredCapabilityForOperation` (server.go:2513-2516 covers only `threads.profile`,
   `threads.container.create`, `threads.publish`, `threads.insights`).** Because
   `requireActionCapability` returns `nil` when no capability is mapped (server.go:2334),
   this operation is callable with **no capability check and no approval check** — it is
   unguarded, not uncallable. It is a read-only status poll, but any future op added to the
   switch without a capability mapping inherits the same bypass.
2. **`actionRequiresApproval` contains dead entries for operations that do not exist in the
   Execute switch**: notion `content.write`/`notion.content.write` (server.go:2554) and
   shopify `orders.write`/`shopify.orders.write` (server.go:2556). They are unreachable
   today; if those operations are ever implemented, the approval gate will *still not fire*
   until a capability mapping is also added (see point 1's early-return).
3. **Empty-capabilities leniency**: a connection whose `capabilities` list is empty passes
   all *non-sensitive* operations without holding the mapped capability (server.go:2337).
   Sensitive operations always require the explicit capability string.
4. **Meta ads reads require the write capability**: `ads.adaccounts`, `ads.campaigns`,
   `ads.adsets`, `ads.ads`, `ads.creatives` all map to `social.ads.manage`
   (server.go:2504-2506), unlike LinkedIn where reads map to `social.ads.read`. Callers
   doing read-only Meta ads work still need the manage capability granted.
5. **Case sensitivity is asymmetric**: the capability map lowercases operations for all
   providers, but `service.go` matches case-sensitively for everything except linkedin and
   the meta family. A mixed-case operation for e.g. microsoft passes the capability layer
   but fails in the executor with `unsupported … operation`. Always send lowercase.

## 7. Contract stability

**The operation strings in this document are the source of truth** for every downstream
caller of the actions surface: Model Plane agent tools (provider-action list/execute),
Application Plane `social-core` (ads workflows, metrics snapshots), `conversation-core`
(WhatsApp/Messenger outbound sends), and Ingestion Plane `leads-core` (provider lead sync).
Callers must use these exact strings, params, and body shapes — do not invent operation
names.

Any change to the `Execute` switch in
`apps/Ingestion Plane/integration-corev2/internal/actions/service.go` (adding, renaming, or
removing an operation) or to `requiredCapabilityForOperation` /
`actionRequiresApproval` in `internal/api/server.go` **must update this document in the
same change**. When adding an operation, add its capability mapping (and approval entry for
writes) in the same commit — see §6.1 for why an unmapped operation silently skips both
gates.
