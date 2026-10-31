# V3-spa-errors: every error code the Verevon gateway can send to the SPA

Generated from `apps/Frontend Plane/verevonv3/apps/gateway/src/**/*.rs` (production code only, `mod ...tests` blocks removed) and `src/shared/i18n/errors.ts`, on 2026-10-20. The machine check is `src/shared/i18n/gateway-error-codes.test.ts`; this file is the human list.

**251 codes** found in gateway source (shapes: `error("code", ..)`, `(StatusCode, "code", "msg")`, `("code", "Message")` pairs, `"code":` json keys, `code:` fields, `let code = if/match`, plus 8 helper-built codes the scan cannot see). Disposition: copy ADDED (this unit): 92; internal only (generic fallback): 23; copy exists: 114; section status in a 200 payload: 16; no copy on purpose: the message is shown: 1; not an error code: 3; no copy on purpose: the caller words it: 1; exempt until the 415 slice lands: 1.

No code is left without copy or a reason. "Copy" means a `{ no, en }` entry in `ERROR_COPY` (errors.ts), which `translateApiError` / `describeApiError` / `browserErrorMessage` look up by `error.code`.

## 1. How a code reaches the SPA

- Typed envelope `{ "error": { "code", "message", "details"?, "retryAfterSeconds"?, "blockReason"? } }`: built by `envelope::error` (almost every gateway-owned refusal). The SPA reads `code`, ignores `message` (English operator text).
- Flat shapes the SPA also reads (`extractError` in `shared/api/http.ts`): `{ "error": "text", "code": "x" }` (model-gateway, e.g. `no_speech_detected`), Better Auth `{ "code", "message" }` (auth routes are proxied verbatim).
- Bare/unknown bodies (plain text, HTML from a proxy, `{ "error": "text" }` with no code): `code` is null; the SPA answers by status class (section 6).
- Not an error response at all: `unavailable` sections inside a 200 Space payload (section 4), `warning` events of the onboarding crawl-preview SSE (section 4), `{ "type": "error" }` frames of the browser live WebSocket, `run_error` / `RUN_ERROR` events of the agent stream, and the chat stream failure codes (own presentation in `chat-normalizers.ts`).

## 2. Codes the gateway emits itself

Columns: code | where | status (nearest `StatusCode` in the source, a heuristic) | gateway message (English, for developers, never shown) | SPA copy.

### Account settings and API keys

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `api_key_create_failed` | domains/settings/api_keys.rs | BAD_GATEWAY | auth-core did not return the API key secret. | copy exists |
| `api_key_error` | domains/settings/api_keys.rs | BAD_GATEWAY/BAD_REQUEST | auth-core refused an API-key operation (message is auth-core's own, never shown) | copy ADDED (this unit) |
| `invalid_api_key_id` | domains/settings/api_keys.rs | BAD_REQUEST | API key id is required. | internal only (generic fallback): the key id comes from the listed key a person clicks; an empty one is a client bug |
| `invalid_api_key_name` | domains/settings/api_keys.rs | BAD_REQUEST | API key name is required. | copy ADDED (this unit) |

### Action dispatch (agent and UI actions)

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `imports_auth_unavailable` | domains/actions/dispatchers.rs, domains/chat/documents.rs +1 | SERVICE_UNAVAILABLE | Import authentication is temporarily unavailable. | copy exists |
| `invalid_input` | domains/actions/dispatchers.rs, domains/search.rs | BAD_REQUEST | knowledge.import_source requires a non-empty 'url' | copy exists |
| `no_active_org` | domains/actions/dispatchers.rs, domains/billing.rs +3 | BAD_REQUEST | No active organization is resolved for this session. | copy exists |
| `not_implemented` | domains/actions/handlers.rs | NOT_IMPLEMENTED | (built at run time) | copy exists |
| `owner_action_contract_invalid` | domains/actions/dispatchers.rs | BAD_GATEWAY | Conversation Core returned no valid human action contracts. | copy ADDED (this unit) |
| `shipping_auth_unavailable` | domains/actions/dispatchers.rs, domains/shipping.rs | SERVICE_UNAVAILABLE | Shipping authentication is temporarily unavailable. | copy exists |
| `ticket_operation_receipt_invalid` | domains/actions/dispatchers.rs | BAD_GATEWAY | Conversation Core returned no valid durable ticket operation receipt. | copy ADDED (this unit) |
| `upload_requires_multipart` | domains/actions/dispatchers.rs | UNPROCESSABLE_ENTITY | File upload must use POST /api/v1/knowledge/imports/upload (multipart form-data); it canno | copy exists |

### Chat history and documents

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `invalid_body` | domains/chat/documents.rs | BAD_REQUEST |  | copy ADDED (this unit) |
| `invalid_model_gateway_response` | domains/chat/history.rs, domains/spaces.rs | BAD_GATEWAY | Model Gateway returned an invalid thread listing. | copy ADDED (this unit) |
| `invalid_request` | domains/chat/history.rs, domains/chat/shared.rs +1 | BAD_REQUEST | (built at run time) | copy ADDED (this unit) |
| `model_request_timeout` | domains/chat/shared.rs | GATEWAY_TIMEOUT | The model did not respond before the request deadline | copy ADDED (this unit) |
| `org_scope_required` | domains/chat/history.rs, domains/social.rs +1 | FORBIDDEN | An authorized organization scope is required. | copy exists |
| `thread_not_found` | domains/chat/history.rs | NOT_FOUND | The chat thread no longer exists. | copy ADDED (this unit) |

### Composer / actions

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `active_membership_required` | domains/agent_actions.rs, domains/spaces.rs | FORBIDDEN | An active organization membership is required to create a Space schedule. | copy ADDED (this unit) |
| `forbidden` | domains/agent_actions.rs, domains/auth/protected.rs +7 | FORBIDDEN | Only organization admins can author skills. | copy exists |

### domains/ag_ui.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `invalid_agent_input` | domains/ag_ui.rs | BAD_REQUEST | (built at run time) | internal only (generic fallback): the agent-UI stream body is built by the client; the message names the missing field for developers |

### domains/agents_runs.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `thread_id_required` | domains/agents_runs.rs | BAD_REQUEST | thread_id query parameter is required | copy exists |

### domains/ai.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `speech_provider_not_allowed` | domains/ai.rs | BAD_REQUEST | Text-to-speech is available only through Azure Speech. | internal only (generic fallback): the SPA does not offer another text-to-speech provider |

### domains/billing.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `invalid_checkout_request` | domains/billing.rs, onboarding/actions/billing.rs | BAD_REQUEST | (built at run time) | copy ADDED (this unit) |

### domains/cost.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `cost_auth_unavailable` | domains/cost.rs | SERVICE_UNAVAILABLE | Cost service authentication is unavailable | copy exists |
| `invalid_cost_scope` | domains/cost.rs | BAD_REQUEST | scope must be me or organization | internal only (generic fallback): the usage scope is a fixed toggle in the SPA; any other value is a hand-made request |
| `organization_required` | domains/cost.rs, domains/ingestions/sources.rs +4 | FORBIDDEN / CONFLICT | An active organization is required | copy exists |

### domains/ingestions/shared.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `validation_error` | domains/ingestions/shared.rs, domains/mail_task.rs +7 | UNPROCESSABLE_ENTITY / BAD_REQUEST | (built at run time) | copy exists |

### domains/insights.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `insights_upstream_unavailable` | domains/insights.rs | BAD_GATEWAY |  | copy ADDED (this unit) |
| `invalid_insights_query` | domains/insights.rs | BAD_REQUEST |  | internal only (generic fallback): the insight filters are picked from fixed lists in the SPA |

### domains/integrations/browse.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `operation_not_allowed` | domains/integrations/browse.rs | BAD_REQUEST | (built at run time) | internal only (generic fallback): the integration browse operation is chosen by the SPA from a fixed list |
| `operation_required` | domains/integrations/browse.rs | BAD_REQUEST | operation is required | internal only (generic fallback): the integration browse operation is always sent by the SPA |

### domains/integrations/shared.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `integration_auth_unavailable` | domains/integrations/shared.rs | SERVICE_UNAVAILABLE | Integration authentication is temporarily unavailable. | copy exists |

### domains/leads.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `entitlement_required` | domains/leads.rs | PAYMENT_REQUIRED |  | copy exists |

### domains/mail_task.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `mail_task_pipeline_failed` | domains/mail_task.rs | BAD_GATEWAY | Mail-task turn execution failed. | internal only (generic fallback): the mail-task ingest endpoint is called service to service; no SPA code calls it |
| `mail_task_pipeline_not_configured` | domains/mail_task.rs | SERVICE_UNAVAILABLE | Mail-task turn execution is not yet configured on this deployment. | internal only (generic fallback): the mail-task ingest endpoint is called service to service; no SPA code calls it |
| `payload_too_large` | domains/mail_task.rs | PAYLOAD_TOO_LARGE | Request body exceeds the mail-task ingest limit. | internal only (generic fallback): the mail-task ingest endpoint is called service to service; no SPA code calls it |
| `rate_limited` | domains/mail_task.rs, rate_limit.rs | TOO_MANY_REQUESTS | Mail-task run rate exceeded for this user. | copy exists |
| `unauthorized` | domains/mail_task.rs, domains/orgs/members.rs +2 | UNAUTHORIZED | Invalid or missing service delegation. | copy exists |

### domains/monitoring.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `fingerprint_unavailable` | domains/monitoring.rs | BAD_GATEWAY | The page was fetched but no content fingerprint was returned. | copy exists |

### domains/navbar.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `invalid_color_scheme` | domains/navbar.rs | BAD_REQUEST | colorScheme must be a named theme color or a hexadecimal color. | copy ADDED (this unit) |
| `invalid_theme` | domains/navbar.rs | BAD_REQUEST | theme must be light, dark, or system. | copy ADDED (this unit) |

### domains/persona.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `persona_instructions_too_long` | domains/persona.rs | BAD_REQUEST | Persona instructions must be 1500 characters or fewer. | copy ADDED (this unit) |
| `persona_unavailable` | domains/persona.rs | SERVICE_UNAVAILABLE | Persona instructions could not be saved. | no copy on purpose: the caller words it: PersonaSection answers it with "could not save / could not load your persona instructions", which names the operation |

### domains/privacy.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `confirmation_required` | domains/privacy.rs | UNPROCESSABLE_ENTITY | Account erasure is irreversible. Resend with {\ | copy exists |

### domains/social.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `social_core_unavailable` | domains/social.rs | SERVICE_UNAVAILABLE | Social core is unavailable; the action was not performed. | copy exists |
| `upstream_shape_error` | domains/social.rs | BAD_GATEWAY | social-core returned an invalid post payload. | copy exists |

### domains/studio.rs

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `studio_project_not_found` | domains/studio.rs | NOT_FOUND | Studio project not found. | copy exists |

### In-app browser: sessions, runs, frames, artifacts, grants

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `browser_action_denied` | domains/browser.rs |  | Raw browser script evaluation is not exposed through the Verevon UI facade. | copy exists |
| `browser_artifact_failed` | domains/browser.rs | BAD_GATEWAY / GONE | The browser artifact could not be returned. | copy exists |
| `browser_artifact_missing` | domains/browser.rs |  | The browser artifact is no longer available. | copy exists |
| `browser_artifact_too_large` | domains/browser.rs | PAYLOAD_TOO_LARGE | The browser artifact is too large to preview. | copy exists |
| `browser_artifact_withheld` | domains/browser.rs |  | Zero Data Retention is on for this run, so no screenshot was captured. | copy exists |
| `browser_capacity_reached` | domains/browser.rs |  | Browser capacity is full. Try again shortly. | copy exists |
| `browser_frame_failed` | domains/browser.rs | BAD_GATEWAY | The live browser frame could not be returned. | copy exists |
| `browser_frame_invalid` | domains/browser.rs | BAD_GATEWAY | Quarry did not return a live browser frame. | copy exists |
| `browser_grant_denied` | domains/browser.rs | FORBIDDEN | Browser access was not granted for this session. | copy exists |
| `browser_grant_limit_reached` | domains/browser.rs | TOO_MANY_REQUESTS | Too many browser sessions are open. Close one and try again. | copy exists |
| `browser_grant_unavailable` | domains/browser.rs | SERVICE_UNAVAILABLE | Browser access is temporarily unavailable. | copy exists |
| `browser_host_not_grantable` | domains/browser.rs | BAD_REQUEST | This address cannot be opened in the in-app browser. | copy exists |
| `browser_human_takeover_active` | domains/browser.rs | CONFLICT / FORBIDDEN | Human takeover is active for this browser session. | copy exists |
| `browser_idempotency_key_reused` | domains/browser.rs | CONFLICT | This request key was already used for a different goal. Use a new key to start a new run. | copy exists |
| `browser_idempotency_key_run_ended` | domains/browser.rs | CONFLICT | The run for this request key has already ended. Use a new key to start another run. | copy exists |
| `browser_idempotency_unverified` | domains/browser.rs | SERVICE_UNAVAILABLE | The earlier run for this request key could not be confirmed. Try again shortly. | copy exists |
| `browser_observation_missing` | domains/browser.rs | CONFLICT | No browser observation is available for this session yet. | copy exists |
| `browser_owner_unavailable` | domains/browser.rs | FORBIDDEN | An authenticated organization is required for browser access. | copy exists |
| `browser_process_lost` | domains/browser.rs |  | The browser behind this session stopped unexpectedly. Start a new session. | copy exists |
| `browser_run_conflict` | domains/browser.rs | CONFLICT | The browser run id is already active. | copy exists |
| `browser_run_invalid` | domains/browser.rs | BAD_GATEWAY | Model Gateway did not return a browser run id. | copy exists |
| `browser_session_failed` | domains/browser.rs | CONFLICT | Quarry browser session is not available. | copy exists |
| `browser_session_grant_inactive` | domains/browser.rs | CONFLICT | This browser session's grant is no longer valid; it may have expired. Reopen the session t | copy exists |
| `browser_session_grant_missing` | domains/browser.rs | CONFLICT | This browser session has no browser grant. Reopen it to start an AI run. | copy exists |
| `browser_session_invalid` | domains/browser.rs | BAD_GATEWAY | Quarry did not return a browser run id. | copy exists |
| `browser_session_not_found` | domains/browser.rs | NOT_FOUND | The browser session is not active. | copy exists |
| `browser_session_reaped` | domains/browser.rs |  | This browser session was closed after being idle. Start a new session. | copy exists |
| `browser_site_blocked` | domains/browser.rs |  | This site blocks automated browsing. Take over the browser yourself or try another source. | copy exists |
| `browser_store_unavailable` | domains/browser.rs | SERVICE_UNAVAILABLE | Browser run ownership could not be verified. | copy exists |
| `browser_tab_limit_reached` | domains/browser.rs |  | The browser has too many tabs open. Close a tab or try again shortly. | copy exists |
| `browser_visual_observation_invalid` | domains/browser.rs | BAD_GATEWAY | The browser visual observation artifact is not valid JSON. | copy exists |
| `browser_ws_invalid_upstream` | domains/browser.rs | BAD_GATEWAY | The browser websocket upstream is unavailable. | copy exists |
| `delegated_auth_unavailable` | domains/browser.rs, domains/chat/shared.rs +4 | SERVICE_UNAVAILABLE | Required data-plane authentication is temporarily unavailable | copy exists |
| `invalid_browser_action` | domains/browser.rs |  | Navigate requires a URL. | copy exists |
| `invalid_browser_artifact` | domains/browser.rs | BAD_REQUEST | The requested browser artifact id is invalid. | copy exists |
| `invalid_browser_control_mode` | domains/browser.rs |  |  | copy exists |
| `invalid_browser_frame` | domains/browser.rs | BAD_REQUEST | Frame format must be jpeg or png. | copy exists |
| `invalid_browser_goal` | domains/browser.rs | BAD_REQUEST | A goal is required to start an AI browser run. | copy exists |
| `invalid_browser_profile` | domains/browser.rs | BAD_REQUEST | The requested browser profile id is invalid. | copy exists |
| `invalid_browser_run` | domains/browser.rs | BAD_REQUEST | The requested browser run id is invalid. | copy exists |
| `invalid_browser_session` | domains/browser.rs | BAD_REQUEST | The requested browser session id is invalid. | copy exists |
| `invalid_browser_tab` | domains/browser.rs | BAD_REQUEST | The requested browser tab id is invalid. | copy exists |
| `invalid_idempotency_key` | domains/browser.rs | BAD_REQUEST | The Idempotency-Key header must be 1 to 128 letters, digits, '-', '_', '.' or ':'. | copy exists |
| `invalid_profile_scope` | domains/browser.rs | BAD_REQUEST | A created browser profile cannot use the ephemeral scope. | copy exists |
| `invalid_profile_update` | domains/browser.rs | BAD_REQUEST | Provide a name and/or scope to update. | copy exists |
| `invalid_url` | domains/browser.rs, domains/knowledge/products.rs +3 | BAD_REQUEST | (built at run time) | copy exists |
| `model_gateway_unreachable` | domains/browser.rs | BAD_GATEWAY | Model Gateway is temporarily unavailable. | copy exists |
| `zdr_persistent_profile_forbidden` | domains/browser.rs |  | A Zero Data Retention session cannot use a persistent browser profile. | copy exists |

### Inbox: drafts and AI proposals

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `active_organization_required` | domains/inbox.rs, domains/tickets.rs | FORBIDDEN | Select an active organization first. | copy ADDED (this unit) |
| `ai_review_mode_required` | domains/inbox.rs | PRECONDITION_FAILED | Retained AI proposals require the organization's Support AI review mode. | copy ADDED (this unit) |
| `invalid_ai_text_proposal` | domains/inbox.rs | BAD_REQUEST | (built at run time) | internal only (generic fallback): proposal ids are generated by the client; a malformed one is a client bug |
| `retention_posture_unavailable` | domains/inbox.rs | SERVICE_UNAVAILABLE | Draft persistence is unavailable until the organization retention policy can be verified. | copy ADDED (this unit) |
| `support_ai_policy_unavailable` | domains/inbox.rs | SERVICE_UNAVAILABLE | AI review is unavailable until the organization policy can be verified. | copy ADDED (this unit) |
| `zdr_ai_proposal_forbidden` | domains/inbox.rs | PRECONDITION_FAILED | AI proposals are not retained while Zero Data Retention is enabled. | copy ADDED (this unit) |
| `zdr_draft_persistence_forbidden` | domains/inbox.rs | PRECONDITION_FAILED | Personal drafts are not retained while Zero Data Retention is enabled. | copy ADDED (this unit) |

### Knowledge base: documents, products, SharePoint sync

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `drive_id_required` | domains/knowledge/sync.rs | BAD_REQUEST | A SharePoint drive is required. | copy ADDED (this unit) |
| `extraction_failed` | domains/knowledge/products.rs | BAD_GATEWAY | Kunne ikke trekke ut produkter fra siden. | copy exists |
| `invalid_content` | domains/knowledge/documents.rs | BAD_REQUEST | content is required | copy ADDED (this unit) |
| `minio` | domains/knowledge/diagnostics.rs |  | MinIO object storage | not an error code: name of a dependency in the knowledge diagnostics readiness list, not an error |
| `no_products` | domains/knowledge/products.rs | BAD_REQUEST | Velg minst ett produkt å oppsummere. | copy ADDED (this unit) |
| `qdrant` | domains/knowledge/diagnostics.rs |  | Qdrant vectors | not an error code: name of a dependency in the knowledge diagnostics readiness list, not an error |
| `quickwit` | domains/knowledge/diagnostics.rs |  | Quickwit indexes | not an error code: name of a dependency in the knowledge diagnostics readiness list, not an error |
| `scrape_failed` | domains/knowledge/products.rs, domains/knowledge/quarry.rs | BAD_GATEWAY | Kunne ikke hente innhold fra siden. Den kan blokkere automatisert henting — sett en proxy- | copy exists |
| `sharepoint_drives_unavailable` | domains/knowledge/sync.rs | BAD_GATEWAY | Could not list document libraries for this site. | copy ADDED (this unit) |
| `sharepoint_folders_unavailable` | domains/knowledge/sync.rs | BAD_GATEWAY | Could not list folders for this document library. | copy ADDED (this unit) |
| `sharepoint_register_failed` | domains/knowledge/sync.rs | BAD_GATEWAY | SharePoint source could not be registered. | copy exists |
| `sharepoint_sites_unavailable` | domains/knowledge/sync.rs | BAD_GATEWAY | Could not list SharePoint sites. Connect a Microsoft 365 integration first. | copy ADDED (this unit) |
| `site_id_required` | domains/knowledge/sync.rs | BAD_REQUEST | A SharePoint site is required. | copy ADDED (this unit) |
| `summary_failed` | domains/knowledge/products.rs | BAD_GATEWAY | Kunne ikke lage AI-sammendrag. | copy ADDED (this unit) |

### Knowledge: scrape preview and crawl

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `crawl_backlog_full` | domains/knowledge/quarry.rs |  | Too many crawls are waiting for this organization. Try again shortly. | copy exists |
| `invalid_crawl_request` | domains/knowledge/quarry.rs | BAD_REQUEST | (built at run time) | copy exists |
| `scrape_rate_limited` | domains/knowledge/quarry.rs | TOO_MANY_REQUESTS | Nettstedet mottar for mange forespørsler akkurat nå. Vent litt og prøv igjen. | copy exists |
| `scrape_refused` | domains/knowledge/quarry.rs | FORBIDDEN | Nettstedet avviste forespørselen om å hente siden. Siden kan kreve innlogging eller være s | copy exists |
| `scrape_timeout` | domains/knowledge/quarry.rs |  | Siden brukte for lang tid på å svare — ofte en tung side eller bot-beskyttelse. Prøv en me | copy exists |
| `site_blocked` | domains/knowledge/quarry.rs | FORBIDDEN | Dette nettstedet stopper automatisert henting med en bot-sperre. Prøv en annen kilde, elle | copy exists |
| `unsupported_media` | domains/knowledge/quarry.rs | TOO_MANY_REQUESTS | Dette innholdet kan ikke vises som forhåndsvisning (for eksempel et bilde, et arkiv eller  | exempt until the 415 slice lands: WS5.G brings the gateway code and its copy entry (one line in errors.ts, uncommitted); the scan test accepts either order. |

### Onboarding

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `billing_admin_required` | onboarding/actions/billing.rs | FORBIDDEN | Only an organization owner or administrator can choose a plan or manage checkout. | copy exists |
| `connection_not_found` | onboarding/actions/connectors.rs | NOT_FOUND |  | copy ADDED (this unit) |
| `control_unreachable` | onboarding/crawl_preview/handler.rs |  |  | copy ADDED (this unit) |
| `crawl_failed` | onboarding/crawl_preview/normalize.rs |  |  | copy ADDED (this unit) |
| `create_failed` | onboarding/actions/organization.rs | BAD_GATEWAY | Identity service did not return an organization id. | copy exists |
| `empty_crawl` | onboarding/crawl_preview/handler.rs |  |  | copy ADDED (this unit) |
| `invalid_brand_theme` | onboarding/theme/mod.rs | BAD_REQUEST | Mode must be verevon or brand. | copy exists |
| `invalid_name` | onboarding/actions/organization.rs | BAD_REQUEST | Organization name is required. | copy ADDED (this unit) |
| `invalid_target_language` | onboarding/lookup/plan.rs | BAD_REQUEST | targetLanguage must be either en or nb. | copy exists |
| `onboarding_completion_failed` | onboarding/session/complete.rs | BAD_GATEWAY | Onboarding could not be completed safely. Your progress was kept; please retry. | copy ADDED (this unit) |
| `organization_activation_failed` | onboarding/actions/organization.rs | BAD_GATEWAY | Your organization was created, but the session could not activate it. Please retry. | copy exists |
| `organization_conflict` | onboarding/actions/organization.rs | CONFLICT | This organization already exists. Sign in with an invited administrator account or ask an  | copy exists |
| `organization_lookup_failed` | onboarding/actions/organization.rs | BAD_GATEWAY | Existing organizations could not be verified. Please retry; no new organization was create | copy exists |
| `organization_membership_stale` | onboarding/actions/billing.rs, onboarding/session/complete.rs +1 | CONFLICT | Your organization access changed. Ask an owner for a new invitation before checkout. | copy exists |
| `organization_membership_unavailable` | onboarding/actions/organization.rs | BAD_GATEWAY | The organization membership could not be verified. Please retry. | copy exists |
| `organization_not_ready` | onboarding/actions/billing.rs, onboarding/session/complete.rs | CONFLICT | An active, provisioned organization is required before checkout. | copy exists |
| `organization_owner_required` | onboarding/actions/organization.rs | CONFLICT | Only the organization owner can provision this workspace. Ask the owner for an invitation  | copy exists |
| `organization_provisioning_failed` | onboarding/actions/organization.rs | BAD_GATEWAY | Your organization could not be safely provisioned. No onboarding progress was lost; please | copy exists |
| `organization_recovery_required` | onboarding/actions/billing.rs, onboarding/session/complete.rs +1 | CONFLICT | Organization membership is still being reconciled. Retry before continuing to checkout. | copy exists |
| `recommendation_unavailable` | onboarding/lookup/plan.rs | SERVICE_UNAVAILABLE | Plan recommendation is temporarily unavailable. Choose a plan manually and retry later. | copy ADDED (this unit) |
| `translation_failed` | onboarding/lookup/plan.rs | BAD_GATEWAY | (built at run time) | copy exists |
| `translation_invalid_response` | onboarding/lookup/plan.rs (translation_failure) | BAD_GATEWAY | the plan-recommendation translation answered with something unreadable | copy ADDED (this unit) |
| `translation_unavailable` | onboarding/lookup/plan.rs (translation_failure) | BAD_GATEWAY | the plan-recommendation translation could not be reached | copy ADDED (this unit) |
| `unauthenticated` | onboarding/actions/organization.rs | UNAUTHORIZED | A signed-in session is required to create an organization. | copy exists |

### Organization: members, settings, instructions, quotas

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `instructions_too_long` | domains/orgs/instructions.rs, domains/spaces.rs | BAD_REQUEST | Organization instructions must be 4000 characters or fewer. | copy ADDED (this unit) |
| `instructions_unavailable` | domains/orgs/instructions.rs, domains/spaces.rs | SERVICE_UNAVAILABLE | Organization instructions could not be saved. | copy ADDED (this unit) |
| `member_not_found` | domains/orgs/members.rs | NOT_FOUND | Organization member not found. | copy ADDED (this unit) |
| `membership_lookup_failed` | domains/orgs/members.rs | BAD_GATEWAY | Unable to resolve the canonical organization member. | copy exists |
| `membership_lookup_invalid` | domains/orgs/members.rs | BAD_GATEWAY | Membership authority returned an invalid response. | copy exists |
| `membership_response_invalid` | domains/orgs/members.rs | BAD_GATEWAY | Membership authority returned an invalid response. | copy exists |
| `organization_access_denied` | domains/orgs/members.rs | FORBIDDEN | The path organization must match the live active membership and required role. | copy exists |
| `upstream_unavailable` | domains/orgs/members.rs, envelope.rs +2 | BAD_GATEWAY | Membership authority is unavailable. | copy exists |
| `zdr_gate_unavailable` | domains/orgs/settings.rs | SERVICE_UNAVAILABLE | Zero Data Retention cannot be turned on right now: the provider catalogue could not be che | copy exists |
| `zdr_no_attested_provider` | domains/orgs/settings.rs | CONFLICT | Zero Data Retention cannot be turned on: no AI provider in this environment has a recorded | copy exists |

### Proxy layer shared by every domain (transport failures, auth callbacks, artifact relay, stream start)

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `artifact_too_large` | upstream.rs | PAYLOAD_TOO_LARGE | That artifact is too large to display. | copy ADDED (this unit) |
| `artifact_unavailable` | upstream.rs | BAD_GATEWAY / INTERNAL_SERVER_ERROR | Could not load the artifact. | copy ADDED (this unit) |
| `auth_callback_response_invalid` | upstream.rs | BAD_GATEWAY | Authentication callback returned an invalid response. | copy exists |
| `auth_callback_response_too_large` | upstream.rs | BAD_GATEWAY | Authentication callback returned an invalid response. | copy exists |
| `auth_callback_target_rejected` | upstream.rs | BAD_GATEWAY | Authentication callback target was rejected. | copy exists |
| `auth_callback_unavailable` | upstream.rs | BAD_GATEWAY | Authentication callback service is unavailable. | copy exists |
| `conversation_membership_required` | upstream.rs | FORBIDDEN | An explicit active organization membership is required. | copy exists |
| `conversation_upstream_target_rejected` | upstream.rs | BAD_GATEWAY | Conversation request target does not match the configured conversation-core origin | copy exists |
| `integration_error` | upstream.rs (integration_error_code) | any | integration-core failed and sent no code the gateway recognises | no copy on purpose: the message is shown: the bounded public message of integration-core is shown on purpose by the Integrations settings (it names the failure of the provider); a generic sentence would hide it |
| `invalid_json` | upstream.rs | BAD_REQUEST | Conversation request body could not be serialized. | copy exists |
| `model_error` | upstream.rs |  |  | copy ADDED (this unit) |
| `no_sources_registered` | upstream.rs (INTEGRATION_ERROR_CODE_ALLOWLIST) | 409 | sync of a Microsoft connection with no SharePoint/OneDrive library chosen yet | copy ADDED (this unit) |
| `notification_upstream_target_rejected` | upstream.rs | BAD_GATEWAY | Notification request target does not match the configured notification-core origin | copy exists |
| `PLAN_REQUIRED` | upstream.rs (integration_error_code) | 402/403 | an integration call the organization's plan does not include | copy ADDED (this unit) |
| `upstream_error` | upstream.rs | BAD_GATEWAY | (built at run time) | copy exists |

### Search

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `invalid_query` | domains/search.rs, onboarding/lookup/brreg.rs | BAD_REQUEST | A non-empty query is required. | copy ADDED (this unit) |
| `search_provider_unconfigured` | domains/search.rs | NOT_IMPLEMENTED | Search is unavailable — no search provider is configured. | copy exists |
| `video_search_timeout` | domains/search.rs (as_video_error relabel of web_search_timeout) | GATEWAY_TIMEOUT | video search took too long | copy ADDED (this unit) |
| `video_search_unavailable` | domains/search.rs (as_video_error relabel of web_search_unavailable) | BAD_GATEWAY | video search could not be completed | copy exists |
| `web_search_timeout` | domains/search.rs | GATEWAY_TIMEOUT | Search timed out. | copy ADDED (this unit) |
| `web_search_unavailable` | domains/search.rs | BAD_GATEWAY | (built at run time) | copy ADDED (this unit) |

### Session and membership check (every protected route)

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `organization_membership_authority_unavailable` | middleware.rs | SERVICE_UNAVAILABLE | The organization membership authority is unavailable. | copy exists |
| `organization_membership_required` | middleware.rs | FORBIDDEN | An explicit active organization membership is required. | copy exists |
| `session_verification_unavailable` | middleware.rs | SERVICE_UNAVAILABLE | Session verification is temporarily unavailable. Your session is unchanged — retry shortly | copy ADDED (this unit) |

### Spaces and rooms (open, create, delete, agents, schedules, presence, activity)

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `agent_binding_state_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The agent's standing in this room could not be changed. Nothing was altered. | copy ADDED (this unit) |
| `agent_binding_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The agent could not be bound. Nothing was provisioned. | copy ADDED (this unit) |
| `agent_creation_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The agent could not be created. Nothing was provisioned. | copy ADDED (this unit) |
| `agent_invocation_blocked` | domains/spaces.rs | FORBIDDEN | This agent's binding blocks invocation in this Space. | copy ADDED (this unit) |
| `agent_membership_unconfirmed` | domains/spaces.rs | BAD_GATEWAY | The agent was created, but Control has not confirmed its room membership yet. It stays pen | copy ADDED (this unit) |
| `agent_mention_not_enabled` | domains/spaces.rs | FORBIDDEN | This agent's binding does not allow @-mention invocation. | copy ADDED (this unit) |
| `agent_mention_requires_space` | domains/spaces.rs | BAD_REQUEST | An agent can only be mentioned inside a Space. | copy ADDED (this unit) |
| `agent_ref_required` | domains/spaces.rs | BAD_REQUEST | An existing agent_ref is required to bind an agent. | internal only (generic fallback): the agent picker always sends the agent reference; only a hand-made request omits it |
| `agent_registry_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The organization's agent installations could not be resolved. | copy ADDED (this unit) |
| `agent_revocation_unconfirmed` | domains/spaces.rs | BAD_GATEWAY | The agent was revoked here, but Control has not confirmed the room's roster yet. It may st | copy ADDED (this unit) |
| `ambiguous_space_selection` | domains/spaces.rs | BAD_REQUEST | Conflicting Space references are not allowed. | internal only (generic fallback): the SPA sends one Space reference; two conflicting ones is a client bug |
| `approvals_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `binding_ref_required` | domains/spaces.rs | BAD_REQUEST | A binding reference is required. | internal only (generic fallback): the agent row always carries its binding reference; only a hand-made request omits it |
| `delivery_ledger_not_built` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `installable_agents_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The organization's agent definitions could not be loaded. | copy ADDED (this unit) |
| `invalid_action_input` | domains/spaces.rs | BAD_REQUEST | Action input must be an object. | internal only (generic fallback): the action runner sends an object; a non-object body is a client bug |
| `invalid_agent_binding_status` | domains/spaces.rs | BAD_REQUEST | An agent can only be paused or resumed here. Use DELETE to revoke it. | internal only (generic fallback): the SPA only offers pause and resume; any other status is a hand-made request |
| `invalid_agent_instructions` | domains/spaces.rs | BAD_REQUEST | Agent instructions must stay under 4000 characters. | copy ADDED (this unit) |
| `invalid_agent_name` | domains/spaces.rs | BAD_REQUEST | An agent name must be between 2 and 60 characters. | copy ADDED (this unit) |
| `invalid_chat_request` | domains/spaces.rs | BAD_REQUEST | Chat request must be an object. | internal only (generic fallback): the Space chat body is built by the client; a non-object is a client bug |
| `invalid_cron_request` | domains/spaces.rs | BAD_REQUEST | Cron request must be an object. | copy ADDED (this unit) |
| `invalid_deletion_request` | domains/spaces.rs | BAD_REQUEST | A Space reference and bounded idempotency key are required. | internal only (generic fallback): the Space reference and idempotency key are generated by the client |
| `invalid_import_request` | domains/spaces.rs | BAD_REQUEST | A source type is required for a Space import. | internal only (generic fallback): the import source type and key are chosen and generated by the client |
| `invalid_owner_grant_decision` | domains/spaces.rs | BAD_GATEWAY | Control returned an invalid owner grant decision. | copy ADDED (this unit) |
| `invalid_owner_grant_request` | domains/spaces.rs | BAD_REQUEST | Space, conversation, grant selection, and idempotency key are invalid. | internal only (generic fallback): the grant selection and idempotency key are generated by the client |
| `invalid_presence` | domains/spaces.rs | BAD_REQUEST | Presence is one of online, typing or offline. | internal only (generic fallback): the SPA only sends online, typing or offline |
| `invalid_presentation` | domains/spaces.rs | BAD_REQUEST | A title or a pin state is required. | copy ADDED (this unit) |
| `invalid_space` | domains/spaces.rs | BAD_REQUEST | A Space reference is required. | internal only (generic fallback): the Space reference comes from the route the person opened; an empty one is a client bug |
| `invalid_space_decision` | domains/spaces.rs | BAD_GATEWAY | Control returned an invalid Space decision. | copy ADDED (this unit) |
| `invalid_task_template` | domains/spaces.rs | BAD_REQUEST | (built at run time) | copy ADDED (this unit) |
| `knowledge_binding_unavailable` | domains/spaces.rs | FORBIDDEN |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `knowledge_read_not_authorized` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `knowledge_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `member_required` | domains/spaces.rs | BAD_REQUEST | A member is required. | copy ADDED (this unit) |
| `mentioned_agent_not_bound` | domains/spaces.rs | NOT_FOUND | This agent is not bound to this Space. | copy ADDED (this unit) |
| `operations_endpoint_unavailable` | domains/spaces.rs | NOT_FOUND |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `operations_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `owner_grant_role_required` | domains/spaces.rs | FORBIDDEN | An organization owner or admin must manage agent action grants. | copy ADDED (this unit) |
| `presence_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | Who else is in this room could not be read. Nothing else was altered. | copy ADDED (this unit) |
| `processes_read_not_authorized` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `processes_session_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `processes_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `read_marker_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | Where you last caught up could not be recorded. Nothing else was altered. | copy ADDED (this unit) |
| `room_membership_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The room's members could not be changed. Nothing was altered. | copy ADDED (this unit) |
| `room_membership_unconfirmed` | domains/spaces.rs | BAD_GATEWAY | The change was recorded, but Control has not confirmed the room's members yet. | copy ADDED (this unit) |
| `room_name_required` | domains/spaces.rs | BAD_REQUEST | A room needs a name. | copy ADDED (this unit) |
| `runs_read_not_authorized` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `runs_session_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `runs_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `schedules_session_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `schedules_upstream_unavailable` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |
| `scoped_append_content_required` | domains/spaces.rs | BAD_REQUEST | A scoped thread append requires message content. | copy ADDED (this unit) |
| `space_deletion_not_found` | domains/spaces.rs | NOT_FOUND | Space deletion request is not available. | copy ADDED (this unit) |
| `space_deletion_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | Space deletion request is unavailable. | copy ADDED (this unit) |
| `space_lifecycle_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | Personal Space lifecycle is unavailable. | copy ADDED (this unit) |
| `space_not_found` | domains/spaces.rs | NOT_FOUND | Personal Space is not available for deletion. | copy ADDED (this unit) |
| `space_provisioning_unavailable` | domains/spaces.rs | SERVICE_UNAVAILABLE | The room could not be created. Nothing was provisioned. | copy ADDED (this unit) |
| `space_read_not_authorized` | domains/spaces.rs | FORBIDDEN | Reading this Space's shared conversation is not authorized. | copy ADDED (this unit) |
| `space_required` | domains/spaces.rs | FORBIDDEN | An active personal Space is required to create a schedule. | copy ADDED (this unit) |
| `space_role_cannot_create_agent` | domains/spaces.rs | FORBIDDEN | Only a Space owner or manager can add an agent here. | copy ADDED (this unit) |
| `space_role_cannot_edit_instructions` | domains/spaces.rs | FORBIDDEN | Only a Space editor, manager, or owner can change its instructions. | copy ADDED (this unit) |
| `task_template_required` | domains/spaces.rs | BAD_REQUEST | A schedule requires an exact task template. | copy ADDED (this unit) |
| `thread_presentation_owner_only` | domains/spaces.rs | FORBIDDEN | Only the member who started this post can retitle or pin it. | copy ADDED (this unit) |
| `thread_presentation_unavailable` | domains/spaces.rs | FORBIDDEN | The post could not be changed. Nothing was altered. | copy ADDED (this unit) |
| `unsupported_space_kind` | domains/spaces.rs | BAD_REQUEST | Only a personal Space or a room can be created here. | internal only (generic fallback): the SPA only creates a personal Space or a room |
| `watches_not_built` | domains/spaces.rs |  |  | section status in a 200 payload: The Space panels word each one themselves (SpaceActivityPanel, SpaceWorkPanel, SpaceKnowledgePanel); it never goes through translateApiError. |

### Tickets

| code | where | status | gateway message | SPA copy |
|---|---|---|---|---|
| `support_recurrence_permission_required` | domains/tickets.rs | FORBIDDEN | This role does not have permission to view similarity candidates. | copy ADDED (this unit) |
| `support_recurrence_policy_unavailable` | domains/tickets.rs | SERVICE_UNAVAILABLE | Similarity candidates are unavailable until the organization policy can be verified. | copy ADDED (this unit) |
| `zdr_recurrence_forbidden` | domains/tickets.rs | PRECONDITION_FAILED | Similarity candidates are not available while Zero Data Retention is enabled. | copy ADDED (this unit) |

## 3. Codes a helper builds (the scan cannot see them, a test pins them)

Listed in `HELPER_EMITTED` of the test, which also checks that each literal is still in the gateway source: `api_key_error`, `integration_error`, `no_sources_registered`, `PLAN_REQUIRED`, `translation_unavailable`, `translation_invalid_response`, `video_search_unavailable`, `video_search_timeout`. All have copy except integration_error (its message is shown on purpose).

## 4. Not error responses, but a person reads them

| shape | codes | who words it |
|---|---|---|
| `unavailable` sections of a 200 Space payload (`spaces.rs`) | `approvals_upstream_unavailable`, `delivery_ledger_not_built`, `knowledge_binding_unavailable`, `knowledge_read_not_authorized`, `knowledge_upstream_unavailable`, `operations_endpoint_unavailable`, `operations_upstream_unavailable`, `processes_read_not_authorized`, `processes_session_unavailable`, `processes_upstream_unavailable`, `runs_read_not_authorized`, `runs_session_unavailable`, `runs_upstream_unavailable`, `schedules_session_unavailable`, `schedules_upstream_unavailable`, `watches_not_built` | the Space panels (`SpaceActivityPanel`, `SpaceWorkPanel`, `SpaceKnowledgePanel`); the test checks each has a case there |
| onboarding crawl-preview SSE `warning` event | `empty_crawl`, `control_unreachable`, `crawl_failed` (+ the client's own `invalid_sse_payload`, `invalid_sse_packet`) | **fixed in this unit**: `OnboardingPage` showed `payload.message` (English operator text or an upstream error string); it now translates by code, and an unknown code gets a neutral sentence |
| agent stream `error` / `RUN_ERROR` events, chat stream failures | `rate_limited`, `subscription_*`, `prompt_too_long`, `model_plane_unavailable`, `inference_stream_*`, `session_liveness_failed`, `assistant_persist_failed`, `agentic_subscription_disabled`, `artifact_revision_conflict`, `response_validation_unavailable` | `chat-normalizers.ts` `streamFailurePresentation` (own copy, own presentation); not part of ERROR_COPY |
| stream start refused (`sse.ts`, status not 2xx) | `unauthorized` or `model_error` (ag_ui.rs/upstream.rs map the model gateway's status to them), `upstream_error` | `describeApiError`/`translateApiError` (both have copy now) |
| the gateway's inbound 429 (`rate_limit.rs`) | was plain text `rate limit exceeded` with no code; **now the typed envelope** `rate_limited` + `retryAfterSeconds` + `Retry-After` | `ERROR_COPY.rate_limited` (retryable: "try again in N seconds") |

## 5. Codes the gateway passes through unchanged

The gateway forwards upstream error bodies verbatim on these paths (`proxy_json`, `proxy_bearer_json`, `proxy_conversation_json`, `proxy_notification_json`, `proxy_session_json`, `forward_quarry_failure`, `proxy_auth*`), so the plane's own code reaches the SPA. A body with no `error` (empty or non-JSON) becomes `upstream_error` (502) in `upstream.rs`.

| plane / path | codes | SPA copy |
|---|---|---|
| Quarry (browser calls via `forward_quarry_failure`; `quarry-core::ErrorCode`, 20 codes) | `CHALLENGE_DETECTED` (gateway rewrites to `browser_site_blocked`), `USER_TOOK_OVER` (rewritten to `browser_human_takeover_active`), `RATE_LIMITED` (the cap reasons are rewritten to `browser_capacity_reached` / `browser_tab_limit_reached`), `DRIVER_FAILED` (`session_reaped` / `browser_process_lost` reasons rewritten to 410), `TIMEOUT`, `SECURITY_BLOCKED`, `RUNTIME_NOT_READY`, `CHECKPOINT_LOST`, `TARGET_REPAIR_REQUIRED`, `UPSTREAM_BLOCKED`, `UNSUPPORTED`, `UNSUPPORTED_MEDIA`, `BUDGET_EXHAUSTED` | copy added for the 13 a person can act on (11 new + the two that already had); `BAD_REQUEST`, `UNAUTHORIZED`, `FORBIDDEN`, `NOT_FOUND`, `CONFLICT`, `ACTION_UNKNOWN`, `INTERNAL` are answered by the status class (the gateway sanitises actions itself, so `ACTION_UNKNOWN` cannot come from a person) |
| Quarry scrape preview (`classify_scrape_failure`) | never passed through: always rewritten to `site_blocked`, `scrape_refused`, `scrape_rate_limited`, `scrape_timeout`, `scrape_failed`, `crawl_backlog_full` | copy exists |
| auth-core (Better Auth, `/api/v1/auth/*` proxied verbatim) | `EMAIL_NOT_VERIFIED`, `INVALID_EMAIL_OR_PASSWORD`, `INVALID_OTP`, `INVITATION_NOT_FOUND` (copy existed); `INVITATION_AUTHORITY_UNAVAILABLE`, `TOO_MANY_REQUESTS`, `USER_ALREADY_EXISTS`, `DISPOSABLE_EMAIL_BLOCKED` (copy added); `USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION` / `USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION` are swallowed by `orgs/members.rs` into a 200; other Better Auth codes (`PASSWORD_TOO_SHORT`, `INVALID_TOKEN`, ...) are NOT covered: `AuthPage` shows Better Auth's English message (not changed here, see not-done) |
| integration-core (`proxy_integration_json` normalises) | gateway codes `PLAN_REQUIRED`, `integration_error`, `no_sources_registered`, `personal_connection_forbidden`, `personal_connection_no_delegated_user` | copy exists / added |
| model-gateway (budget, voice, chat) | `budget_exceeded`, `budget_unavailable`, `no_speech_detected`, `model_request_timeout`, `subscription_*`, flat `{ "error": "text" }` bodies with no code (browser run control: "run not found", "run access denied", ...) | `budget_*` and `model_request_timeout` have copy; `no_speech_detected` is handled by `voice-mode-state.ts`; the code-less flat bodies fall to the status class and the caller's sentence |
| conversation-core, notification-core, user-core, org-core, billing-core, documents-api, ... (`proxy_json`) | their own codes (`conflict`, `not_found`, `forbidden`, `validation_error`, `invalid_input`, ...) | the generic ones have copy (`forbidden`, `validation_error`, `invalid_input`, `not_implemented`); a plane code with no entry is answered by the status class, then the caller's sentence. The gateway cannot list these (they live in other planes); the test covers every code the GATEWAY names |

## 6. What the SPA does with a failed request, by status

`requestJson` / `requestForm` (`shared/api/http.ts`) throw `ApiError { status, code, details, retryAfterSeconds, resumable, blockReason }`; a lost connection is the browser's own `TypeError` (no response). `translateApiError` and `describeApiError` (errors.ts) turn either into copy; both answer the same since this unit.

| case | what happens | where tested |
|---|---|---|
| 401 `unauthorized` (gateway's own session check) | `signalSessionExpiry` dispatches `verevon:session-expired` once per failed request; `AppShell` clears the session and the route guard returns to sign-in. Now from `requestJson`, `requestForm` and the onboarding crawl-preview stream (before: `requestJson` only). Never on `/api/v1/auth/*` (a wrong password stays on its form; the session probe cannot loop). Streams (`sse.ts`) do not signal: the agent stream maps the model gateway's own 401 to the same code. Text: "Du må logge inn på nytt." | `http.test.ts`, `stream.test.ts` |
| 401 with another code (Better Auth `INVALID_OTP`, ...) | stays on the form; no event | `http.test.ts` |
| 503 `session_verification_unavailable` | never ends the session (`session-store.ts` `isDefinitiveSignOut`); copy says the session is unchanged | `http.test.ts` |
| 403 | code copy if any, else "Du har ikke tilgang til å gjøre dette." (before: the caller's "could not do X" through `translateApiError`) | `browser-errors.test.ts`, `gateway-error-codes.test.ts` |
| 404, 409 | code copy if any, else the caller's sentence (it names what failed; there is no recovery of its own). A 409 `USER_TOOK_OVER` / `browser_human_takeover_active` is the `takeover` kind. `ConversationPanel` handles its own 409 lease conflict. | `browser-errors.test.ts` |
| 422 / 400 | `validation_error`, `invalid_input` copy ("Noen av feltene er ikke gyldige"). **The gateway sends no per-field details on a 422** (message only), so there are no field errors to show; this is a gateway contract gap, not an SPA one. | `gateway-error-codes.test.ts` |
| 429 | `rate_limited` copy with "Prøv igjen om N sekunder" from `retryAfterSeconds` or the `Retry-After` header, for any code (own copy wins, a retryable one gets the sentence). The gateway's own limiter answered text/plain before; now typed. | `http.test.ts`, `rate_limit.rs` test |
| 5xx with a code | code copy (`upstream_unavailable`, ...) | |
| 5xx without a code (HTML from a proxy, bare text) | the caller's sentence; never the body | `http.test.ts` |
| network failure (`TypeError`) | "Fikk ikke kontakt med tjenesten. Sjekk nettverket ditt og prøv igjen." (before: `Failed to fetch` shown raw by every `err instanceof Error ? err.message` site) | `http.test.ts`, `gateway-error-codes.test.ts` |
| raw `err.message` shown | 44 single-line catch sites in 18 feature files converted to `translateApiError(err, i18n.tr, { no, en })` (knowledge composer, search, settings, social, finetune, inbox, SharePoint pickers, ...). Left as they were: see not-done. | typecheck + vitest |

## 7. Left out on purpose

- `AuthPage.tsx` (6 sites) and `TwoFactorEnrollmentSection.tsx`: show Better Auth's English `message`; the right fix is a copy table for Better Auth's code set, which auth-core owns.
- Sites whose fallback is not a bilingual `i18n.tr(..)` pair or whose component has no `i18n` in scope (`DashboardCards`, `PluginsSection`, `NexiCheckout`, `RouterPolicyPage`, `use-chat-controller` approvals, `StudioPage`, `OrgDeletion*`, `DashboardComposer`).
- `quarry-client.ts` (generated SDK `ResponseError`): no session-expiry signal and no ApiError; it has 21 pre-existing type errors.
- `BrowserTimelineDetail.fetchArtifactJson` (raw `fetch`, Norwegian text with the status number).
