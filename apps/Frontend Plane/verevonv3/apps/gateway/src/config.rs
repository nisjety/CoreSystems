use std::{env, time::Duration};

use anyhow::Result;
use axum::http::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    HeaderName, HeaderValue, Method as HttpMethod,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::audience_tokens::{new_audience_token_cache, AudienceTokenCache};

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) client: reqwest::Client,
    /// Client for SSE/streaming upstreams — NO overall request timeout. The shared
    /// `client` has a 25s ceiling that would sever long-lived event streams (chat,
    /// ag-ui, search answer, crawl events) mid-stream and drop their terminal events.
    pub(crate) streaming_client: reqwest::Client,
    pub(crate) internal_api_key: String,
    /// Ownership PR-4/PR-6 honesty gate. The SAME `CONTROL_PLANE_ENFORCEMENT`
    /// value retrieval-engine reads (off|permissive|strict). The frontend's
    /// privacy affordances (badges/ShareDialog) render ONLY when this is
    /// "strict" AND viewer identity is live — surfaced via GET /api/v1/ownership/status.
    pub(crate) enforcement_mode: String,
    /// `BROWSER_GRANT_ENFORCEMENT` (off|permissive|strict): whether the
    /// in-app browser obtains a BrowserBroker session grant, and whether a
    /// failure to obtain one stops the session (WS1.B).
    pub(crate) browser_grant_mode: BrowserGrantMode,
    /// `BROWSER_HUMAN_BROWSE` (WS8.D): whether a session asks for a person's open-web
    /// `human_browse` grant. Only the literal `true` turns it on, in every environment (see
    /// [`exact_true_flag`]).
    pub(crate) browser_human_browse: bool,
    /// `BROWSER_LIVE_VIEW_ONLY` (W3-E3.4-5, W3-E3.4-5b): whether the owner's view-only live
    /// routes (`/api/v1/browser/sessions/{id}/view/frames/ws` and `.../view/frames/stream`) are
    /// served. Only the literal `true` turns it on (see [`env_exact_true`]); while off they answer
    /// 404. Quarry honours the
    /// scope only with `QUARRY_EDGE__LIVE_VIEW_ONLY` on, so the gateway also drops input frames
    /// itself on the socket; the SSE stream carries no client frames at all.
    pub(crate) browser_live_view_only: bool,
    /// `BROWSER_CONSENT_API` (Q3-E3.2-e3-gw): whether the viewer's per-origin browser consent
    /// decisions can be listed and revoked (`/api/v1/browser/consent`). Read once here, never in a
    /// handler. Exactly `true` enables it; while off the routes are absent. user-core also needs
    /// its own browser-consent switch on, or it answers `browser_consent_disabled`.
    pub(crate) browser_consent_api: bool,
    /// `BROWSER_DOWNLOADS` (Q3-E3.4-7b): whether a browser session's governed downloads can be listed
    /// (`GET /api/v1/browser/sessions/{id}/downloads`) and sent to the knowledge base
    /// (`browser_download.send_to_knowledge`). Read once here, never in a handler. Exactly `true`
    /// enables it; while off both answer 404 and nothing goes upstream. Quarry also needs
    /// `QUARRY_EDGE__DOWNLOADS` and `QUARRY_EDGE__DOWNLOAD_HANDOFF` on.
    pub(crate) browser_downloads: bool,
    /// `BROWSER_RECIPES` (Q4-E4.2-4a): whether a browser session's receipts compile canonically
    /// (version 2) and can be saved as a recipe (`POST /api/v1/browser/sessions/{id}/recipes`,
    /// `browser_evidence.save_recipe`) and the saved recipes listed (`GET /api/v1/browser/recipes`)
    /// and read one at a time with their slots (`GET /api/v1/browser/recipes/{recipe_id}`, QF-29).
    /// Read once here, never in a handler. Only the literal `true` turns it on (see
    /// [`exact_true_flag`]); while off the routes are absent and the action answers 404. Quarry also
    /// needs `QUARRY_EDGE__PROCEDURE_CANONICAL` on and capability-core `PROCEDURES_API` on.
    pub(crate) browser_recipes: bool,
    /// `BROWSER_REGION_ATTACH` (E3.3-d, QF-7): whether a dragged page region can be attached
    /// (`POST /api/v1/browser/sessions/{id}/attachments/region`, `browser_region.attach`). Read
    /// once here, never in a handler. Only the literal `true` turns it on (see [`env_exact_true`]);
    /// while off the route is absent and the action answers 404.
    pub(crate) browser_region_attach: bool,
    /// `BROWSER_TAB_MENTIONS` (E3.3-c, QF-7): whether a turn's `@tab` mentions can be re-resolved
    /// (`POST /api/v1/browser/sessions/{id}/context/tab-mentions`,
    /// `browser_context.resolve_tab_mentions`). Read once here, never in a handler. Only the literal
    /// `true` turns it on (see [`env_exact_true`]); while off the route is absent and the action
    /// answers 404.
    pub(crate) browser_tab_mentions: bool,
    /// `BROWSER_POLICY_ADMIN_API` (WS7.H): whether the organization browser policy actions
    /// (`browser.get_org_policy` and `browser.set_org_policy`) are served. Read once here, never in a
    /// handler (V3e-bff-3). Only the literal `true` turns it on (see [`env_exact_true`]); while off both
    /// answer 404 `browser_policy_admin_disabled` before any other check.
    pub(crate) browser_policy_admin_api: bool,
    /// `BROWSER_TURN_CONTEXT` (QF-8, E3.3-c/-d): whether a chat turn may carry browser page
    /// context. Read once here, never in a handler. Exactly `true` enables it. Off or on, the
    /// chat send path drops any client-built `browser_tab_refs` / `browser_regions`; on, the
    /// gateway re-resolves the tab refs against Quarry and verifies each region's attach token,
    /// and writes back only what it vouched for.
    pub(crate) browser_turn_context: bool,
    /// `BROWSER_REGION_ATTACH_KEY` (QF-8): the HMAC key of the stateless region attach token.
    /// Empty means no token is issued and every region on a chat turn is refused. Never logged.
    pub(crate) browser_region_attach_key: String,
    pub(crate) auth_core_url: String,
    pub(crate) verevon_public_origin: String,
    pub(crate) session_core_url: String,
    pub(crate) session_core_service_token: String,
    pub(crate) user_core_service_token: String,
    pub(crate) billing_core_url: String,
    pub(crate) billing_core_service_token: String,
    pub(crate) org_core_url: String,
    pub(crate) org_core_service_token: String,
    pub(crate) integration_core_url: String,
    pub(crate) audit_core_url: String,
    pub(crate) audit_core_service_token: String,
    pub(crate) insight_core_url: String,
    pub(crate) leads_core_url: String,
    /// The anbud core (tender matching). Its key is the gateway's own credential for that core: the browser
    /// never sees it, and an empty key answers the anbud routes with `not_configured`.
    pub(crate) anbud_core_url: String,
    pub(crate) anbud_api_key: String,
    pub(crate) anbud_api_key_header: String,
    pub(crate) marketing_core_url: String,
    pub(crate) marketing_core_service_key: String,
    /// Temporary single-org Marketing Core deployment binding, never a fallback
    /// tenant. Sensitive routes compare it to live session membership.
    pub(crate) marketing_core_organization_id: String,
    pub(crate) shipping_core_url: String,
    pub(crate) user_core_url: String,
    /// Application Plane's Convex deployment. Read from the environment once,
    /// here, like every other upstream — not with `std::env::var` inside a
    /// handler. A per-call read is a process-wide global that two requests (or
    /// two tests) cannot point at different places, which is exactly how a
    /// test that touches none of this began failing when unrelated tests were
    /// added beside it.
    pub(crate) application_convex_url: String,
    pub(crate) application_convex_service_key: String,
    pub(crate) graph_index_url: String,
    pub(crate) quarry_edge_url: String,
    pub(crate) model_recommend_url: String,
    pub(crate) model_gateway_url: String,
    pub(crate) cost_core_url: String,
    pub(crate) model_gateway_dev_bearer: String,
    pub(crate) inference_core_url: String,
    pub(crate) documents_api_url: String,
    pub(crate) retrieval_engine_url: String,
    pub(crate) wiki_store_url: String,
    pub(crate) embedding_engine_url: String,
    pub(crate) quickwit_adapter_url: String,
    pub(crate) finspo_core_url: String,
    pub(crate) imports_api_url: String,
    pub(crate) notification_core_url: String,
    pub(crate) notification_core_service_token: String,
    pub(crate) information_core_url: String,
    pub(crate) conversation_core_url: String,
    pub(crate) conversation_core_service_token: String,
    pub(crate) social_core_url: String,
    /// Never read in production, and that is the point: video search used to
    /// call SearXNG straight from the BFF, bypassing quarry-edge's router,
    /// cache, host-diversity cap, org scoping and billing. That bypass is
    /// closed — every search vertical now proxies through quarry-edge — and
    /// `search_videos`' regression test keeps it closed by pointing this field
    /// at a mock that mounts nothing and asserting the mock received no
    /// request at all. The tripwire only works while the field still exists,
    /// so it is deliberately retained rather than deleted with its warning.
    #[allow(
        dead_code,
        reason = "tripwire for the closed direct-to-SearXNG bypass; see doc comment"
    )]
    pub(crate) searxng_url: String,
    pub(crate) autocomplete_core_url: String,
    pub(crate) autocomplete_token: String,
    pub(crate) zammad_api_url: String,
    /// Support Plane (apps/Support Plane) connection details handed to the SPA
    /// for `@verevon/remote-core`. Empty = not configured; there is deliberately
    /// no fallback host. `remote_support_server_public_key` is RustDesk's public
    /// access key — public by design, not a secret.
    pub(crate) remote_support_rendezvous_url: String,
    pub(crate) remote_support_relay_url: String,
    pub(crate) remote_support_server_public_key: String,
    pub(crate) zammad_api_token: String,
    pub(crate) audience_token_cache: AudienceTokenCache,
    /// WS15.S0: what the Ingestions schedules surface may offer. Read once at
    /// startup from `QUARRY_SCHEDULE_CAPABILITIES`, never per request.
    pub(crate) schedule_capabilities: ScheduleCapabilities,
    /// Retained only for legacy browser-domain unit fixtures. Runtime browser
    /// authority and presentation state live in Quarry-v2.
    #[cfg(test)]
    pub(crate) browser_run_store: crate::domains::browser::BrowserRunStore,
    pub(crate) cache: crate::cache::ResultCache,
    /// Fleet-wide inbound rate limiter. Backed by the same Dragonfly connection
    /// as `cache` (distributed token buckets), degrading to an in-process bucket
    /// when the cache is disabled. Wired as an `Extension` layer in `main.rs`.
    pub(crate) rate_limiter: crate::rate_limit::RateLimiter,
    pub(crate) studio_store: crate::domains::studio::StudioStore,
    pub(crate) allow_dev_actor_headers: bool,
    pub(crate) allow_dev_auth_bypass: bool,
    /// WS03 S8-A: whether `POST /api/v1/orchestration/runs/{run_id}/continue` forwards the PARK
    /// form of its body, an exactly empty `approval_id` with a claim token, which resumes a run
    /// the executor parked. Read once from `VEREVON_GATEWAY_CONTINUE_PARK_ENABLED` (exactly
    /// `true`, default off) in `build_state`, never in a handler. Off, the route is byte for byte
    /// what it was: a blank `approval_id` is the 400 `invalid_request`. Flip it together with
    /// model-gateway's `EXECUTION_LOOP_PARKING_ENABLED`: on in front of a gateway with parking
    /// off, the park form is answered 400 `continue_incomplete` (reported as `model_error`).
    pub(crate) continue_park_form: bool,
    /// WS14 S5: whether `POST /api/v1/orchestration/runs/{run_id}/continue` forwards the QUESTION
    /// form of its body (a `question_id` with the answers or a skip), which continues a run paused
    /// for its owner's answer. Read once from `VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED` (exactly
    /// `true`, default off) in `build_state`, never in a handler. Off, a body naming `question_id`
    /// is the 400 `invalid_request`, and every other body is exactly what it was. Flip it together
    /// with model-gateway's question-continue switch.
    pub(crate) continue_question_form: bool,
    /// WS10-S5: whether the org model-entitlement admin routes (`/api/v1/orgs/{id}/model-entitlement`
    /// and its `explain`), the `org.set_model_entitlement` action's write and the ZDR switch
    /// gate's `hidden_count` read are live. Read once from `MODEL_ENTITLEMENT_ADMIN_ENABLED`
    /// (exactly `true`, default off) in `build_state`, never in a handler. Off, GET answers
    /// `{"enabled": false}` so the SPA hides the section, every write and explain is a 404, and
    /// the ZDR gate answers as it did before the policy existed.
    pub(crate) model_entitlement_admin_enabled: bool,
    /// `GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED` (ZDR Z-03c): whether the one org-posture reader
    /// (`zdr::org_posture`, `zdr::org_posture_live`) answers from org-core's computed
    /// `retention_posture.effective_zdr` instead of parsing the stored intent
    /// (`metadata.interactiveRetention.zdr`) itself. Read once from the environment in
    /// `build_state` (trimmed, exactly `true` turns it on), never in a handler. Off, the readers
    /// keep the intent-only rule. On, a record that lacks a boolean `effective_zdr` (org-core older
    /// than ZDR-Z-03a) is `Unknown`, which every caller already treats as fail closed. A rollout
    /// control, never an authorization input: org-core computes the posture.
    pub(crate) zdr_effective_posture_read: bool,
    /// `ZDR_WAIVER_UI_ENABLED` (ZDR Z-19a): whether the BFF asks Auth Core for a retention decision
    /// (`retention_decision::turn_retention`) on a subscription chat turn. Read once from the
    /// environment in `build_state` (trimmed, exactly `true` turns it on), never in a handler. Off,
    /// `turn_retention` answers `Unchanged` and no call is made. A rollout control, never an
    /// authorization input: Auth Core decides and signs; the BFF only asks.
    pub(crate) zdr_waiver_ui_enabled: bool,
    /// `GATEWAY_RETENTION_DECISION_SERVICE_API_KEY` (ZDR Z-19a): the gateway's own service credential
    /// for the Auth Core retention-decision policy token (`verevon-gateway-retention-decision`).
    /// Trimmed; a `change-me*` placeholder refuses boot unless `ALLOW_INSECURE_DEV_DEFAULTS=1`
    /// (`upstream_secret`); `None` when unset or blank, which makes every decision refuse with
    /// `zdr_decision_unavailable`. A secret: never logged, never sent to a browser. `AppState` has no
    /// `Debug` impl, so it cannot be printed through the state either.
    pub(crate) retention_decision_service_api_key: Option<String>,
    /// `AGENT_DEFINITION_VERSIONS` (EP-6 S7/S8): the agent builder routes (draft, publish, versions)
    /// and the draft preview of a chat body (`agent_preview`). Read once from the environment in
    /// `build_state`, exactly `true` turns it on, never by default in development. Off, the builder
    /// routes do not exist (the gateway answers 404 as it did before) and a chat body's
    /// `agent_preview` is left as it came. A rollout control, never an authorization input: the
    /// builder's own access check and Convex decide who may read or write a draft.
    pub(crate) agent_definition_versions: bool,
    /// `CHAT_PROCESS_OUTPUT_READ_ENABLED` (W4-P3): the owner's read of one background process's output,
    /// `GET /api/v1/chat/runs/{run_id}/processes/{pid}/output`. Read once from the environment in
    /// `build_state`, exactly `true` turns it on. Off, the route answers 404 `process_output_disabled`.
    /// A rollout control, never an authorization input: Control's sandbox-manager read decision and
    /// Model Gateway's run-owner check decide what a caller reads.
    pub(crate) chat_process_output_enabled: bool,
    /// `CHAT_SNAPSHOTS_ENABLED` (W4-SH-6): the owner's governed snapshot links, `POST/GET
    /// /api/v1/chat/threads/{thread_id}/snapshots`, `DELETE /api/v1/chat/snapshots/{id}`, the grant routes
    /// under `/api/v1/chat/snapshots/{id}/grants` and the member's read `GET /api/v1/chat/shared-snapshots/{id}`.
    /// Read once from the environment in `build_state`, exactly `true` turns it on. Off, every one of those routes
    /// answers 404 `chat_snapshots_disabled`. A rollout control, never an authorization input: Session Core proves the
    /// owner, Control holds the grants, and Model Gateway has its own switch.
    pub(crate) chat_snapshots_enabled: bool,
    /// `EVENT_TRIGGERS_UI_ENABLED` (W4-T7): the event trigger routes (`/api/v1/event-triggers` and its dry run). Read
    /// once from the environment in `build_state`, exactly `true` turns them on. Off, the paths are absent and answer
    /// the fallback 404. A rollout control, never an authorization input: Model Gateway's own switch and Session Core
    /// decide what a caller may read or change.
    pub(crate) event_triggers_enabled: bool,
    /// `CREDENTIAL_BINDINGS_UI_ENABLED` (W4-CC-8): the org credential-binding routes (`/api/v1/credential-bindings`,
    /// POST, GET and DELETE `/{id}`) and the custom REST connector route (`/api/v1/connectors/custom-rest`). Read once
    /// from the environment in `build_state`, exactly `true` turns them on. Off, each of them answers 404
    /// `credential_bindings_disabled` and nothing is sent to Integration Core. A rollout control, never an authorization
    /// input: Integration Core admits only an owner or admin and holds the sealed secret.
    pub(crate) credential_bindings_enabled: bool,
    /// `AUTH_STEP_UP_ROUTE_ENABLED` (F-f03-15): whether `POST /api/v1/auth/step-up` exists, the gateway's relay of
    /// Auth Core's password re-check behind a high-risk approval. Read once from the environment in `build_state`,
    /// exactly `true` turns it on. Off, the route is not registered and answers 404 as before. A rollout control,
    /// never an authorization input: Auth Core re-verifies the password and records the proof for the session.
    pub(crate) auth_step_up_route_enabled: bool,
    /// "Forward an email to create a task" inbound pipeline (chat-wave2
    /// mail-task spec, owner ruling 2026-09-28: Postmark inbound,
    /// self-forward only). Empty token = the whole `/internal/mail-task/ingest`
    /// route refuses every caller — off by default until integration-corev2's
    /// matching `MAIL_TASK_GATEWAY_SERVICE_TOKEN` is provisioned alongside it.
    pub(crate) mail_task_ingest_service_token: String,
    /// Own low-ceiling limiter (mail-task runs are expensive execution-core
    /// turns, not ordinary reads) sharing the fleet limiter's Dragonfly
    /// backend but never its RPM budget. See `rate_limit.rs`'s
    /// `from_cache_with_rpm`.
    pub(crate) mail_task_rate_limiter: crate::rate_limit::RateLimiter,
    pub(crate) mail_task_nats_url: String,
    pub(crate) mail_task_nats_user: String,
    pub(crate) mail_task_nats_password: String,
    /// The actual "create the thread, post the synthesized turn, invoke
    /// execution-core" step. Deliberately `None` in every production build
    /// today -- see `domains/mail_task.rs`'s module doc for why this is a real
    /// gap, not an oversight, and what unblocks it.
    pub(crate) mail_task_pipeline:
        Option<std::sync::Arc<dyn crate::domains::mail_task::MailTaskPipeline>>,
    /// Space feature flags (P-GW1). Read once in `build_state`, never in a handler, and never an
    /// authorization input: a flag only decides whether a route or UI surface exists.
    #[allow(dead_code, reason = "first reader is the next Space slice that adds a flag")]
    pub(crate) space_features: SpaceFeatureFlags,
}

/// Space feature flags carried on `AppState`. Every Space slice that adds a flag appends ONE
/// field here (alphabetical), one line in [`SpaceFeatureFlags::from_env`], and nothing else:
/// `Default` is all-off, so test literals never name the field and `AppState::for_tests()` needs
/// no edit. Production default is OFF; development may default ON (see [`space_flag`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct SpaceFeatureFlags {
    /// `SPACE_AGENT_WAKE_POLICY_API` (WS08-S5): `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy`,
    /// the owner/manager writer of a binding's `respond_to`. Off answers 404 `feature_disabled`. A
    /// rollout control, never an authorization input: Control's live role (owner or manager) decides
    /// who may write it, and it moves no content, so Zero Data Retention organizations are unaffected.
    pub(crate) agent_wake_policy_api: bool,
    /// `SPACE_ADDRESSED_WAKE_API` (WS08-S4): the wake mode of `POST /api/v1/chat/stream` and
    /// `/api/v1/ag-ui/stream` (`wake:{message_id, client_wake_id}`: one run started from the caller's
    /// own stored post for one bound agent) and `features.addressed_wake`. Off answers 404
    /// `space_addressed_wake_disabled` to a body that carries `wake`; the legacy `mentioned_agent_ref`
    /// turn is unaffected. A rollout control, never an authorization input: Control's signed
    /// `model.run.wake` decision and Session Core's verifier decide, and a Zero Data Retention caller
    /// is refused (typed 412) before Control is called.
    pub(crate) addressed_wake_api: bool,
    /// `SPACE_MENTION_RECORDS_API` (WS08-S4): `mentions` on the human post routes (planned by
    /// `plan_mentions`, recorded by Session Core) and `features.mention_records`. Off ignores a
    /// `mentions` key (the post stands). A rollout control, never an authorization input.
    pub(crate) mention_records_api: bool,
    /// `SPACE_APPROVAL_SCOPES_ENABLED` (WS10-S4): the standing-rule half of a Space approval decision
    /// (`scope: always` on `POST /api/v1/spaces/{space_ref}/approvals/{approval_id}/decide`) and the
    /// `approval-grants` and `approval-policy` routes. Off answers 404 `feature_disabled` there and a
    /// `once` decision still works. A rollout control, never an authorization input: Control's role,
    /// entitlement (`approval_grant_entitled`) and Zero Data Retention rules decide, and a ZDR org
    /// never gets a rule whatever this says.
    pub(crate) approval_scopes: bool,
    /// `GATEWAY_SPACE_APPEND_RETRIEVAL` (WS18-S2): a follow-up turn of a scoped thread fetches its own
    /// retrieval decision and carries the token (empty on a decline, which suppresses grounding).
    /// Read once in `build_state`, and only the literal `true` turns it on: unlike the other Space flags
    /// it never defaults on in development, so every other value leaves the follow-up as it was.
    pub(crate) append_retrieval: bool,
    /// `SPACE_THREAD_SCOPE_RESOLVE` (D2-461): a chat turn on an existing thread that omits `space_ref`
    /// resolves the thread's Space from Session Core (owner-bound by-id lookup) and continues the scoped
    /// decision path, or is refused with a typed error; it never falls through to an organisation-wide
    /// search. Read once in `build_state`, and only the literal `true` turns it on, never by default in
    /// development. Off, a turn with no `space_ref` is untouched. A rollout control, never an
    /// authorization input: Control signs the decision and the Model Plane service enforces it. The
    /// lookup stores nothing, so Zero Data Retention callers are unaffected.
    pub(crate) thread_scope_resolve: bool,
    /// `GATEWAY_SPACE_MEMORY_UI` (EP-8 S7): the Space memory routes (`GET /api/v1/spaces/{space_ref}/memory`,
    /// `DELETE .../memory/{memory_id}`). Only the literal `true` turns it on, never by default in development.
    /// Off answers 404 `feature_unavailable`. A rollout control, never an authorization input: Control's
    /// `model.space.memory.manage` decision decides who may list or delete.
    pub(crate) memory_ui: bool,
    /// `AGENT_VERSION_PINNING` (EP-6 S9): a mention turn runs the binding's pinned (else published) agent
    /// version and carries its allowlist and cap to the model gateway. Only the literal `true` turns it on,
    /// never by default in development. Off keeps the legacy persona injection. A rollout control, never an
    /// authorization input: the allowlist and cap only narrow.
    pub(crate) agent_version_pinning: bool,
    /// `BROWSER_AI_RUN_ATTACH_SESSION`: whether the owner may attach an AI run to a browser session
    /// (the attach capability, the tab control and Stop agent routes, and an attached AI run start).
    /// Read once here, never in a handler. Exactly `true` enables it; any other value, and an unset
    /// variable, is off in every environment. While off the attach routes answer 409
    /// `browser_attach_unavailable` and the capabilities report it unavailable. A rollout control,
    /// never an authorization input: Quarry and model-gateway still decide each attach, and a Zero
    /// Data Retention run takes no attachment.
    pub(crate) browser_ai_run_attach_session: bool,
    /// `BROWSER_DO_ENABLED` or `BROWSER_SITES_ENABLED` (WS21-S6), each the literal `true`: the browser-site routes and
    /// the decide-time "remember" write of a `browser_site_consent` approval. Set once at startup through
    /// `browser_sites::routes_enabled`, never in a handler. A rollout control, never an authorization input:
    /// org-core decides the member's own choice, and admin policy and Quarry still decide at grant mint.
    pub(crate) browser_sites_enabled: bool,
    /// `CHAT_LOCALE_HINT` (WS16-S6): stamp the viewer's interface language (`nb-NO` or `en-US`) on
    /// the model-bound body as the request-scoped `locale` field. Off removes the field, whatever
    /// the browser sent. A rollout control, never an authorization input: the field is a framing
    /// hint, is never stored, and grants nothing.
    pub(crate) chat_locale_hint: bool,
    /// `CHAT_ARTIFACT_PROMOTE_ENABLED` (EP-3 S8): `POST /api/v1/chat/documents` accepts
    /// `origin: chat-artifact` with a `source_ref`, refuses a Zero Data Retention caller with the
    /// typed 412 itself, and the composer capabilities report `artifact_promote`. Only the literal
    /// `true` turns it on, never by default in development. Off, the route is what it was: `origin`
    /// and `source_ref` are ignored and every copy is a `chat-attachment`. A rollout control, never
    /// an authorization input: documents-api decides under the user's own scoped token.
    pub(crate) chat_artifact_promote: bool,
    /// `GATEWAY_CHAT_PROJECTS_ENABLED` (WS18-S4, owner decision D-f06-C): a project start, a turn that names a Space
    /// for a thread that does not exist yet, is refused with a typed 404 `chat_projects_disabled` before any upstream
    /// call, and the composer capabilities report `chat_projects` only while it is on. Only the literal `true` turns
    /// it on, never by default in development. Off, a turn with no thread is what it was. A rollout control, never an
    /// authorization input: the Space role and the Model Plane decide each turn. The first release is append-only;
    /// live following, approvals and plan hydration for project threads are not part of it.
    pub(crate) chat_projects: bool,
    /// `CHAT_RESEARCH_RUNS` (Quarry v2 E4.6, Q4-062b): `GET /api/v1/chat/research-runs/{id}` and
    /// `POST /api/v1/chat/research-runs/{id}/cancel`, the person's read and stop of one addressable
    /// research run. Only the literal `true` turns it on; default off, never by default in development.
    /// Off, the routes are not registered (404). A rollout control, never an authorization input:
    /// Model Gateway and Session Core decide that the run is the caller's own, and a zero-retention
    /// run comes back with ids, status and counters only.
    pub(crate) chat_research_runs: bool,
    /// `WEB_DOMAIN_POLICY_BFF` (EP-9, O-23, D-f10-3): the org's web domain rules also bind this gateway's web search
    /// (web, similar, images, videos and the answer stream) and the crawl action. Read once here. Only the literal
    /// `true` turns it on, in every environment. Off, no handler calls Model Gateway for a domain check and every
    /// search and crawl is what it was. On, the verdicts bind only when Model Gateway's own `WEB_DOMAIN_POLICY_MODE`
    /// is `enforce` (under `shadow` the would-be refusals are logged, under `off` nothing is read). A rollout
    /// control, never an authorization input: Model Gateway asks org-core under the caller's verified organisation.
    pub(crate) web_domain_policy_bff: bool,
    /// `ORG_COMPLIANCE_ROLE_ENABLED` (O-21): the compliance organisation role is live. Read once here, never in a
    /// handler. Only the literal `true` turns it on, in every environment. On, an organisation admin joins the owner
    /// and the compliance member in placing and releasing legal holds, and a compliance member reads the audit and
    /// compliance reports. Off, the hold write list is the owner and compliance only and the audit routes stay owner
    /// and admin only, byte for byte. A rollout control, never an authorization input: org-core and audit-core read
    /// the same switch and decide again from the caller's own membership.
    pub(crate) org_compliance_role: bool,
    /// `VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE` (WS14.S7): whether the UI crawl submit forwards
    /// `ignore_robots_txt: true` to the edge. Read once here, never in a handler. Exactly `true`
    /// enables it; any other value, and an unset variable, is off in every environment. A rollout
    /// control, never an authorization input: the edge refuses the override without the org
    /// entitlement and for service principals, and quarry-control audits it before the ack.
    pub(crate) crawl_robots_override: bool,
    /// `CHAT_MEDIA_TRANSCRIPTS_ENABLED` (W5, F-t3-090): `POST /api/v1/chat/transcripts`, which transcribes one audio
    /// upload through Model Gateway's speech route and saves the text as a private `media_transcript` document. Only
    /// the literal `true` turns it on, never by default in development. Off, the route answers 404
    /// `media_transcripts_disabled`. A rollout control, never an authorization input: the speech route, documents-api
    /// and the caller's own token decide what is read and written, and a Zero Data Retention caller is refused with
    /// the typed 412 whatever this says.
    pub(crate) chat_media_transcripts: bool,
    /// `SPACE_DENIALS_TYPED` (WS03-S5): map Control's typed Space denials to the browser envelope.
    /// Off restores pass-through. A rollout control, never an authorization input.
    pub(crate) denials_typed: bool,
    /// `SPACE_DM_API` (WS12-S3): `POST /api/v1/spaces/dm`, `POST`/`DELETE /api/v1/spaces/{space_ref}/hide`,
    /// the DM fields of `GET /api/v1/spaces` and `features.dm`. Off answers 404 `dm_disabled`, hides DM
    /// kinds from the listing and the SPA's DM group. A rollout control, never an authorization input:
    /// Control's `dm-eligibility` and the fixed roster decide, and a Zero Data Retention org is refused
    /// with the typed 412 whatever this says. Production default off; DMs also need WS04-S5 audience registration.
    pub(crate) dm_api: bool,
    /// `SPACE_MESSAGE_REVISE_API` (WS12-S7): `PATCH`/`DELETE /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}`,
    /// `features.message_revise` and `data.message_policy` of the Space context. Off answers 404
    /// `space_message_revise_disabled`. A rollout control, never an authorization input: Control's
    /// `model.thread.revise` decision and Session Core's author/owner/moderator check decide, and a Zero
    /// Data Retention org is refused with the typed 412 whatever this says. Production default off.
    pub(crate) message_revise_api: bool,
    /// `SPACE_REACTIONS_API` (WS12-S8): `PUT`/`DELETE .../threads/{thread_id}/messages/{message_id}/reactions/{emoji}`,
    /// the `reactions[]` merged into transcript turns and `features.reactions`. Off answers 404
    /// `space_reactions_disabled`. A rollout control, never an authorization input: the caller's live
    /// room role (editor or above), the Control shared-read decision and Session Core's message check
    /// decide, and a Zero Data Retention org is refused with the typed 412 before any write. Needs
    /// Convex `SPACE_REACTIONS` on and model-gateway `SPACE_MESSAGE_REVISE_API` (target route). Off by
    /// default in every environment, development included; only the literal `true` turns it on.
    pub(crate) reactions_api: bool,
    /// `CLIENT_VERSION_POLICY` / `CLIENT_MIN_CONTRACT` (WS07-S3): the stale-client contract the
    /// public `/api/v1/client-config` route reports. Unlike the booleans its default is `banner`,
    /// never off. Carried here (not as an `AppState` field) so no test literal needs an edit.
    pub(crate) client_policy: crate::client_contract::ClientPolicy,
    /// `CSP_REPORT_LOG_HOSTS` (WS07-S6): log the blocked ORIGIN (scheme and host, never a path
    /// or query) of each CSP report, for a host inventory. Forced off when `APP_ENV` is
    /// production (see `dev_only_documented_flag`); the counters never carry a URL either way. Its
    /// switch reads `1`, `true`, `yes` or `on`, the words the flag registry documents.
    pub(crate) csp_report_log_hosts: bool,
    /// `CLIENT_ENV_IDENTITY` (F-t3-098): the closed environment label `GET /api/v1/client-config` reports
    /// as `environment` (`production`, `staging`, `development` or `other`), derived once from `APP_ENV`
    /// by `client_environment_label`. `None` while the switch is off (the default), and the key is then
    /// left out. The raw `APP_ENV` value is never stored. A display identity only, never an
    /// authorization input.
    pub(crate) client_environment: Option<&'static str>,
    /// `SPACE_CONNECTIVITY_BANNER_UI` (WS01-S5): reported to the SPA as `features.connectivity_banner`
    /// (the offline versus service-down banner). A rollout control, never an authorization input.
    pub(crate) connectivity_banner_ui: bool,
    /// `SPACE_HUMAN_POSTS_API` (WS01-S5): the two human-post routes (`POST .../threads` and
    /// `POST .../threads/{thread_id}/messages`) and `features.human_posts`. Off answers 404
    /// `space_human_posts_disabled`. A rollout control, never an authorization input: Control's
    /// decision and Session Core's member gate decide who may post, and a Zero Data Retention org
    /// is refused whatever this says.
    pub(crate) human_posts_api: bool,
    /// `SPACE_MEMBERSHIP_WRITER_API` (WS04-S6): room member add, role change and leave go to
    /// Control's actor-carrying writer, then the audience reconcile. Off keeps the legacy
    /// Convex-then-declare path (emergency stop only: it cannot remove a member added through
    /// Control). A rollout control, never an authorization input; Control decides every role.
    pub(crate) membership_writer_api: bool,
    /// `SPACE_POLICY_ADMIN_API` (WS03-S4): the three org-admin Space effect-policy routes and
    /// the console that reads them. Off answers 404 `space_policy_admin_disabled`. A rollout
    /// control, never an authorization input; Control's role check and write switch decide.
    pub(crate) policy_admin_api: bool,
    /// `SPACE_REALTIME_STREAM` (WS02-S4): `GET /api/v1/spaces/{space_ref}/events`, the validating
    /// relay of the Space event stream. Off answers 404 `space_stream_disabled` and the SPA stays
    /// on its 6 s poll. A rollout control, never an authorization input; an org under Zero Data
    /// Retention gets `poll_only` even when on.
    pub(crate) realtime_stream: bool,
    /// `SPACE_ROOM_DETAILS_UI` (WS04-S2): `PATCH /api/v1/spaces/{space_ref}` (room name and topic),
    /// the `spaces.update_room_details` action and `features.room_details`. Off answers 404
    /// `feature_disabled`. A rollout control, never an authorization input: Control's live role
    /// (owner or manager) decides who may rename. The topic is a label, so Zero Data Retention
    /// organizations are unaffected.
    pub(crate) room_details_ui: bool,
    /// `SPACE_PEOPLE_SEARCH_API` (WS04-S3): `GET /api/v1/directory/people`, the people search the
    /// room member picker uses. Off answers 404 `feature_disabled`. A rollout control, never an
    /// authorization input: Control's directory read (signed delegation, scope
    /// `directory:read:self`) decides what the caller may see.
    pub(crate) people_search_api: bool,
    /// `SPACE_DIRECTORY_RESOLVER_API` (WS04-S9): `POST /api/v1/directory/resolve` and the composed
    /// `kinds=person,agent` form of `GET /api/v1/directory/people`, plus
    /// `features.directory_resolver`. Off answers 404 `feature_disabled`. A rollout control, never
    /// an authorization input: the answer is presence only (`meta.presence_only`), and Control's
    /// directory read decides which people the caller may see.
    pub(crate) directory_resolver_api: bool,
    /// `SPACE_ROOM_ARCHIVE_API` (WS04-S7): `POST /api/v1/spaces/{space_ref}/archive` and
    /// `/unarchive` (Control's archive state) and `features.room_archive`. Off answers 404
    /// `feature_disabled`; an archived room still reads and still refuses writes, because that is
    /// Control's state, not this flag. A rollout control, never an authorization input: Control's
    /// live role (owner or manager) decides who may archive. No content moves, so Zero Data
    /// Retention organizations are unaffected.
    pub(crate) room_archive_api: bool,
    /// `SPACE_ROOM_DIRECTORY_API` (WS04-S8): `GET /api/v1/directory/rooms`, `POST
    /// /api/v1/spaces/{space_ref}/join`, `PATCH /api/v1/spaces/{space_ref}/settings`, the actions
    /// `spaces.join_room` and `spaces.update_room_settings`, and `features.room_directory`. Off
    /// answers 404 `feature_disabled`. A rollout control, never an authorization input: Control's
    /// per-organization directory policy (`space_directory_policies`, absent means off), the
    /// room's visibility and the caller's live role decide who may list, join or change a room.
    /// No content moves, so Zero Data Retention organizations are unaffected.
    pub(crate) room_directory_api: bool,
    /// `SPACE_ROOM_ARTIFACTS_API` (WS07-S9): `GET /api/v1/spaces/{space_ref}/artifacts`, the room
    /// artifact aggregate, and `features.room_artifacts`. Off answers 404
    /// `space_room_artifacts_disabled` and the SPA hides the chips and the tab. A rollout control,
    /// never an authorization input: Control's shared-read decision and Session Core's audience
    /// ceiling decide what the caller may list.
    pub(crate) room_artifacts: bool,
    /// `SPACE_ROOM_PINS_API` (WS13-S8): `PUT`/`DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}`,
    /// the `pins` section of the thread listing and `features.room_pins`. Off answers 404
    /// `feature_disabled`, the listing omits `pins` and the SPA stops lifting. A rollout control,
    /// never an authorization input: Control's live role (editor or above) decides who may pin.
    pub(crate) room_pins: bool,
    /// `SPACE_ROOM_FILES_API` (WS14-S7): the durable room-file routes `POST`/`GET
    /// /api/v1/spaces/{space_ref}/files`, `.../files/uploads/{upload_id}`, `.../files/{document_id}/text`,
    /// `DELETE .../files/{document_id}`, and `features.room_files.durable`. Off answers 404
    /// `room_files_disabled`. A rollout control, never an authorization input: Control's import and
    /// retrieval decisions and the Data and Ingestion planes decide, and a Zero Data Retention
    /// organization is refused the upload with the typed 412 whatever this says.
    pub(crate) room_files_api: bool,
    /// `SPACE_ROOM_FILES_UI` (WS14-S7): reported as `features.room_files.files_tab` (together with
    /// `room_files_api`) so the SPA shows the Files tab. Never an authorization input.
    pub(crate) room_files_ui: bool,
    /// `RUN_TRACE_API` (WS10.S6, GW-2): `GET /api/v1/runs/{run_id}/trace`, one page of a run's durable
    /// browser trace. Only the literal `true` turns it on; default off in every environment, never by
    /// default in development. Off, the route is not registered (404). A rollout control, never an
    /// authorization input: Model Gateway and Session Core decide that the run is the caller's own, and
    /// a zero-retention run has no rows, so its page is empty.
    pub(crate) run_trace_api: bool,
    /// `SPACE_TURN_ATTACHMENTS_UI` (WS14-S7): reported as `features.room_files.turn_attachments` so the
    /// composer offers per-turn attachments. Never an authorization input.
    pub(crate) turn_attachments_ui: bool,
    /// `SPACE_MESSAGE_SEARCH_API` (WS13-S3): `POST /api/v1/spaces/{space_ref}/search` and
    /// `GET /api/v1/spaces/{space_ref}/entries/{message_id}` (permission-checked message search and
    /// message addresses, relayed to model-gateway). Off answers 404 `space_search_disabled`.
    /// Reported as `features.message_search` together with `message_search_ui`. A rollout control,
    /// never an authorization input: Control's per-request read decision and Session Core's
    /// audience predicates decide what a caller finds.
    pub(crate) message_search_api: bool,
    /// `SPACE_MESSAGE_SEARCH_UI` (WS13-S3 reports it, S5 reads it): shows the in-room search control.
    /// A rollout control, never an authorization input.
    pub(crate) message_search_ui: bool,
    /// `SPACE_ENTRY_LINKS_UI` (WS13-S3 reports it, S4a/S4b read it): `features.entry_links`, the
    /// message permalink grammar and `/spaces/:spaceId/m/:messageId`. A rollout control only.
    pub(crate) entry_links_ui: bool,
    /// `SPACE_COMMAND_PALETTE_UI` (WS13-S3 reports it, S6/S7 read it): `features.command_palette`,
    /// the Cmd/Ctrl+K palette and platform slash commands. Off keeps the old search dialog. A
    /// rollout control only.
    pub(crate) command_palette_ui: bool,
    /// `SPACE_THREAD_FORK_API` (WS13-S3 reports it; the fork slice S10 is conditional and not built):
    /// `features.thread_fork`. A rollout control only.
    pub(crate) thread_fork_api: bool,
    /// `SPACE_SEND_IDEMPOTENCY_API` (WS01-S5): reported to the SPA as `features.send_idempotency`
    /// (the SPA attaches a client send id to an agent-path send so a retry cannot double-post).
    /// Session Core's dedupe is inert without a key. A rollout control, never an authorization input.
    pub(crate) send_idempotency_api: bool,
    /// `SPACE_THREAD_PANE_UI` (WS01-S5): reported to the SPA as `features.thread_pane` (the right-hand
    /// thread pane and `?thread_id=` deep link). A rollout control, never an authorization input.
    pub(crate) thread_pane_ui: bool,
    /// `SPACE_TOOL_POLICY_EDITOR_ENABLED` (WS10-S5b): the per-tool policy editor routes
    /// (`GET|PUT /api/v1/spaces/{space_ref}/tool-policy`, `PUT .../agents/{binding_ref}/tool-policy`)
    /// and the Agent tab editor, reported to the SPA as `features.tool_policy_editor`. Off answers
    /// 404 `feature_disabled` and hides the editor; enforcement is governed by Execution Core's own
    /// gate, not by this flag. A rollout control, never an authorization input: Control's live
    /// manager or owner role decides who may write, and a policy only tightens.
    pub(crate) tool_policy_editor: bool,
    /// `SPACE_ATTENTION_API` (WS11-S7): the attention inbox routes (`GET /api/v1/spaces/attention/items`,
    /// `POST .../items/{item_id}/state`, `GET|PUT /api/v1/spaces/{space_ref}/threads/{thread_id}/follow`).
    /// Off answers 404 `space_attention_disabled`. A rollout control, never an authorization input: the
    /// caller's own Application rows, Control's index and per-Space read decisions decide what is shown.
    pub(crate) attention_api: bool,
    /// `SPACE_ATTENTION_UI` (WS11-S7): reported as `features.attention` so the SPA shows the `/attention`
    /// inbox and the thread follow control. A rollout control only.
    pub(crate) attention_ui: bool,
    /// `SPACE_STATUS_API` (WS11-S10, `space_status_api`): the org-scoped status routes (`GET|PUT
    /// /api/v1/spaces/status`) and the `status` / `activity` annotations on a room's roster and agent
    /// list. Off answers 404 `space_status_disabled` on the status routes and leaves the roster and
    /// agent responses unannotated. A rollout control, never an authorization input.
    pub(crate) status_api: bool,
    /// `SPACE_REMINDERS_API` (WS11-S11, `space_reminders_api`): the reminder routes (`POST
    /// /api/v1/spaces/{space_ref}/reminders`, `GET /api/v1/spaces/reminders`, `DELETE
    /// /api/v1/spaces/reminders/{id}`) and the `spaces.create_reminder` / `spaces.cancel_reminder`
    /// actions. Off answers 404 `space_reminders_disabled`. A rollout control, never an
    /// authorization input: membership, the Zero Data Retention posture and the caller's own rows decide.
    pub(crate) reminders_api: bool,
    /// `ORG_IDENTITY_ADMIN_API` (WS05-S6): the owner/admin domain verification routes
    /// (`/api/v1/orgs/{id}/domains`), proxied to org-core. Off answers 404 `feature_disabled`.
    /// A rollout control, never an authorization input: the live owner/admin membership decides.
    pub(crate) org_identity_admin_api: bool,
    /// `SPACE_RUN_LIVE_TEXT` (WS09-S2 reserves the flag, S4 reads it): stream the answer text to the
    /// room while a run works. Off means step classes and `text_progress` only. A rollout control,
    /// never an authorization input.
    #[allow(dead_code, reason = "first reader is WS09-S4, which streams the room live view")]
    pub(crate) run_live_text: bool,
    /// `SPACE_RUN_LIVE_VIEW` (WS09-S2 reserves the flag, S4/S5 read it): the room live view of an
    /// agent run, reported as `data.agent_work.live`. A rollout control, never an authorization input.
    #[allow(dead_code, reason = "first reader is WS09-S4, which serves the room live view")]
    pub(crate) run_live_view: bool,
    /// `SPACE_RUN_MANAGER_STOP` (WS09-S2 reserves the flag, S8 reads it): a manager's Stop of a run,
    /// reported as `data.agent_work.stop_manager`. A rollout control, never an authorization input:
    /// Control's run-control decision decides who may stop.
    #[allow(dead_code, reason = "first reader is WS09-S8, which serves the manager Stop")]
    pub(crate) run_manager_stop: bool,
    /// `SPACE_RUN_OWNER_STEER` (WS09-S2): the person who started a run may type into it while it
    /// works, reported as `data.agent_work.steer_owner`. Off keeps the composer disabled during a
    /// turn. A rollout control, never an authorization input: the queue route re-authorizes each
    /// message, and a Zero Data Retention org is reported false because that route refuses there.
    pub(crate) run_owner_steer: bool,
    /// `SPACE_SIDEBAR_API` (WS16-S8): `GET` and `PUT /api/v1/spaces/sidebar` and the
    /// `spaces.update_sidebar` action, the member's personal sidebar layout (refs only, stored in
    /// Application). Off answers 404 `space_sidebar_disabled`, which the SPA reads as "use the
    /// legacy panel". A rollout control, never an authorization input: a Zero Data Retention org
    /// is answered `zdr_local` on `GET` and refused on `PUT` whatever this says.
    pub(crate) sidebar: bool,
    /// `SPACE_COMPOSER_DRAFTS_UI` (WS15-S7, reported as `features.composer_drafts`): the room
    /// composer keeps per-room, per-thread drafts. A rollout control, never an authorization input.
    pub(crate) composer_drafts_ui: bool,
    /// `SPACE_COMPOSER_MARKDOWN_UI` (WS15-S7, `features.composer_markdown`): the markdown toolbar,
    /// preview and counter in the room composer. Never an authorization input.
    pub(crate) composer_markdown_ui: bool,
    /// `SPACE_HUMAN_MARKDOWN_UI` (WS15-S7, `features.human_markdown`): people's posts render as
    /// markdown. Never an authorization input.
    pub(crate) human_markdown_ui: bool,
    /// `SPACE_TIMELINE_ANCHOR_UI` (WS15-S7, `features.timeline_anchor`): the anchored single-root
    /// room timeline. Never an authorization input.
    pub(crate) timeline_anchor_ui: bool,
    /// `SPACE_TIMELINE_API` (WS15-S7): `GET /api/v1/spaces/{space_ref}/timeline` and the tail window
    /// parameters of the thread transcript route, reported as `features.timeline_windows`. Off
    /// answers 404 `space_timeline_disabled` (the SPA then reads the legacy listing) and leaves the
    /// transcript whole. A rollout control, never an authorization input: Control's membership and
    /// per-page read decision and Session Core's audience ceiling decide what a window holds.
    pub(crate) timeline_api: bool,
    /// `SPACE_SYSTEM_ENTRIES_API` (WS15-S11): the room's system rows (membership changes read from
    /// Control's ledger) merged into the timeline answer, reported as `features.system_entries`.
    /// Needs `SPACE_TIMELINE_API`. Off leaves `system_entries` empty. A rollout control, never an
    /// authorization input: Control's feed route requires a live membership, and the rows hold ids
    /// and closed words only.
    pub(crate) system_entries_api: bool,
    /// `SCIM_EDGE_ENABLED` (WS05-S8): the public, session-less SCIM edge `/api/v1/scim/v2/*`
    /// that forwards an identity provider's bearer token to org-core. Off (the default, in
    /// development too) answers the same 501 SCIM error as before the feature existed. A rollout
    /// control, never an authorization input: org-core resolves the token and the organization. Its
    /// switch reads only the exact literal `true` (R-bff-02), read once by `SpaceFeatureFlags::from_env`.
    pub(crate) scim_edge: ScimEdgeConfig,
    /// `ASSISTANT_NAVIGATION_ENABLED`: the assistant navigation model call (`POST /api/v1/ai/assistant-navigation`).
    /// Only the literal `true` turns it on, in every environment. Off, the route answers 503
    /// `assistant_navigation_unavailable` and no model call is made. Read once here, never in a handler.
    pub(crate) assistant_navigation_enabled: bool,
    /// `BROWSER_AI_RUN_REAL_GRANT` (WS21-S7): a workspace AI run starts under a real broker grant. Only the
    /// literal `true` turns it on, and a zero-retention session never takes one. Read once here, never in a handler.
    pub(crate) browser_ai_run_real_grant: bool,
    /// `BROWSER_ASSISTANT_TASK_GRANTS`: an assistant task session asks for its own task-scoped grant. Only the
    /// literal `true` turns it on. Read once here, never in a handler.
    pub(crate) browser_assistant_task_grants: bool,
    /// `BROWSER_DO_ENABLED` (WS21-S2): the agent browser's Do tools. Only the literal `true` turns it on. Read once
    /// here, never in a handler; `browser_sites_enabled` reads the same value.
    pub(crate) browser_do_enabled: bool,
    /// `SANDBOX_THREAD_WORKSPACE_ENABLED` (WS22-S3): a thread's workspace is used and erased with the thread. Only
    /// the literal `true` turns it on. Read once here, never in a handler.
    pub(crate) sandbox_thread_workspace_enabled: bool,
    /// `SKILL_MEMBER_AUTHORING` (EP-11 S1): a member may author private skills. Only the literal `true` turns it
    /// on. Read once here, never in a handler.
    pub(crate) skill_member_authoring: bool,
    /// `SCHEDULE_CONTRACT_ENABLED` (DEC-16): a schedule create or renewal may carry a stored-consent contract to
    /// Control. Only the literal `true` turns it on. Read once here, never in a handler.
    pub(crate) schedule_contract_enabled: bool,
}

/// The SCIM edge switch and its org-core credential (principal `verevon-scim-edge`). The edge is
/// armed only when the flag is on AND the credential is a secure secret, so a half-configured
/// deployment keeps answering 501 instead of forwarding anonymously.
#[derive(Clone, Default)]
pub(crate) struct ScimEdgeConfig {
    pub(crate) enabled: bool,
    pub(crate) token: String,
}

impl std::fmt::Debug for ScimEdgeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScimEdgeConfig")
            .field("enabled", &self.enabled)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl ScimEdgeConfig {
    /// Read once, by `SpaceFeatureFlags::from_env` in `build_state`. `SCIM_EDGE_ENABLED` is on only for the
    /// exact literal `true` (`1`, `yes` and `on` leave it off, and the startup report in `switch_spellings`
    /// names them), and the edge also needs a secure credential.
    pub(crate) fn from_env() -> Self {
        let flag = env_exact_true("SCIM_EDGE_ENABLED");
        let token = env::var("ORG_CORE_SCIM_EDGE_TOKEN").unwrap_or_default();
        let token = token.trim();
        let secure = service_token_is_secure(token);
        warn_scim_edge_misconfiguration(&mut std::io::stderr().lock(), flag, secure);
        Self {
            enabled: flag && secure,
            token: if secure { token.to_owned() } else { String::new() },
        }
    }
}

/// The startup refusal for a SCIM edge that is switched on without a secure credential. It goes to stderr,
/// as the other startup refusals do, so it shows whatever `RUST_LOG` is. Nothing is written unless the switch
/// is on and the credential is not a secure secret.
fn warn_scim_edge_misconfiguration(out: &mut impl std::io::Write, flag: bool, secure: bool) {
    if flag && !secure {
        // A failed write to stderr must not stop the gateway from starting, so the error is dropped.
        let _ = writeln!(
            out,
            "SCIM_EDGE_ENABLED is on but ORG_CORE_SCIM_EDGE_TOKEN is not a secure secret; the SCIM edge stays off"
        );
    }
}

impl SpaceFeatureFlags {
    pub(crate) fn from_env() -> Self {
        // `BROWSER_DO_ENABLED` is read here once. It feeds the agent browser's Do switch and the browser-site
        // routes (with `BROWSER_SITES_ENABLED`), so the two readings can never disagree.
        let browser_do_enabled = env_exact_true("BROWSER_DO_ENABLED");
        Self {
            agent_wake_policy_api: space_flag("SPACE_AGENT_WAKE_POLICY_API"),
            addressed_wake_api: space_flag("SPACE_ADDRESSED_WAKE_API"),
            mention_records_api: space_flag("SPACE_MENTION_RECORDS_API"),
            approval_scopes: space_flag("SPACE_APPROVAL_SCOPES_ENABLED"),
            append_retrieval: env::var("GATEWAY_SPACE_APPEND_RETRIEVAL").is_ok_and(|value| value == "true"),
            thread_scope_resolve: env::var("SPACE_THREAD_SCOPE_RESOLVE").is_ok_and(|value| value == "true"),
            memory_ui: env::var("GATEWAY_SPACE_MEMORY_UI").is_ok_and(|value| value == "true"),
            agent_version_pinning: env::var("AGENT_VERSION_PINNING").is_ok_and(|value| value == "true"),
            browser_ai_run_attach_session: env::var("BROWSER_AI_RUN_ATTACH_SESSION")
                .is_ok_and(|value| value == "true"),
            browser_sites_enabled: crate::domains::browser_sites::routes_enabled(
                browser_do_enabled,
                env_exact_true("BROWSER_SITES_ENABLED"),
            ),
            chat_locale_hint: space_flag("CHAT_LOCALE_HINT"),
            chat_artifact_promote: chat_artifact_promote_from(
                env::var("CHAT_ARTIFACT_PROMOTE_ENABLED").ok().as_deref(),
            ),
            chat_projects: chat_projects_from(
                env::var("GATEWAY_CHAT_PROJECTS_ENABLED").ok().as_deref(),
            ),
            chat_research_runs: env_exact_true("CHAT_RESEARCH_RUNS"),
            web_domain_policy_bff: env_exact_true("WEB_DOMAIN_POLICY_BFF"),
            org_compliance_role: env_exact_true("ORG_COMPLIANCE_ROLE_ENABLED"),
            crawl_robots_override: crate::domains::knowledge::quarry::crawl_robots_override_from(
                env::var(crate::domains::knowledge::quarry::CRAWL_ROBOTS_OVERRIDE_FLAG)
                    .ok()
                    .as_deref(),
            ),
            chat_media_transcripts: chat_media_transcripts_from(
                env::var("CHAT_MEDIA_TRANSCRIPTS_ENABLED").ok().as_deref(),
            ),
            denials_typed: space_flag("SPACE_DENIALS_TYPED"),
            dm_api: space_flag("SPACE_DM_API"),
            message_revise_api: space_flag("SPACE_MESSAGE_REVISE_API"),
            reactions_api: space_flag("SPACE_REACTIONS_API"),
            client_policy: crate::client_contract::ClientPolicy::from_env(),
            csp_report_log_hosts: dev_only_documented_flag("CSP_REPORT_LOG_HOSTS"),
            client_environment: client_environment_identity(),
            connectivity_banner_ui: space_flag("SPACE_CONNECTIVITY_BANNER_UI"),
            human_posts_api: space_flag("SPACE_HUMAN_POSTS_API"),
            membership_writer_api: space_flag("SPACE_MEMBERSHIP_WRITER_API"),
            policy_admin_api: space_flag("SPACE_POLICY_ADMIN_API"),
            realtime_stream: space_flag("SPACE_REALTIME_STREAM"),
            room_details_ui: space_flag("SPACE_ROOM_DETAILS_UI"),
            people_search_api: space_flag("SPACE_PEOPLE_SEARCH_API"),
            directory_resolver_api: space_flag("SPACE_DIRECTORY_RESOLVER_API"),
            room_archive_api: space_flag("SPACE_ROOM_ARCHIVE_API"),
            room_directory_api: space_flag("SPACE_ROOM_DIRECTORY_API"),
            room_artifacts: space_flag("SPACE_ROOM_ARTIFACTS_API"),
            room_pins: space_flag("SPACE_ROOM_PINS_API"),
            room_files_api: space_flag("SPACE_ROOM_FILES_API"),
            room_files_ui: space_flag("SPACE_ROOM_FILES_UI"),
            run_trace_api: env_exact_true("RUN_TRACE_API"),
            turn_attachments_ui: space_flag("SPACE_TURN_ATTACHMENTS_UI"),
            message_search_api: space_flag("SPACE_MESSAGE_SEARCH_API"),
            message_search_ui: space_flag("SPACE_MESSAGE_SEARCH_UI"),
            entry_links_ui: space_flag("SPACE_ENTRY_LINKS_UI"),
            command_palette_ui: space_flag("SPACE_COMMAND_PALETTE_UI"),
            thread_fork_api: space_flag("SPACE_THREAD_FORK_API"),
            send_idempotency_api: space_flag("SPACE_SEND_IDEMPOTENCY_API"),
            thread_pane_ui: space_flag("SPACE_THREAD_PANE_UI"),
            tool_policy_editor: space_flag("SPACE_TOOL_POLICY_EDITOR_ENABLED"),
            attention_api: space_flag("SPACE_ATTENTION_API"),
            attention_ui: space_flag("SPACE_ATTENTION_UI"),
            status_api: space_flag("SPACE_STATUS_API"),
            reminders_api: space_flag("SPACE_REMINDERS_API"),
            org_identity_admin_api: space_flag("ORG_IDENTITY_ADMIN_API"),
            run_live_text: space_flag("SPACE_RUN_LIVE_TEXT"),
            run_live_view: space_flag("SPACE_RUN_LIVE_VIEW"),
            run_manager_stop: space_flag("SPACE_RUN_MANAGER_STOP"),
            run_owner_steer: space_flag("SPACE_RUN_OWNER_STEER"),
            sidebar: space_flag("SPACE_SIDEBAR_API"),
            composer_drafts_ui: space_flag("SPACE_COMPOSER_DRAFTS_UI"),
            composer_markdown_ui: space_flag("SPACE_COMPOSER_MARKDOWN_UI"),
            human_markdown_ui: space_flag("SPACE_HUMAN_MARKDOWN_UI"),
            timeline_anchor_ui: space_flag("SPACE_TIMELINE_ANCHOR_UI"),
            timeline_api: space_flag("SPACE_TIMELINE_API"),
            system_entries_api: space_flag("SPACE_SYSTEM_ENTRIES_API"),
            scim_edge: ScimEdgeConfig::from_env(),
            assistant_navigation_enabled: env_exact_true("ASSISTANT_NAVIGATION_ENABLED"),
            browser_ai_run_real_grant: env_exact_true("BROWSER_AI_RUN_REAL_GRANT"),
            browser_assistant_task_grants: env_exact_true("BROWSER_ASSISTANT_TASK_GRANTS"),
            browser_do_enabled,
            sandbox_thread_workspace_enabled: env_exact_true("SANDBOX_THREAD_WORKSPACE_ENABLED"),
            skill_member_authoring: env_exact_true("SKILL_MEMBER_AUTHORING"),
            schedule_contract_enabled: env_exact_true("SCHEDULE_CONTRACT_ENABLED"),
        }
    }
}

/// Space switches that Model Plane also reads, by the same name. Model Plane turns each one on only
/// for the exact literal `true` (`switch_from` in mp-contracts), so the gateway reads these the same
/// way. Read leniently here, a `1`, `yes` or `on` would show a route in the UI that Model Plane
/// refuses. Model Plane's base compose defaults them off (only its dev override turns them on), so an
/// unset value is off here in every environment too.
pub(crate) const MODEL_PLANE_EXACT_TRUE_SWITCHES: [&str; 7] = [
    "SPACE_HUMAN_POSTS_API",
    "SPACE_MESSAGE_SEARCH_API",
    "SPACE_MESSAGE_REVISE_API",
    "SPACE_TIMELINE_API",
    "SPACE_RUN_LIVE_VIEW",
    "SPACE_RUN_LIVE_TEXT",
    "SPACE_RUN_MANAGER_STOP",
];

/// The BFF-only Space rollout controls that default off in every environment (the base compose
/// declares `${KEY:-false}`). They read the exact literal `true`, as the Model Plane mirrors do, with
/// no development default: a development stack without an override shows none of these routes, and
/// a stray `1` or `yes` cannot turn one on (V3d-bff-3).
pub(crate) const BFF_EXACT_TRUE_SPACE_SWITCHES: [&str; 6] = [
    "SPACE_REACTIONS_API",
    "SPACE_THREAD_FORK_API",
    "SPACE_ROOM_FILES_API",
    "SPACE_ROOM_FILES_UI",
    "SPACE_TURN_ATTACHMENTS_UI",
    "SPACE_SYSTEM_ENTRIES_API",
];

/// The default-off browser switches that read only the exact literal `true`, each through
/// [`exact_true_flag`] or [`env_exact_true`] in `build_state` (V3e-bff-3). The startup report in
/// `switch_spellings` lists them, so a former spelling such as `1`, `yes` or `TRUE` is logged. The last three
/// were read per request before R-bff-01 and are read once now, by `SpaceFeatureFlags::from_env`.
pub(crate) const BROWSER_EXACT_TRUE_SWITCHES: [&str; 12] = [
    "BROWSER_HUMAN_BROWSE",
    "BROWSER_RECIPES",
    "BROWSER_CONSENT_API",
    "BROWSER_DOWNLOADS",
    "BROWSER_TURN_CONTEXT",
    "BROWSER_LIVE_VIEW_ONLY",
    "BROWSER_REGION_ATTACH",
    "BROWSER_TAB_MENTIONS",
    "BROWSER_POLICY_ADMIN_API",
    "BROWSER_DO_ENABLED",
    "BROWSER_AI_RUN_REAL_GRANT",
    "BROWSER_ASSISTANT_TASK_GRANTS",
];

/// The BFF switches outside the Space and browser families that read only the exact literal `true`. Each is read
/// once, by `SpaceFeatureFlags::from_env` (or `ScimEdgeConfig::from_env` for `SCIM_EDGE_ENABLED`), in `build_state`,
/// and never in a handler. The startup report in `switch_spellings` lists them too.
pub(crate) const BFF_EXACT_TRUE_SWITCHES: [&str; 5] = [
    "ASSISTANT_NAVIGATION_ENABLED",
    "SCIM_EDGE_ENABLED",
    "SCHEDULE_CONTRACT_ENABLED",
    "SANDBOX_THREAD_WORKSPACE_ENABLED",
    "SKILL_MEMBER_AUTHORING",
];

/// Whether a Space switch reads only the exact literal `true`: a name in either list above. Every other
/// Space flag keeps the lenient reading (see [`space_flag`]). Each of those is a default-on kill switch
/// in the base compose, which `main.rs` pins, so the lenient words only ever read a switch on by default.
pub(crate) fn is_exact_true_space_switch(key: &str) -> bool {
    MODEL_PLANE_EXACT_TRUE_SWITCHES.contains(&key) || BFF_EXACT_TRUE_SPACE_SWITCHES.contains(&key)
}

/// One Space flag. A name for which [`is_exact_true_space_switch`] holds is on only for the exact
/// literal `true`: not trimmed, not case-folded. An UNSET variable is OFF for those names in every
/// environment, with no development default. Every other Space flag is on for `1|true|yes|on`, and an
/// UNSET variable falls back to ON in development and OFF when `APP_ENV` is production, the same dev
/// default as every other dev-defaulted gateway flag.
#[allow(dead_code, reason = "first consumer is the next Space slice that adds a flag")]
pub(crate) fn space_flag(key: &str) -> bool {
    if is_exact_true_space_switch(key) {
        return env_exact_true(key);
    }
    let fallback = dev_flags_allowed(&env::var("APP_ENV").unwrap_or_default());
    env_bool(key, fallback)
}

#[cfg(test)]
impl AppState {
    /// A complete, offline `AppState` for tests. Prefer this plus struct-update syntax over a
    /// fresh literal so a new field never has to be added to every test.
    #[allow(dead_code, reason = "used by Space slice tests as they land")]
    pub(crate) fn for_tests() -> Self {
        crate::tests::test_state(false)
    }
}

/// Which optional parts of the Ingestions schedules surface this installation
/// can honour (WS15.S0). Every flag is off by default, and a flag is turned on
/// by configuration only once the backend behind it exists: `kinds` when a
/// schedule kind has a workflow (S1), `overlap` when overlap policy is enforced
/// (S2), `trigger` and `backfill` when the control verbs are implemented (S3).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ScheduleCapabilities {
    pub(crate) trigger: bool,
    pub(crate) backfill: bool,
    pub(crate) overlap: bool,
    pub(crate) kinds: Vec<String>,
    pub(crate) needs_activation: bool,
    /// Q2-WS15.L5c: crawl/batch pause, resume and cancel are honoured
    /// (QUARRY_EDGE__JOB_CONTROL on at the edge). The SPA draws the crawl
    /// controls only when this is true. Key `job_control` in the JSON value.
    pub(crate) job_control: bool,
    /// WS15.S4e: the newest fires of one schedule are read (QUARRY_EDGE__SCHEDULE_RUNS on at
    /// the edge). The SPA draws the Show runs control only when this is true, so a default
    /// deploy never offers a read the edge answers with 501. Key `runs` in the JSON value.
    pub(crate) runs: bool,
}

/// Parse `QUARRY_SCHEDULE_CAPABILITIES` (a JSON object). Unset, blank or
/// malformed input yields the all-off default, so a broken value can never
/// switch a capability on. The raw value is not logged, only the failure.
pub(crate) fn schedule_capabilities_from_env(raw: Option<&str>) -> ScheduleCapabilities {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return ScheduleCapabilities::default();
    };
    match serde_json::from_str::<ScheduleCapabilities>(raw) {
        Ok(capabilities) => capabilities,
        Err(_) => {
            tracing::warn!(
                "QUARRY_SCHEDULE_CAPABILITIES is not valid JSON; all schedule capabilities stay off"
            );
            ScheduleCapabilities::default()
        }
    }
}

pub(crate) async fn build_state() -> Result<AppState> {
    let cache_url = env::var("GATEWAY_CACHE_REDIS_URL").ok();
    let cache = crate::cache::ResultCache::connect(cache_url.as_deref()).await;
    let internal_api_key = internal_api_key()?;
    let session_core_service_token = required_service_token("SESSION_CORE_SERVICE_TOKEN")?;
    let user_core_service_token = required_service_token("USER_CORE_SERVICE_TOKEN")?;
    let billing_core_service_token = required_service_token("BILLING_CORE_SERVICE_TOKEN")?;
    let org_core_service_token = required_service_token("ORG_CORE_SERVICE_TOKEN")?;
    let audit_core_service_token = required_service_token("AUDIT_CORE_SERVICE_TOKEN")?;
    validate_service_token_distinctness(
        &[
            ("SESSION_CORE_SERVICE_TOKEN", &session_core_service_token),
            ("USER_CORE_SERVICE_TOKEN", &user_core_service_token),
            ("BILLING_CORE_SERVICE_TOKEN", &billing_core_service_token),
            ("ORG_CORE_SERVICE_TOKEN", &org_core_service_token),
            ("AUDIT_CORE_SERVICE_TOKEN", &audit_core_service_token),
        ],
        &[("INTERNAL_API_KEY", &internal_api_key)],
    )?;
    Ok(AppState {
        client: standard_http_client()?,
        // Streaming client: same connection hygiene but NO overall `.timeout()`, so
        // long-lived SSE streams are not severed at 25s. Connect/idle/dead-socket
        // protection remains; stream lifetime is bounded by the client disconnecting.
        streaming_client: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?,
        internal_api_key,
        // Same env var retrieval-engine reads; normalize + whitelist so an
        // unknown/unset value can never accidentally read as "strict".
        enforcement_mode: env::var("CONTROL_PLANE_ENFORCEMENT")
            .ok()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| matches!(v.as_str(), "off" | "permissive" | "strict"))
            .unwrap_or_else(|| "off".to_string()),
        browser_grant_mode: browser_grant_mode(
            env::var("BROWSER_GRANT_ENFORCEMENT").ok().as_deref(),
            &env::var("APP_ENV").unwrap_or_default(),
        ),
        browser_human_browse: exact_true_flag(env::var("BROWSER_HUMAN_BROWSE").ok().as_deref()),
        browser_live_view_only: env_exact_true("BROWSER_LIVE_VIEW_ONLY"),
        browser_consent_api: browser_consent_api_enabled(
            env::var("BROWSER_CONSENT_API").ok().as_deref(),
        ),
        browser_downloads: browser_downloads_enabled(env::var("BROWSER_DOWNLOADS").ok().as_deref()),
        browser_recipes: exact_true_flag(env::var("BROWSER_RECIPES").ok().as_deref()),
        browser_region_attach: env_exact_true("BROWSER_REGION_ATTACH"),
        browser_tab_mentions: env_exact_true(crate::domains::browser_context::TAB_MENTIONS_FLAG),
        browser_policy_admin_api: env_exact_true("BROWSER_POLICY_ADMIN_API"),
        browser_turn_context: browser_turn_context_enabled(
            env::var("BROWSER_TURN_CONTEXT").ok().as_deref(),
        ),
        browser_region_attach_key: env::var("BROWSER_REGION_ATTACH_KEY")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        auth_core_url: env_url("AUTH_CORE_URL", "http://auth-core:3011"),
        verevon_public_origin: canonical_public_origin(
            &env::var("VEREVON_PUBLIC_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:5173".to_owned()),
            !dev_flags_allowed(&env::var("APP_ENV").unwrap_or_default()),
        )?,
        session_core_url: env_url("SESSION_CORE_URL", "http://session-core:3017"),
        session_core_service_token,
        user_core_service_token,
        billing_core_url: env_url("BILLING_CORE_URL", "http://billing-core:3014"),
        billing_core_service_token,
        org_core_url: env_url("ORG_CORE_URL", "http://org-core:8080"),
        org_core_service_token,
        integration_core_url: env_url("INTEGRATION_CORE_URL", "http://integration-api:3026"),
        // audit-core (Control Plane) serves the audit read API the Trust Center
        // aggregates over. Its credential is audience-bound and cannot be reused
        // for Org or Billing authority.
        audit_core_url: env_url("AUDIT_CORE_URL", "http://audit-core:8187"),
        audit_core_service_token,
        // insight-core (Application Plane, registry-only) serves the connector
        // registry. Internal-key auth + x-org-id header, like the other cores.
        insight_core_url: env_url("INSIGHT_CORE_URL", "http://insight-core:3163"),
        leads_core_url: env_url("LEADS_CORE_URL", "http://leads-core:3164"),
        anbud_core_url: env_url("ANBUD_CORE_URL", "http://anbud-core:3138"),
        anbud_api_key: upstream_secret(
            "ANBUD_API_KEY",
            &env::var("ANBUD_API_KEY").unwrap_or_default(),
        )?,
        anbud_api_key_header: env::var("ANBUD_API_KEY_HEADER")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "X-API-Key".to_owned()),
        marketing_core_url: env_url("MARKETING_CORE_URL", "http://marketing-core:3165"),
        marketing_core_service_key: upstream_secret(
            "MARKETING_CORE_SERVICE_KEY",
            &env::var("MARKETING_CORE_SERVICE_KEY").unwrap_or_default(),
        )?,
        marketing_core_organization_id: env::var("MARKETING_CORE_ORGANIZATION_ID").unwrap_or_default().trim().to_owned(),
        // shipping-core (Ingestion Plane) — freight aggregator; on the
        // inter-plane bus, so the in-network name resolves from the gateway.
        shipping_core_url: env_url("SHIPPING_CORE_URL", "http://shipping-core:8080"),
        user_core_url: env_url("USER_CORE_URL", "http://user-core:3012"),
        application_convex_url: std::env::var("APPLICATION_CONVEX_URL").unwrap_or_default(),
        application_convex_service_key: upstream_secret(
            "APPLICATION_CONVEX_SERVICE_KEY",
            &std::env::var("APPLICATION_CONVEX_SERVICE_KEY").unwrap_or_default(),
        )?,
        graph_index_url: env_url("GRAPH_INDEX_URL", "http://dpv2-graph-index:9203"),
        quarry_edge_url: env_url("QUARRY_EDGE_URL", "http://quarry-edge:8082"),
        model_recommend_url: env_url(
            "MODEL_PLANE_RECOMMEND_URL",
            "http://model-gateway:8080/v1/recommend/plan",
        ),
        model_gateway_url: env_url("MODEL_GATEWAY_URL", "http://model-gateway:8080"),
        // Phase 7 B5 — Model Plane cost ledger (cost-core HTTP API). Reachable
        // over the inter-plane-bus alias; backs the cost/usage dashboard.
        cost_core_url: env_url("COST_CORE_URL", "http://model-plane-cost-core-1:8089"),
        model_gateway_dev_bearer: env::var("MODEL_GATEWAY_DEV_BEARER")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        // inference-core's internal HTTP (health server, :8082) serves the
        // router-policy GET/PUT used by the admin UI. Must be reachable from the
        // gateway over the inter-plane-bus (like model-gateway).
        inference_core_url: env_url("INFERENCE_CORE_URL", "http://inference-core:8082"),
        documents_api_url: env_url("DOCUMENTS_API_URL", "http://dpv2-documents-api:8010"),
        // retrieval-engine listens on container port 8004 (compose maps host 8014:8004);
        // gateway↔engine traffic rides the container port over inter-plane-bus.
        retrieval_engine_url: env_url("RETRIEVAL_ENGINE_URL", "http://dpv2-retrieval-engine:8004"),
        wiki_store_url: env_url("WIKI_STORE_URL", "http://dpv2-wiki-store:8011"),
        // Data Plane v2 diagnostics stay behind service contracts. Storage
        // backends (Qdrant, Quickwit, MinIO) remain private to dpv2-net.
        embedding_engine_url: env_url("EMBEDDING_ENGINE_URL", "http://dpv2-embedding-engine:9202"),
        quickwit_adapter_url: env_url("QUICKWIT_ADAPTER_URL", "http://dpv2-quickwit-adapter:9204"),
        // finspo-core (SharePoint/OneDrive storage analytics + duplicate management).
        finspo_core_url: env_url("FINSPO_CORE_URL", "http://finspo-api:3130"),
        imports_api_url: env_url("IMPORTS_API_URL", "http://imports-core:3025"),
        notification_core_url: env_url("NOTIFICATION_CORE_URL", "http://notification-core:3140"),
        notification_core_service_token: required_service_token("NOTIFICATION_CORE_SERVICE_TOKEN")?,
        information_core_url: env_url("INFORMATION_CORE_URL", "http://information-core:3190"),
        // Inbox / support conversations (Application Plane conversation-core-go).
        conversation_core_url: env_url("CONVERSATION_CORE_URL", "http://conversation-core-go:3160"),
        conversation_core_service_token: required_service_token("CONVERSATION_CORE_SERVICE_TOKEN")?,
        // Social publishing and calendar persistence (Application Plane social-core).
        social_core_url: env_url("SOCIAL_CORE_URL", "http://social-core:3162"),
        // Search verticals: SearXNG powers the videos category; quarry-edge (above)
        // owns web/images/answer; autocomplete-core backs typeahead suggestions.
        searxng_url: env_url("SEARXNG_URL", "http://searxng:8080"),
        autocomplete_core_url: env_url("AUTOCOMPLETE_CORE_URL", "http://autocomplete-core:3219"),
        autocomplete_token: autocomplete_internal_token()?,
        zammad_api_url: env_url("ZAMMAD_API_URL", "http://zammad-railsserver:3000"),
        remote_support_rendezvous_url: optional_env("REMOTE_SUPPORT_RENDEZVOUS_URL"),
        remote_support_relay_url: optional_env("REMOTE_SUPPORT_RELAY_URL"),
        remote_support_server_public_key: optional_env("REMOTE_SUPPORT_SERVER_PUBLIC_KEY"),
        zammad_api_token: upstream_secret(
            "ZAMMAD_API_TOKEN",
            &env::var("ZAMMAD_API_TOKEN").unwrap_or_default(),
        )?,
        audience_token_cache: new_audience_token_cache(),
        #[cfg(test)]
        browser_run_store: crate::domains::browser::new_browser_run_store(),
        // Reuse the cache's Dragonfly connection for fleet-wide rate limiting.
        // Built before `cache` is moved into the struct below (literal fields
        // evaluate top-to-bottom, so this borrow happens first).
        rate_limiter: crate::rate_limit::RateLimiter::from_cache(&cache),
        mail_task_rate_limiter: crate::rate_limit::RateLimiter::from_cache_with_rpm(
            &cache,
            mail_task_rate_limit_rpm(),
        ),
        cache,
        studio_store: crate::domains::studio::StudioStore::new(),
        schedule_capabilities: schedule_capabilities_from_env(
            env::var("QUARRY_SCHEDULE_CAPABILITIES").ok().as_deref(),
        ),
        allow_dev_actor_headers: dev_only_flag("ALLOW_DEV_ACTOR_HEADERS"),
        allow_dev_auth_bypass: dev_only_flag("ALLOW_DEV_AUTH_BYPASS"),
        continue_park_form: continue_park_form_enabled(),
        continue_question_form: continue_question_form_enabled(),
        model_entitlement_admin_enabled: model_entitlement_admin_enabled(),
        zdr_effective_posture_read: zdr_effective_posture_read_from(
            env::var("GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED")
                .ok()
                .as_deref(),
        ),
        zdr_waiver_ui_enabled: zdr_waiver_ui_enabled_from(
            env::var("ZDR_WAIVER_UI_ENABLED").ok().as_deref(),
        ),
        retention_decision_service_api_key: Some(upstream_secret(
            "GATEWAY_RETENTION_DECISION_SERVICE_API_KEY",
            &optional_env("GATEWAY_RETENTION_DECISION_SERVICE_API_KEY"),
        )?)
        .filter(|value| !value.is_empty()),
        agent_definition_versions: agent_definition_versions_enabled(),
        chat_process_output_enabled: chat_process_output_enabled(),
        chat_snapshots_enabled: chat_snapshots_enabled(),
        event_triggers_enabled: event_triggers_ui_enabled(),
        credential_bindings_enabled: credential_bindings_ui_enabled(),
        auth_step_up_route_enabled: auth_step_up_route_enabled(),
        mail_task_ingest_service_token: upstream_secret(
            "MAIL_TASK_INGEST_SERVICE_TOKEN",
            &optional_env("MAIL_TASK_INGEST_SERVICE_TOKEN"),
        )?,
        mail_task_nats_url: optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_URL"),
        mail_task_nats_user: optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_USER"),
        mail_task_nats_password: upstream_secret(
            "VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD",
            &optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD"),
        )?,
        // See the field doc comment: no production wiring yet.
        mail_task_pipeline: None,
        space_features: SpaceFeatureFlags::from_env(),
    })
}

/// `MAIL_TASK_RATE_LIMIT_PER_HOUR`, converted to the requests-per-minute unit
/// `RateLimiter` uses. Deliberately low (spec §6, "an attacker who obtains a
/// valid alias ... can drive unbounded execution-core spend") -- each mail-task
/// run is a full execution-core turn, not an ordinary read.
const DEFAULT_MAIL_TASK_RUNS_PER_HOUR: f64 = 20.0;

fn mail_task_rate_limit_rpm() -> f64 {
    let per_hour = env::var("MAIL_TASK_RATE_LIMIT_PER_HOUR")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(DEFAULT_MAIL_TASK_RUNS_PER_HOUR);
    per_hour / 60.0
}

fn standard_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        // Generous overall ceiling so browser-render scrapes (quarry /v1/scrape)
        // don't get cut off; most internal calls return in well under a second.
        .timeout(Duration::from_secs(25))
        // Never carry service credentials or signed delegation headers across
        // an upstream redirect, even when a compromised service returns 3xx.
        .redirect(reqwest::redirect::Policy::none())
        // Drop idle keep-alive connections before the upstream/bridge reaps them.
        .pool_idle_timeout(Duration::from_secs(20))
        .tcp_keepalive(Duration::from_secs(20))
        .build()?)
}

pub(crate) fn build_cors_layer() -> CorsLayer {
    let origins = env::var("VEREVON_ALLOWED_ORIGINS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|origin| HeaderValue::from_str(origin.trim()).ok())
                .collect::<Vec<_>>()
        })
        .filter(|origins| !origins.is_empty())
        .unwrap_or_else(|| {
            vec![
                HeaderValue::from_static("http://localhost:5173"),
                HeaderValue::from_static("http://127.0.0.1:5173"),
            ]
        });

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            HttpMethod::GET,
            HttpMethod::POST,
            HttpMethod::PUT,
            HttpMethod::PATCH,
            HttpMethod::DELETE,
            HttpMethod::OPTIONS,
        ])
        .allow_headers([
            ACCEPT,
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("x-verevon-org-id"),
        ])
        // WS10.S1: browsers may read the id from a response (support reports).
        .expose_headers([HeaderName::from_static("x-request-id")])
        .allow_credentials(true)
}

fn internal_api_key() -> Result<String> {
    match env::var("INTERNAL_API_KEY") {
        Ok(value) => {
            let trimmed = value.trim();
            // Reject the ENTIRE `change-me*` placeholder family (case-insensitive),
            // not just the exact string "change-me". A value like
            // `change-me-internal-service-secret` authenticates to no core, so a
            // gateway that boots with it silently 401s every internal-key call
            // (Brreg, /me/session-context, audit, …). Treat any change-me* / empty
            // value as "not configured".
            let is_placeholder =
                trimmed.is_empty() || trimmed.to_ascii_lowercase().starts_with("change-me");
            if !is_placeholder {
                return Ok(trimmed.to_owned());
            }
            // A placeholder was EXPLICITLY set — this is the silent-401 trap.
            // Fail loudly unless the operator explicitly opts into insecure dev
            // defaults, so a misconfigured key surfaces at boot, not as a fleet of
            // mysterious 401s once requests start flowing.
            if !trimmed.is_empty() && insecure_dev_defaults_enabled() {
                eprintln!(
                    "WARN: INTERNAL_API_KEY is a `change-me*` placeholder; internal-key \
                     calls to cores WILL 401 (running anyway: ALLOW_INSECURE_DEV_DEFAULTS=1)"
                );
                return Ok(trimmed.to_owned());
            }
            anyhow::bail!(
                "INTERNAL_API_KEY is empty or a `change-me*` placeholder; set the real \
                 shared internal key (or ALLOW_INSECURE_DEV_DEFAULTS=1 to override)"
            );
        }
        Err(_) => {
            // No INTERNAL_API_KEY at all: keep prior dev ergonomics — a debug build
            // (or explicit opt-in) boots with the canonical fake key.
            if cfg!(debug_assertions) || insecure_dev_defaults_enabled() {
                return Ok("change-me".into());
            }
            anyhow::bail!("INTERNAL_API_KEY must be set and cannot be a change-me* placeholder");
        }
    }
}

/// `AUTOCOMPLETE_INTERNAL_TOKEN`, the bearer the BFF presents to autocomplete-core. An unset or
/// empty value is allowed and keeps typeahead disabled. A `change-me*` placeholder (any case) gets
/// the same refusal as `INTERNAL_API_KEY`: it boots only when `ALLOW_INSECURE_DEV_DEFAULTS` is the
/// exact literal `1`.
fn autocomplete_internal_token() -> Result<String> {
    let trimmed = env::var("AUTOCOMPLETE_INTERNAL_TOKEN")
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !trimmed.to_ascii_lowercase().starts_with("change-me") {
        return Ok(trimmed);
    }
    if insecure_dev_defaults_enabled() {
        eprintln!(
            "WARN: AUTOCOMPLETE_INTERNAL_TOKEN is a `change-me*` placeholder; autocomplete-core \
             calls WILL 401 (running anyway: ALLOW_INSECURE_DEV_DEFAULTS=1)"
        );
        return Ok(trimmed);
    }
    anyhow::bail!(
        "AUTOCOMPLETE_INTERNAL_TOKEN cannot be a `change-me*` placeholder; unset it to disable \
         typeahead, set the real shared token, or set ALLOW_INSECURE_DEV_DEFAULTS=1 to override"
    )
}

/// `build_state` reads these upstream secrets through here (P2-bff-05-rel1, the R-ing-04 rule):
/// `ANBUD_API_KEY`, `MARKETING_CORE_SERVICE_KEY`, `APPLICATION_CONVEX_SERVICE_KEY`,
/// `ZAMMAD_API_TOKEN`, `MAIL_TASK_INGEST_SERVICE_TOKEN`, `VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD`,
/// `GATEWAY_RETENTION_DECISION_SERVICE_API_KEY`. The value is trimmed. A `change-me*` placeholder
/// (any case) refuses boot unless `ALLOW_INSECURE_DEV_DEFAULTS` is the exact literal `1`, as
/// `AUTOCOMPLETE_INTERNAL_TOKEN` does. Without that refusal the placeholder boots and goes out as a
/// credential, which gives silent 401s. The mail-task ingest token is the key the gateway checks
/// inbound delegations with, so a public placeholder there would also let anyone forge them. An
/// unset or blank value stays allowed and still leaves the integration off.
fn upstream_secret(name: &str, raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if !trimmed.to_ascii_lowercase().starts_with("change-me") {
        return Ok(trimmed.to_owned());
    }
    if insecure_dev_defaults_enabled() {
        eprintln!(
            "WARN: {name} is a `change-me*` placeholder, a public value and not a secret \
             (running anyway: ALLOW_INSECURE_DEV_DEFAULTS=1)"
        );
        return Ok(trimmed.to_owned());
    }
    anyhow::bail!(
        "{name} cannot be a `change-me*` placeholder; unset it to leave that integration off, \
         set the real shared secret, or set ALLOW_INSECURE_DEV_DEFAULTS=1 to override"
    )
}

fn required_service_token(name: &str) -> Result<String> {
    let value = env::var(name).unwrap_or_default();
    let trimmed = value.trim();
    if !service_token_is_secure(trimmed) {
        anyhow::bail!("{name} must be a non-placeholder secret of at least 32 characters");
    }
    Ok(trimmed.to_owned())
}

fn service_token_is_secure(value: &str) -> bool {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    value.len() >= 32
        && !lower.starts_with("test")
        && !lower.starts_with("placeholder")
        && !lower.starts_with("change-me")
        && !lower.starts_with("replace-with")
}

fn validate_service_token_distinctness(
    scoped: &[(&str, &str)],
    forbidden: &[(&str, &str)],
) -> Result<()> {
    for (index, (name, value)) in scoped.iter().enumerate() {
        if let Some((reused_name, _)) = forbidden
            .iter()
            .chain(scoped[..index].iter())
            .find(|(_, candidate)| candidate.trim() == value.trim())
        {
            anyhow::bail!("{name} must not reuse {reused_name}");
        }
    }
    Ok(())
}

/// Dev auth escape hatches must never activate in a production deploy, even if
/// the env var leaks into the profile: `APP_ENV=production` (or `prod`) wins
/// over the flag. Read exactly: only the literal `true` turns one on.
fn dev_only_flag(key: &str) -> bool {
    dev_gated(key, env_exact_true(key))
}

/// The dev-only switch whose vocabulary the flag registry documents as `1`, `true`, `yes` or `on`
/// (`CSP_REPORT_LOG_HOSTS`). It has the same production refusal as [`dev_only_flag`].
fn dev_only_documented_flag(key: &str) -> bool {
    dev_gated(key, env_bool(key, false))
}

/// Refuses an enabled dev switch in production, whatever its vocabulary.
fn dev_gated(key: &str, enabled: bool) -> bool {
    let app_env = env::var("APP_ENV").unwrap_or_default();
    if enabled && !dev_flags_allowed(&app_env) {
        eprintln!("refusing {key}=true because APP_ENV={app_env}; dev bypass is disabled");
        return false;
    }
    enabled
}

/// `VEREVON_GATEWAY_CONTINUE_PARK_ENABLED`, read once by `build_state` (WS03 S8-A). Off unless it
/// is exactly `true`.
fn continue_park_form_enabled() -> bool {
    continue_park_form_from(env::var("VEREVON_GATEWAY_CONTINUE_PARK_ENABLED").ok().as_deref())
}

/// Exactly the word `true`: not trimmed, not case-folded, and none of the other spellings
/// `env_bool` accepts. Anything else, including an unset variable, is off.
fn continue_park_form_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED`, read once by `build_state` (WS14 S5). Off unless
/// it is exactly `true`.
fn continue_question_form_enabled() -> bool {
    continue_question_form_from(
        env::var("VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED")
            .ok()
            .as_deref(),
    )
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: not trimmed, not
/// case-folded, and none of the other spellings `env_bool` accepts.
fn continue_question_form_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `MODEL_ENTITLEMENT_ADMIN_ENABLED`, read once by `build_state` (WS10-S5). Off unless it is
/// exactly `true`.
fn model_entitlement_admin_enabled() -> bool {
    model_entitlement_admin_from(env::var("MODEL_ENTITLEMENT_ADMIN_ENABLED").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a policy editor
/// that a stray `1` or `TRUE` switches on would let an operator set a policy that inference-core
/// is not yet enforcing.
fn model_entitlement_admin_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED`, read once by `build_state` (ZDR Z-03c). Off unless the
/// trimmed value is exactly `true`: a stray `1` or `TRUE` must not move the org-posture reader onto
/// a field an older org-core does not serve.
fn zdr_effective_posture_read_from(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("true")
}

/// `ZDR_WAIVER_UI_ENABLED`, read once by `build_state` (ZDR Z-19a). Off unless the trimmed value is
/// exactly the literal `true`.
fn zdr_waiver_ui_enabled_from(value: Option<&str>) -> bool {
    value.map(str::trim) == Some("true")
}

/// `AGENT_DEFINITION_VERSIONS`, read once by `build_state` (EP-6 S7/S8). Off unless it is exactly
/// `true`. The agent builder reads it from `AppState` at router build and per preview turn.
fn agent_definition_versions_enabled() -> bool {
    agent_definition_versions_from(env::var("AGENT_DEFINITION_VERSIONS").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a stray `1` or `TRUE`
/// would publish agent versions and expose a draft preview before anyone chose to.
pub(crate) fn agent_definition_versions_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `CHAT_PROCESS_OUTPUT_READ_ENABLED`, read once by `build_state` (W4-P3). Off unless it is exactly
/// `true`. The process output route reads it from `AppState`, never from the environment.
fn chat_process_output_enabled() -> bool {
    chat_process_output_from(env::var("CHAT_PROCESS_OUTPUT_READ_ENABLED").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a stray `1` or `TRUE`
/// would open a route that relays a process's output before anyone chose to.
fn chat_process_output_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `CHAT_SNAPSHOTS_ENABLED`, read once by `build_state` (W4-SH-6). Off unless it is exactly `true`. The snapshot
/// routes read it from `AppState`, never from the environment.
fn chat_snapshots_enabled() -> bool {
    chat_snapshots_from(env::var("CHAT_SNAPSHOTS_ENABLED").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a stray `1` or `TRUE` would open the
/// routes that copy a conversation and share it before anyone chose to.
fn chat_snapshots_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `EVENT_TRIGGERS_UI_ENABLED`, read once by `build_state` (W4-T7). Off unless it is exactly `true`. The trigger routes
/// read it from `AppState` at router build, never from the environment.
fn event_triggers_ui_enabled() -> bool {
    event_triggers_ui_from(env::var("EVENT_TRIGGERS_UI_ENABLED").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a stray `1` or `TRUE` would open the
/// routes that list and change a person's event triggers before anyone chose to.
fn event_triggers_ui_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `AUTH_STEP_UP_ROUTE_ENABLED`, read once by `build_state` (F-f03-15). Off unless it is exactly `true`. The
/// step-up route is registered from `AppState` when the router is built, never read from the environment.
fn auth_step_up_route_enabled() -> bool {
    auth_step_up_route_enabled_from(env::var("AUTH_STEP_UP_ROUTE_ENABLED").ok().as_deref())
}

/// Exactly the word `true`, for the same reason as `credential_bindings_ui_from`: a stray `1` or `TRUE` would
/// register a route that relays a password re-check before anyone chose to.
pub(crate) fn auth_step_up_route_enabled_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `CREDENTIAL_BINDINGS_UI_ENABLED`, read once by `build_state` (W4-CC-8). Off unless it is exactly `true`. The binding
/// and custom connector routes read it from `AppState` at request time, never from the environment.
fn credential_bindings_ui_enabled() -> bool {
    credential_bindings_ui_from(env::var("CREDENTIAL_BINDINGS_UI_ENABLED").ok().as_deref())
}

/// Exactly the word `true`: a stray `1` would open the routes that store a credential before anyone chose to.
fn credential_bindings_ui_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Exactly the word `true` (EP-3 S8): a promote switch that a stray `1` turned on would start
/// copying thread work product into the knowledge base before anyone chose to.
fn chat_artifact_promote_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Exactly the word `true` (WS18-S4, D-f06-C): a stray `1` would let a project start reach the model before anyone
/// chose to. Read once in `SpaceFeatureFlags::from_env`, and the stream handler sees it only through `AppState`.
fn chat_projects_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Exactly the word `true` (W5, F-t3-090): a switch that a stray `1` turned on would start saving a recording's text as
/// durable documents before anyone chose to.
fn chat_media_transcripts_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// `CLIENT_ENV_IDENTITY` (F-t3-098), read once by `build_state` through `SpaceFeatureFlags::from_env`.
/// Off unless it is exactly `true`; when on, the closed label of `APP_ENV`.
fn client_environment_identity() -> Option<&'static str> {
    client_env_identity_from(env::var("CLIENT_ENV_IDENTITY").ok().as_deref())
        .then(|| client_environment_label(&env::var("APP_ENV").unwrap_or_default()))
}

/// Exactly the word `true`, for the same reason as `continue_park_form_from`: a stray `1` would
/// publish the environment name before anyone chose to.
fn client_env_identity_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The four words the environment identity publishes, from `APP_ENV`: `production` (or `prod`),
/// `staging` (or `stage`), `development` (or `dev`), and `other` for anything else, unset included,
/// so an unrecognised value never reads as production. The raw value never leaves this function.
fn client_environment_label(app_env: &str) -> &'static str {
    match app_env.trim().to_ascii_lowercase().as_str() {
        "production" | "prod" => "production",
        "staging" | "stage" => "staging",
        "development" | "dev" => "development",
        _ => "other",
    }
}

fn dev_flags_allowed(app_env: &str) -> bool {
    !matches!(app_env.to_ascii_lowercase().as_str(), "production" | "prod")
}

/// How the BFF treats BrowserBroker grants for the in-app browser
/// (`BROWSER_GRANT_ENFORCEMENT`, WS1.B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BrowserGrantMode {
    /// No grant is issued; Quarry runs on its own client-supplied constraints.
    Off,
    /// A session grant is requested; if it cannot be issued the session still
    /// opens without one (logged by outcome class, never by host).
    Permissive,
    /// No grant, no browser session.
    Strict,
}

impl BrowserGrantMode {
    pub(crate) const fn issues_grants(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Resolves `BROWSER_GRANT_ENFORCEMENT` against `APP_ENV`. Outside production
/// the default is `permissive`; in production it is `strict`, and an explicit
/// `off` is refused there (the `dev_only_flag` pattern: report and fall back
/// to the safe value rather than silently run without grants). An unknown
/// value is the environment's default, never a looser mode.
pub(crate) fn browser_grant_mode(raw: Option<&str>, app_env: &str) -> BrowserGrantMode {
    let production = !dev_flags_allowed(app_env);
    let default = if production {
        BrowserGrantMode::Strict
    } else {
        BrowserGrantMode::Permissive
    };
    let Some(value) = raw.map(|value| value.trim().to_ascii_lowercase()) else {
        return default;
    };
    match value.as_str() {
        "" => default,
        "strict" => BrowserGrantMode::Strict,
        "permissive" => BrowserGrantMode::Permissive,
        "off" if production => {
            eprintln!(
                "refusing BROWSER_GRANT_ENFORCEMENT=off because APP_ENV={app_env}; using strict"
            );
            BrowserGrantMode::Strict
        }
        "off" => BrowserGrantMode::Off,
        _ => {
            eprintln!("unknown BROWSER_GRANT_ENFORCEMENT value; using the {default:?} default");
            default
        }
    }
}

/// Parses a browser switch that reads only the exact literal `true`: `BROWSER_HUMAN_BROWSE` and
/// `BROWSER_RECIPES` (V3d-bff-3), and, through [`env_exact_true`], `BROWSER_LIVE_VIEW_ONLY`,
/// `BROWSER_REGION_ATTACH`, `BROWSER_TAB_MENTIONS` and `BROWSER_POLICY_ADMIN_API` (V3e-bff-3). Not
/// trimmed and not case-folded, so `1`, `yes`, `on`, `TRUE` and a padded `true` are off in every
/// environment. The flag is never read against `APP_ENV`: every environment defaults to off.
pub(crate) fn exact_true_flag(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Parses `BROWSER_CONSENT_API` (Q3-E3.2-e3-gw). Only the literal `true` enables the consent
/// routes; unset, blank or any other value is off, in every environment.
pub(crate) fn browser_consent_api_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Parses `BROWSER_DOWNLOADS` (Q3-E3.4-7b). Only the literal `true` enables the downloads list and
/// the knowledge hand-off; unset, blank or any other value is off, in every environment.
pub(crate) fn browser_downloads_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Parses `BROWSER_TURN_CONTEXT` (QF-8). Only the literal `true` lets a chat turn carry
/// gateway-verified browser context; unset, blank or any other value is off, in every environment.
pub(crate) fn browser_turn_context_enabled(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Reads `key` as the exact word `true`: not trimmed, not case-folded, and no other spelling. An unset
/// or non-UTF-8 value is off.
fn env_exact_true(key: &str) -> bool {
    env::var_os(key).is_some_and(|value| value.to_str() == Some("true"))
}

/// The lenient reader: `1`, `true`, `yes` or `on` in any case turn the switch on, and the fallback
/// applies when the variable is unset. Used by the Space kill switches and by the switches the flag
/// registry documents with these words.
fn env_bool(key: &str, fallback: bool) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(fallback)
}

/// `ALLOW_INSECURE_DEV_DEFAULTS`, read exactly as Model Plane reads it: only the literal `1`
/// opens the insecure-default gate (Model Plane's `insecure_dev_defaults_from`, model-gateway
/// auth.rs). `true`, `TRUE`, `yes` and `on` leave it closed here too, so the BFF and Model
/// Plane never disagree about the one bypass they share (V3c-bff-3-rel2). Unset, empty, `0`
/// and padded values are closed as well.
fn insecure_dev_defaults_from(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Reads `ALLOW_INSECURE_DEV_DEFAULTS` through [`insecure_dev_defaults_from`].
fn insecure_dev_defaults_enabled() -> bool {
    insecure_dev_defaults_from(env::var("ALLOW_INSECURE_DEV_DEFAULTS").ok().as_deref())
}

/// Optional setting: empty string when unset. Trailing slashes are stripped so
/// URL joins never double up.
fn optional_env(key: &str) -> String {
    env::var(key)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_owned()
}

fn env_url(key: &str, fallback: &str) -> String {
    env::var(key)
        .unwrap_or_else(|_| fallback.into())
        .trim_end_matches('/')
        .to_owned()
}

fn canonical_public_origin(value: &str, production_like: bool) -> Result<String> {
    let parsed = url::Url::parse(value.trim())?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    let allowed_scheme =
        parsed.scheme() == "https" || (!production_like && parsed.scheme() == "http" && loopback);
    if !allowed_scheme
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        anyhow::bail!("VEREVON_PUBLIC_ORIGIN must be a canonical HTTPS origin");
    }
    Ok(parsed.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_public_origin, chat_artifact_promote_from, chat_projects_from,
        continue_park_form_from, continue_question_form_from, dev_flags_allowed,
        model_entitlement_admin_from, service_token_is_secure, standard_http_client,
        validate_service_token_distinctness, SpaceFeatureFlags,
    };

    #[tokio::test]
    async fn standard_upstream_client_never_follows_redirects() {
        use axum::{response::Redirect, routing::get, Router};

        let app = Router::new()
            .route(
                "/redirect",
                get(|| async { Redirect::temporary("/target") }),
            )
            .route("/target", get(|| async { "scoped token must not arrive" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect fixture");
        let address = listener.local_addr().expect("fixture address");
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let response = standard_http_client()
            .expect("client")
            .get(format!("http://{address}/redirect"))
            .header("x-service-token", "scoped-token")
            .send()
            .await
            .expect("redirect response");
        server.abort();
        assert_eq!(response.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
    }

    #[test]
    fn public_origin_requires_https_in_production_and_loopback_http_in_development() {
        assert_eq!(
            canonical_public_origin("https://verevon.example/", true).unwrap(),
            "https://verevon.example"
        );
        assert_eq!(
            canonical_public_origin("http://localhost:5173", false).unwrap(),
            "http://localhost:5173"
        );
        for value in [
            "http://verevon.example",
            "https://user:pass@verevon.example",
            "https://verevon.example/path",
            "https://verevon.example?tenant=acme",
            "https://verevon.example/#fragment",
        ] {
            assert!(canonical_public_origin(value, true).is_err(), "{value}");
        }
        assert!(canonical_public_origin("http://localhost:5173", true).is_err());
        assert!(canonical_public_origin("http://verevon.example", false).is_err());
    }

    #[test]
    fn service_tokens_reject_public_placeholder_families() {
        assert!(!service_token_is_secure(
            "test-generated-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "placeholder-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "replace-with-dedicated-random-32-byte-minimum-key"
        ));
        assert!(!service_token_is_secure(
            "change-me-notification-gateway-secret-32-bytes"
        ));
        assert!(service_token_is_secure(
            "generated-secret-value-with-at-least-32-bytes"
        ));
    }

    #[test]
    fn service_tokens_must_be_distinct_from_each_other_and_legacy_keys() {
        assert!(validate_service_token_distinctness(
            &[
                ("ORG_CORE_SERVICE_TOKEN", "org-token"),
                ("BILLING_CORE_SERVICE_TOKEN", "billing-token"),
            ],
            &[("INTERNAL_API_KEY", "legacy-key")],
        )
        .is_ok());
        assert!(validate_service_token_distinctness(
            &[
                ("ORG_CORE_SERVICE_TOKEN", "same-token"),
                ("BILLING_CORE_SERVICE_TOKEN", "same-token"),
            ],
            &[],
        )
        .is_err());
        assert!(validate_service_token_distinctness(
            &[("ORG_CORE_SERVICE_TOKEN", "legacy-key")],
            &[("INTERNAL_API_KEY", "legacy-key")],
        )
        .is_err());
    }

    #[test]
    fn dev_flags_refused_in_production_profiles() {
        assert!(!dev_flags_allowed("production"));
        assert!(!dev_flags_allowed("Production"));
        assert!(!dev_flags_allowed("prod"));
    }

    #[test]
    fn dev_flags_allowed_outside_production() {
        assert!(dev_flags_allowed(""));
        assert!(dev_flags_allowed("development"));
        assert!(dev_flags_allowed("staging"));
    }

    #[test]
    fn browser_grant_mode_defaults_permissive_outside_production_and_strict_in_it() {
        use super::{browser_grant_mode, BrowserGrantMode::*};
        assert_eq!(browser_grant_mode(None, ""), Permissive);
        assert_eq!(browser_grant_mode(None, "development"), Permissive);
        assert_eq!(browser_grant_mode(None, "production"), Strict);
        assert_eq!(browser_grant_mode(Some("  "), "prod"), Strict);
        assert_eq!(browser_grant_mode(Some("STRICT"), ""), Strict);
        assert_eq!(
            browser_grant_mode(Some("permissive"), "production"),
            Permissive
        );
        assert_eq!(browser_grant_mode(Some("off"), "development"), Off);
    }

    #[test]
    fn browser_grant_mode_refuses_off_in_production_and_never_loosens_on_a_typo() {
        use super::{browser_grant_mode, BrowserGrantMode::*};
        assert_eq!(browser_grant_mode(Some("off"), "production"), Strict);
        assert_eq!(browser_grant_mode(Some("Off"), "Prod"), Strict);
        assert_eq!(browser_grant_mode(Some("of"), "production"), Strict);
        assert_eq!(browser_grant_mode(Some("disabled"), ""), Permissive);
        assert!(!Off.issues_grants());
        assert!(Permissive.issues_grants() && Strict.issues_grants());
    }

    /// WS03 S8-A: the park form of `/continue` is off unless the variable is exactly `true`.
    /// Nothing is trimmed or case-folded, and the other truthy spellings `env_bool` accepts
    /// (`1`, `yes`, `on`) do not turn it on.
    #[test]
    fn the_park_form_flag_is_on_only_for_the_exact_word_true() {
        assert!(continue_park_form_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("TRUE"),
            Some("True"),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("yes"),
            Some("on"),
            Some("truee"),
        ] {
            assert!(!continue_park_form_from(value), "{value:?}");
        }
    }

    /// WS14 S5: the question form of `/continue` is off unless the variable is exactly `true`.
    #[test]
    fn the_question_form_flag_is_on_only_for_the_exact_word_true() {
        assert!(continue_question_form_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("TRUE"),
            Some("True"),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("yes"),
            Some("on"),
            Some("truee"),
        ] {
            assert!(!continue_question_form_from(value), "{value:?}");
        }
    }

    /// WS14 S5: the question flag is read once, in `build_state`, from the environment, and the
    /// compose file declares it once, default off.
    #[test]
    fn the_question_form_flag_is_read_once_by_build_state_and_declared_once_in_compose() {
        let config = include_str!("config.rs").replace("
", "
");
        let production = config.split("#[cfg(test)]
mod tests").next().unwrap_or(&config);
        assert_eq!(
            production
                .matches("\"VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED\"")
                .count(),
            1
        );
        assert_eq!(
            production
                .matches("continue_question_form: continue_question_form_enabled(),")
                .count(),
            1
        );
        let compose = include_str!("../../../docker-compose.yml").replace("
", "
");
        assert_eq!(
            compose
                .matches(
                    "VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED: ${VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED:-false}
"
                )
                .count(),
            1
        );
    }

    /// WS10-S5: the model-entitlement admin switch is on only for the exact word `true`, and is
    /// read once, in `build_state`; the orgs handlers see it only through `AppState`.
    #[test]
    fn the_model_entitlement_admin_flag_is_on_only_for_the_exact_word_true() {
        assert!(model_entitlement_admin_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("TRUE"),
            Some("True"),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("yes"),
            Some("on"),
        ] {
            assert!(!model_entitlement_admin_from(value), "{value:?}");
        }
    }

    /// WS18-S4 (D-f06-C): the project-chat switch is on only for the exact word `true`, and a default
    /// `SpaceFeatureFlags` has it off with the browser-site switch. The stream handler reads it only through `AppState`.
    #[test]
    fn the_chat_projects_flag_is_on_only_for_the_exact_word_true() {
        assert!(chat_projects_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("TRUE"),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("yes"),
            Some("on"),
        ] {
            assert!(!chat_projects_from(value), "{value:?}");
        }
        assert!(!SpaceFeatureFlags::default().chat_projects);
        assert!(!SpaceFeatureFlags::default().browser_sites_enabled);
        let streams = include_str!("domains/chat/streams.rs").replace("\r\n", "\n");
        let handlers = streams
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(&streams);
        assert!(
            !handlers.contains("GATEWAY_CHAT_PROJECTS_ENABLED"),
            "a handler reads the env"
        );
    }

    /// EP-3 S8: the artifact promote switch is on only for the exact word `true`, read once in
    /// `SpaceFeatureFlags::from_env`, and the documents handler sees it only through `AppState`.
    #[test]
    fn the_chat_artifact_promote_flag_is_on_only_for_the_exact_word_true() {
        assert!(chat_artifact_promote_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("false"),
            Some("TRUE"),
            Some(" true"),
            Some("true "),
            Some("1"),
            Some("yes"),
            Some("on"),
        ] {
            assert!(!chat_artifact_promote_from(value), "{value:?}");
        }
        assert!(!SpaceFeatureFlags::default().chat_artifact_promote);
        let documents = include_str!("domains/chat/documents.rs").replace("\r\n", "\n");
        let handlers = documents.split("#[cfg(test)]\nmod tests").next().unwrap_or(&documents);
        assert!(!handlers.contains("CHAT_ARTIFACT_PROMOTE_ENABLED"), "a handler reads the env");
        assert!(!handlers.contains("env::var"), "a handler reads the env");
        let config = include_str!("config.rs").replace("\r\n", "\n");
        let production = config.split("#[cfg(test)]\nmod tests").next().unwrap_or(&config);
        assert_eq!(production.matches("\"MODEL_ENTITLEMENT_ADMIN_ENABLED\"").count(), 1);
        for module in [
            include_str!("domains/orgs/model_entitlement.rs"),
            include_str!("domains/orgs/settings.rs"),
        ] {
            let module = module.replace("\r\n", "\n");
            let handlers = module.split("#[cfg(test)]\nmod tests").next().unwrap_or(&module);
            assert!(!handlers.contains("MODEL_ENTITLEMENT_ADMIN_ENABLED"), "a handler reads the env");
            assert!(!handlers.contains("env::var"), "a handler reads the env");
        }
    }

    /// The flag is read once, in `build_state`, from this one variable: no handler reads the
    /// environment (a per-call read is a process-wide global that parallel tests race on).
    #[test]
    fn the_park_form_flag_is_read_once_by_build_state_and_by_no_handler() {
        // The source's own line endings do not matter here: fold them away first.
        let config = include_str!("config.rs").replace("\r\n", "\n");
        let production = config.split("#[cfg(test)]\nmod tests").next().unwrap_or(&config);
        assert_eq!(production.matches("\"VEREVON_GATEWAY_CONTINUE_PARK_ENABLED\"").count(), 1);
        assert_eq!(production.matches("continue_park_form: continue_park_form_enabled(),").count(), 1);
        let orchestration = include_str!("domains/orchestration.rs").replace("\r\n", "\n");
        let handlers = orchestration
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(&orchestration);
        assert!(!handlers.contains("VEREVON_GATEWAY_CONTINUE_PARK_ENABLED"), "a handler reads the env");
        assert!(!handlers.contains("env::var"), "a handler reads the env");
        // The compose file declares it once, default off.
        let compose = include_str!("../../../docker-compose.yml").replace("\r\n", "\n");
        assert_eq!(
            compose
                .matches(
                    "VEREVON_GATEWAY_CONTINUE_PARK_ENABLED: ${VEREVON_GATEWAY_CONTINUE_PARK_ENABLED:-false}\n"
                )
                .count(),
            1
        );
        assert_eq!(
            compose.matches("\n      VEREVON_GATEWAY_CONTINUE_PARK_ENABLED:").count(),
            1,
            "declared exactly once"
        );
    }

    /// Runs `body` with the given env vars set (and restored), serialised on the shared lock.
    fn with_env<T>(vars: &[(&str, Option<&str>)], body: impl FnOnce() -> T) -> T {
        let _guard = super::TEST_ENV_LOCK.blocking_lock();
        let saved: Vec<(String, Option<String>)> = vars
            .iter()
            .map(|(key, _)| ((*key).to_owned(), std::env::var(key).ok()))
            .collect();
        for (key, value) in vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let out = body();
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        out
    }

    /// P-GW1: a default-built flag set has every Space flag off, so a test literal that never names
    /// the field and `AppState::for_tests()` can never accidentally turn a route on.
    #[test]
    fn space_feature_flags_default_has_every_flag_off() {
        let flags = super::SpaceFeatureFlags::default();
        assert!(!flags.denials_typed);
        assert!(!flags.csp_report_log_hosts);
        assert!(!flags.connectivity_banner_ui);
        assert!(!flags.human_posts_api);
        assert!(!flags.membership_writer_api);
        assert!(!flags.policy_admin_api);
        assert!(!flags.realtime_stream);
        assert!(!flags.room_details_ui);
        assert!(!flags.people_search_api);
        assert!(!flags.room_archive_api);
        assert!(!flags.room_artifacts);
        assert!(!flags.send_idempotency_api);
        assert!(!flags.thread_pane_ui);
    }

    /// WS10.S6 (GW-2): `RUN_TRACE_API` is on only for the literal `true`, in every environment,
    /// development included, so the run-trace route ships dark. `1`, `yes`, `on`, `TRUE` and a padded
    /// `true` leave it off (V3d-bff-3).
    #[test]
    fn run_trace_api_flag_is_default_off_in_every_environment() {
        assert!(!super::SpaceFeatureFlags::default().run_trace_api);
        let read = || super::SpaceFeatureFlags::from_env().run_trace_api;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(&[("APP_ENV", app_env), ("RUN_TRACE_API", None)], || {
                assert!(!read(), "APP_ENV={app_env:?}");
            });
        }
        for value in [
            "", "0", "1", "false", "off", "yes", "on", "TRUE", " true", "true ",
        ] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("RUN_TRACE_API", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        for app_env in ["development", "production"] {
            let vars = [("APP_ENV", Some(app_env)), ("RUN_TRACE_API", Some("true"))];
            with_env(&vars, || assert!(read(), "APP_ENV={app_env}"));
        }
    }

    /// `CHAT_RESEARCH_RUNS` is default off in every environment and reads only the literal `true`:
    /// `1`, `yes`, `on`, `TRUE` and a padded `true` leave the research-run routes unregistered.
    #[test]
    fn chat_research_runs_flag_is_default_off_and_reads_only_the_literal_true() {
        assert!(!super::SpaceFeatureFlags::default().chat_research_runs);
        let read = || super::SpaceFeatureFlags::from_env().chat_research_runs;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(
                &[("APP_ENV", app_env), ("CHAT_RESEARCH_RUNS", None)],
                || {
                    assert!(!read(), "APP_ENV={app_env:?}");
                },
            );
        }
        for value in ["", "0", "1", "yes", "on", "TRUE", " true", "true "] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("CHAT_RESEARCH_RUNS", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("CHAT_RESEARCH_RUNS", Some("true")),
        ];
        with_env(&vars, || assert!(read()));
    }

    /// `WEB_DOMAIN_POLICY_BFF` (O-23) is default off in every environment and reads only the literal `true`:
    /// `1`, `yes`, `on`, `TRUE` and a padded `true` leave the BFF's search and crawl exactly as they were.
    #[test]
    fn web_domain_policy_bff_flag_is_default_off_and_reads_only_the_literal_true() {
        assert!(!super::SpaceFeatureFlags::default().web_domain_policy_bff);
        let read = || super::SpaceFeatureFlags::from_env().web_domain_policy_bff;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(
                &[("APP_ENV", app_env), ("WEB_DOMAIN_POLICY_BFF", None)],
                || {
                    assert!(!read(), "APP_ENV={app_env:?}");
                },
            );
        }
        for value in ["", "0", "1", "yes", "on", "TRUE", " true", "true ", "enforce"] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("WEB_DOMAIN_POLICY_BFF", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("WEB_DOMAIN_POLICY_BFF", Some("true")),
        ];
        with_env(&vars, || assert!(read()));
    }

    /// `ORG_COMPLIANCE_ROLE_ENABLED` (O-21) is default off in every environment and reads only the literal `true`:
    /// `1`, `yes`, `on`, `TRUE` and a padded `true` leave the hold and audit role lists exactly as they were.
    #[test]
    fn org_compliance_role_flag_is_default_off_and_reads_only_the_literal_true() {
        assert!(!super::SpaceFeatureFlags::default().org_compliance_role);
        let read = || super::SpaceFeatureFlags::from_env().org_compliance_role;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(
                &[("APP_ENV", app_env), ("ORG_COMPLIANCE_ROLE_ENABLED", None)],
                || {
                    assert!(!read(), "APP_ENV={app_env:?}");
                },
            );
        }
        for value in ["", "0", "1", "yes", "on", "TRUE", " true", "true "] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("ORG_COMPLIANCE_ROLE_ENABLED", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("ORG_COMPLIANCE_ROLE_ENABLED", Some("true")),
        ];
        with_env(&vars, || assert!(read()));
    }

    /// `ALLOW_DEV_ACTOR_HEADERS` and `ALLOW_DEV_AUTH_BYPASS` are dev escape hatches: each reads only
    /// the literal `true`, is off unless set, and is refused in production whatever it says.
    #[test]
    fn dev_escape_hatches_read_only_the_literal_true_and_are_refused_in_production() {
        for key in ["ALLOW_DEV_ACTOR_HEADERS", "ALLOW_DEV_AUTH_BYPASS"] {
            with_env(&[("APP_ENV", Some("development")), (key, None)], || {
                assert!(!super::dev_only_flag(key), "{key} unset");
            });
            for value in ["", "0", "1", "yes", "on", "TRUE", " true", "true "] {
                let vars = [("APP_ENV", Some("development")), (key, Some(value))];
                with_env(&vars, || {
                    assert!(!super::dev_only_flag(key), "{key}={value:?}")
                });
            }
            with_env(
                &[("APP_ENV", Some("development")), (key, Some("true"))],
                || {
                    assert!(super::dev_only_flag(key), "{key} true in development");
                },
            );
            with_env(
                &[("APP_ENV", Some("production")), (key, Some("true"))],
                || {
                    assert!(!super::dev_only_flag(key), "{key} true in production");
                },
            );
        }
    }

    /// `CSP_REPORT_LOG_HOSTS` keeps the `1`, `true`, `yes` and `on` reading that the flag registry
    /// documents for it, so it is a documented exception to the exact-literal rule. It is still
    /// refused in production.
    #[test]
    fn csp_report_log_hosts_keeps_the_documented_words_outside_production() {
        for value in ["1", "true", "yes", "on"] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("CSP_REPORT_LOG_HOSTS", Some(value)),
            ];
            with_env(&vars, || {
                assert!(
                    super::SpaceFeatureFlags::from_env().csp_report_log_hosts,
                    "{value}"
                );
            });
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("CSP_REPORT_LOG_HOSTS", Some("1")),
        ];
        with_env(&vars, || {
            assert!(!super::SpaceFeatureFlags::from_env().csp_report_log_hosts);
        });
    }

    /// The BFF-only Space switches that default off (`BFF_EXACT_TRUE_SPACE_SWITCHES`) read only the
    /// exact literal `true` and have no development default: unset is off in development and
    /// production, and `1`, `yes`, `on`, `TRUE`, a padded `true` and an empty value are off in both.
    #[test]
    fn bff_default_off_space_switches_are_on_only_for_the_exact_literal_true() {
        for key in super::BFF_EXACT_TRUE_SPACE_SWITCHES {
            let read = |app_env: &str, value: Option<&str>| {
                let vars = [("APP_ENV", Some(app_env)), (key, value)];
                with_env(&vars, || super::space_flag(key))
            };
            assert!(!read("development", None), "{key} unset in development");
            assert!(!read("production", None), "{key} unset in production");
            assert!(read("production", Some("true")), "{key} set to true");
            assert!(
                read("development", Some("true")),
                "{key} set to true in development"
            );
            for value in ["", "1", "yes", "on", "TRUE", "True", " true", "true "] {
                assert!(!read("development", Some(value)), "{key}={value:?}");
                assert!(!read("production", Some(value)), "{key}={value:?}");
            }
        }
    }

    /// P-GW1 / WS01-S5 / WS03-S4: an unset Space flag is OFF under `APP_ENV=production`, ON in
    /// development, and an explicit value wins in both.
    #[test]
    fn space_flag_is_off_in_production_when_unset_and_on_in_dev() {
        let key = "SPACE_TEST_ONLY_FLAG_P_GW1";
        with_env(&[("APP_ENV", Some("production")), (key, None)], || {
            assert!(!super::space_flag(key));
        });
        with_env(&[("APP_ENV", Some("prod")), (key, None)], || {
            assert!(!super::space_flag(key));
        });
        with_env(&[("APP_ENV", Some("development")), (key, None)], || {
            assert!(super::space_flag(key));
        });
        with_env(&[("APP_ENV", Some("production")), (key, Some("true"))], || {
            assert!(super::space_flag(key));
        });
        with_env(&[("APP_ENV", Some("development")), (key, Some("off"))], || {
            assert!(!super::space_flag(key));
        });
    }

    /// Model Plane reads each name in `MODEL_PLANE_EXACT_TRUE_SWITCHES` as the exact literal
    /// `true` (`switch_from`), so the gateway does too: `1`, `yes`, `on`, `TRUE`, a padded `true`
    /// and an empty value are off in every environment. An unset value is off everywhere as well
    /// (V3c-bff-3-rel1): Model Plane's base compose defaults these off, so there is no dev default.
    #[test]
    fn model_plane_mirrored_space_switches_are_on_only_for_the_exact_literal_true() {
        for key in super::MODEL_PLANE_EXACT_TRUE_SWITCHES {
            let read = |app_env: &str, value: Option<&str>| {
                let vars = [("APP_ENV", Some(app_env)), (key, value)];
                with_env(&vars, || super::space_flag(key))
            };
            assert!(!read("development", None), "{key} unset in development");
            assert!(!read("production", None), "{key} unset in production");
            assert!(read("production", Some("true")), "{key} set to true");
            assert!(
                read("development", Some("true")),
                "{key} set to true in development"
            );
            for value in ["", "1", "yes", "on", "TRUE", "True", " true", "true "] {
                assert!(!read("development", Some(value)), "{key}={value:?}");
                assert!(!read("production", Some(value)), "{key}={value:?}");
            }
        }
    }

    /// V3c-bff-3-rel1: each Space flag that mirrors a Model Plane switch reads its own variable,
    /// is OFF when that variable is unset in development as well as production, and is on only for
    /// the literal `true`. The development default that used to show these routes is gone.
    #[test]
    fn model_plane_mirrored_space_flags_are_off_in_development_when_unset() {
        let cases: [(&str, fn(&super::SpaceFeatureFlags) -> bool); 7] = [
            ("SPACE_HUMAN_POSTS_API", |flags| flags.human_posts_api),
            ("SPACE_MESSAGE_SEARCH_API", |flags| flags.message_search_api),
            ("SPACE_MESSAGE_REVISE_API", |flags| flags.message_revise_api),
            ("SPACE_TIMELINE_API", |flags| flags.timeline_api),
            ("SPACE_RUN_LIVE_VIEW", |flags| flags.run_live_view),
            ("SPACE_RUN_LIVE_TEXT", |flags| flags.run_live_text),
            ("SPACE_RUN_MANAGER_STOP", |flags| flags.run_manager_stop),
        ];
        for (key, read) in cases {
            let flag = |app_env: &str, value: Option<&str>| {
                with_env(&[("APP_ENV", Some(app_env)), (key, value)], || {
                    read(&super::SpaceFeatureFlags::from_env())
                })
            };
            assert!(!flag("development", None), "{key} unset in development");
            assert!(!flag("production", None), "{key} unset in production");
            assert!(
                flag("development", Some("true")),
                "{key} true in development"
            );
            assert!(flag("production", Some("true")), "{key} true in production");
            assert!(
                !flag("development", Some("1")),
                "{key}=1 is not the literal true"
            );
        }
    }

    /// The Space flags outside `MODEL_PLANE_EXACT_TRUE_SWITCHES` are not Model Plane switches, so
    /// they keep the lenient `env_bool` words. The exact-literal rule does not spread to them.
    #[test]
    fn other_space_flags_keep_the_lenient_reading() {
        let key = "SPACE_SIDEBAR_API";
        assert!(!super::MODEL_PLANE_EXACT_TRUE_SWITCHES.contains(&key));
        for value in ["1", "yes", "on", "TRUE"] {
            let vars = [("APP_ENV", Some("production")), (key, Some(value))];
            with_env(&vars, || assert!(super::space_flag(key), "{value:?}"));
        }
    }

    /// V3c-bff-3-rel2: `ALLOW_INSECURE_DEV_DEFAULTS` is shared with Model Plane, whose
    /// `insecure_dev_defaults_from` opens its gate only for the literal `1`. The BFF reads it the
    /// same way, so `true`, `TRUE`, `yes`, `on`, a padded `1`, `0`, an empty value and an unset
    /// variable all leave the insecure-default gate closed.
    #[test]
    fn insecure_dev_defaults_opens_only_for_the_exact_literal_one() {
        assert!(super::insecure_dev_defaults_from(Some("1")));
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("true"),
            Some("TRUE"),
            Some("True"),
            Some("yes"),
            Some("on"),
            Some(" 1 "),
            Some("1 "),
            Some(" 1"),
        ] {
            assert!(!super::insecure_dev_defaults_from(value), "{value:?}");
        }
    }

    /// V3c-bff-3-rel2: a `change-me*` `INTERNAL_API_KEY` boots only when the flag is exactly `1`.
    /// `true` and the other spellings the lenient reader used to accept leave the refusal in
    /// place, as does an unset flag.
    #[test]
    fn placeholder_internal_api_key_boots_only_when_the_flag_is_exactly_one() {
        let placeholder = "change-me-internal-service-secret";
        for flag in [
            None,
            Some("0"),
            Some("true"),
            Some("TRUE"),
            Some("yes"),
            Some("on"),
        ] {
            let vars = [
                ("INTERNAL_API_KEY", Some(placeholder)),
                ("ALLOW_INSECURE_DEV_DEFAULTS", flag),
            ];
            with_env(&vars, || {
                assert!(super::internal_api_key().is_err(), "{flag:?}");
            });
        }
        let vars = [
            ("INTERNAL_API_KEY", Some(placeholder)),
            ("ALLOW_INSECURE_DEV_DEFAULTS", Some("1")),
        ];
        with_env(&vars, || {
            assert_eq!(super::internal_api_key().unwrap(), placeholder);
        });
    }

    /// R-ing-04: `AUTOCOMPLETE_INTERNAL_TOKEN` gets the same refusal as `INTERNAL_API_KEY`. Any
    /// `change-me*` value (any case, padded or not) refuses to boot unless `ALLOW_INSECURE_DEV_DEFAULTS`
    /// is the exact literal `1`; `true` and the other spellings leave the refusal in place.
    #[test]
    fn autocomplete_token_refuses_change_me_placeholders_unless_the_switch_is_exactly_one() {
        for placeholder in ["change-me", "change-me-x", "CHANGE-ME-X", " change-me-x "] {
            for flag in [
                None,
                Some("0"),
                Some("true"),
                Some("TRUE"),
                Some("yes"),
                Some("on"),
            ] {
                let vars = [
                    ("AUTOCOMPLETE_INTERNAL_TOKEN", Some(placeholder)),
                    ("ALLOW_INSECURE_DEV_DEFAULTS", flag),
                ];
                with_env(&vars, || {
                    assert!(
                        super::autocomplete_internal_token().is_err(),
                        "{placeholder:?} {flag:?}"
                    );
                });
            }
        }
        for placeholder in ["change-me", "change-me-x"] {
            let vars = [
                ("AUTOCOMPLETE_INTERNAL_TOKEN", Some(placeholder)),
                ("ALLOW_INSECURE_DEV_DEFAULTS", Some("1")),
            ];
            with_env(&vars, || {
                assert_eq!(super::autocomplete_internal_token().unwrap(), placeholder);
            });
        }
    }

    /// R-ing-04: a real autocomplete token passes through trimmed with the insecure switch off, and an
    /// unset token stays allowed (empty means typeahead is disabled, as before).
    #[test]
    fn autocomplete_token_accepts_a_real_value_and_keeps_unset_allowed() {
        let real = "fixture-autocomplete-token-aaaaaaaaaaaaaaaa";
        let padded = format!("  {real}  ");
        let vars = [
            ("AUTOCOMPLETE_INTERNAL_TOKEN", Some(padded.as_str())),
            ("ALLOW_INSECURE_DEV_DEFAULTS", None),
        ];
        with_env(&vars, || {
            assert_eq!(super::autocomplete_internal_token().unwrap(), real);
        });
        let vars = [
            ("AUTOCOMPLETE_INTERNAL_TOKEN", None),
            ("ALLOW_INSECURE_DEV_DEFAULTS", None),
        ];
        with_env(&vars, || {
            assert_eq!(super::autocomplete_internal_token().unwrap(), "");
        });
    }

    /// The rule `upstream_secret` applies: the value is trimmed, a case-insensitive `change-me`
    /// prefix is refused unless the switch is exactly `1`, and an empty value passes through.
    #[test]
    fn upstream_secret_trims_and_refuses_change_me_unless_the_switch_is_exactly_one() {
        for raw in ["change-me", " CHANGE-ME-x ", "Change-Me-x"] {
            for flag in [None, Some("0"), Some("true"), Some("TRUE"), Some(" 1")] {
                with_env(&[("ALLOW_INSECURE_DEV_DEFAULTS", flag)], || {
                    assert!(
                        super::upstream_secret("ANBUD_API_KEY", raw).is_err(),
                        "{raw:?} {flag:?}"
                    );
                });
            }
            with_env(&[("ALLOW_INSECURE_DEV_DEFAULTS", Some("1"))], || {
                assert_eq!(
                    super::upstream_secret("ANBUD_API_KEY", raw).unwrap(),
                    raw.trim()
                );
            });
        }
        with_env(&[("ALLOW_INSECURE_DEV_DEFAULTS", None)], || {
            assert_eq!(
                super::upstream_secret("ZAMMAD_API_TOKEN", "  fixture-zammad-token-0001 ").unwrap(),
                "fixture-zammad-token-0001"
            );
            assert_eq!(
                super::upstream_secret("ZAMMAD_API_TOKEN", "  ").unwrap(),
                ""
            );
        });
    }

    /// The seven upstream secrets `build_state` reads through `upstream_secret` (P2-bff-05-rel1).
    const UPSTREAM_SECRETS: [&str; 7] = [
        "ANBUD_API_KEY",
        "MARKETING_CORE_SERVICE_KEY",
        "APPLICATION_CONVEX_SERVICE_KEY",
        "ZAMMAD_API_TOKEN",
        "MAIL_TASK_INGEST_SERVICE_TOKEN",
        "VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD",
        "GATEWAY_RETENTION_DECISION_SERVICE_API_KEY",
    ];

    /// Fake core tokens: distinct from each other and from the fake internal key, each at least 32
    /// characters, so `build_state` boots with nothing but the secret under test.
    const FAKE_CORE_TOKENS: [(&str, &str); 7] = [
        (
            "SESSION_CORE_SERVICE_TOKEN",
            "fixture-session-core-service-token-0001",
        ),
        (
            "USER_CORE_SERVICE_TOKEN",
            "fixture-user-core-service-token-0001",
        ),
        (
            "BILLING_CORE_SERVICE_TOKEN",
            "fixture-billing-core-service-token-0001",
        ),
        (
            "ORG_CORE_SERVICE_TOKEN",
            "fixture-org-core-service-token-0001",
        ),
        (
            "AUDIT_CORE_SERVICE_TOKEN",
            "fixture-audit-core-service-token-0001",
        ),
        (
            "NOTIFICATION_CORE_SERVICE_TOKEN",
            "fixture-notification-core-service-token-0001",
        ),
        (
            "CONVERSATION_CORE_SERVICE_TOKEN",
            "fixture-conversation-core-service-token-0001",
        ),
    ];

    /// Sets the given env values (`None` removes one) and puts the previous values back on drop.
    /// Async tests hold `TEST_ENV_LOCK` across their awaits with it; `with_env` is for sync tests.
    struct EnvOverlay(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl EnvOverlay {
        fn apply(vars: Vec<(&'static str, Option<String>)>) -> Self {
            let previous = vars
                .into_iter()
                .map(|(key, value)| {
                    let old = std::env::var_os(key);
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                    (key, old)
                })
                .collect();
            Self(previous)
        }
    }

    impl Drop for EnvOverlay {
        fn drop(&mut self) {
            for (key, old) in self.0.drain(..).rev() {
                match old {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// Boots `build_state` with `secret` set to `value`, every other upstream secret unset, and
    /// `ALLOW_INSECURE_DEV_DEFAULTS` set to `insecure`. Holds the shared env lock across the boot.
    async fn boot_with(
        secret: &'static str,
        value: Option<&str>,
        insecure: Option<&str>,
    ) -> anyhow::Result<super::AppState> {
        let _lock = super::TEST_ENV_LOCK.lock().await;
        let mut vars: Vec<(&'static str, Option<String>)> = vec![
            ("APP_ENV", Some("development".to_owned())),
            (
                "VEREVON_PUBLIC_ORIGIN",
                Some("http://localhost:5173".to_owned()),
            ),
            ("GATEWAY_CACHE_REDIS_URL", None),
            ("AUTOCOMPLETE_INTERNAL_TOKEN", None),
            ("ALLOW_INSECURE_DEV_DEFAULTS", insecure.map(str::to_owned)),
            (
                "INTERNAL_API_KEY",
                Some("fixture-internal-key-0001".to_owned()),
            ),
        ];
        for (key, token) in FAKE_CORE_TOKENS {
            vars.push((key, Some(token.to_owned())));
        }
        for upstream in UPSTREAM_SECRETS {
            let value = if upstream == secret {
                value.map(str::to_owned)
            } else {
                None
            };
            vars.push((upstream, value));
        }
        let _env = EnvOverlay::apply(vars);
        super::build_state().await
    }

    /// The value `build_state` stored for one upstream secret: `None` only for the retention key,
    /// which is an `Option`, when it is unset.
    fn stored_upstream_secret(state: &super::AppState, name: &str) -> Option<String> {
        match name {
            "ANBUD_API_KEY" => Some(state.anbud_api_key.clone()),
            "MARKETING_CORE_SERVICE_KEY" => Some(state.marketing_core_service_key.clone()),
            "APPLICATION_CONVEX_SERVICE_KEY" => Some(state.application_convex_service_key.clone()),
            "ZAMMAD_API_TOKEN" => Some(state.zammad_api_token.clone()),
            "MAIL_TASK_INGEST_SERVICE_TOKEN" => Some(state.mail_task_ingest_service_token.clone()),
            "VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD" => {
                Some(state.mail_task_nats_password.clone())
            }
            "GATEWAY_RETENTION_DECISION_SERVICE_API_KEY" => {
                state.retention_decision_service_api_key.clone()
            }
            other => panic!("{other} is not one of the upstream secrets"),
        }
    }

    /// P2-bff-05-rel1: `build_state` applies the R-ing-04 rule to the seven upstream secrets it
    /// used to read raw. A `change-me*` placeholder, padded or in any case, refuses boot unless
    /// `ALLOW_INSECURE_DEV_DEFAULTS` is exactly `1`; at `1` it boots with the trimmed value. A real
    /// value is stored trimmed (`APPLICATION_CONVEX_SERVICE_KEY` was not), and an unset one stays
    /// allowed.
    #[tokio::test]
    async fn upstream_secrets_refuse_change_me_at_boot_unless_the_switch_is_exactly_one() {
        const REAL: &str = "  fixture-upstream-secret-0001  ";
        for secret in UPSTREAM_SECRETS {
            for placeholder in ["change-me", "  CHANGE-ME-upstream  "] {
                for insecure in [None, Some("0"), Some("true"), Some("yes")] {
                    let Err(error) = boot_with(secret, Some(placeholder), insecure).await else {
                        panic!("{secret} {placeholder:?} {insecure:?} must refuse to boot");
                    };
                    assert!(error.to_string().contains(secret), "{error:#}");
                }
                let state = boot_with(secret, Some(placeholder), Some("1"))
                    .await
                    .unwrap_or_else(|error| panic!("{secret} must boot at 1: {error:#}"));
                assert_eq!(
                    stored_upstream_secret(&state, secret).as_deref(),
                    Some(placeholder.trim()),
                    "{secret} keeps the trimmed placeholder at 1"
                );
            }
            let state = boot_with(secret, Some(REAL), None)
                .await
                .unwrap_or_else(|error| panic!("{secret} with a real value: {error:#}"));
            assert_eq!(
                stored_upstream_secret(&state, secret).as_deref(),
                Some("fixture-upstream-secret-0001"),
                "{secret} is stored trimmed"
            );
            let state = boot_with(secret, None, None)
                .await
                .unwrap_or_else(|error| panic!("{secret} unset: {error:#}"));
            assert!(
                stored_upstream_secret(&state, secret)
                    .unwrap_or_default()
                    .is_empty(),
                "{secret} unset stays empty"
            );
        }
    }

    /// WS14.S7: `VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE` is off unless it is exactly `true`, in every
    /// environment, development included, and is read into `SpaceFeatureFlags` rather than per request.
    #[test]
    fn crawl_robots_override_flag_is_default_off_in_every_environment() {
        assert!(!super::SpaceFeatureFlags::default().crawl_robots_override);
        let read = || super::SpaceFeatureFlags::from_env().crawl_robots_override;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(
                &[
                    ("APP_ENV", app_env),
                    ("VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE", None),
                ],
                || assert!(!read(), "APP_ENV={app_env:?}"),
            );
        }
        for value in ["", "1", "TRUE", "false", "on"] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("VEREVON_GATEWAY_CRAWL_ROBOTS_OVERRIDE", Some("true")),
        ];
        with_env(&vars, || assert!(read()));
    }

    /// `BROWSER_AI_RUN_ATTACH_SESSION` is off unless it is exactly `true`, in every environment, and is
    /// read into `SpaceFeatureFlags` once rather than per request.
    #[test]
    fn browser_ai_run_attach_session_flag_is_off_unless_exactly_true() {
        assert!(!super::SpaceFeatureFlags::default().browser_ai_run_attach_session);
        let read = || super::SpaceFeatureFlags::from_env().browser_ai_run_attach_session;
        for app_env in [None, Some("production"), Some("development")] {
            with_env(
                &[
                    ("APP_ENV", app_env),
                    ("BROWSER_AI_RUN_ATTACH_SESSION", None),
                ],
                || assert!(!read(), "APP_ENV={app_env:?}"),
            );
        }
        for value in ["", "1", "TRUE", "false", "on", "yes"] {
            let vars = [
                ("APP_ENV", Some("development")),
                ("BROWSER_AI_RUN_ATTACH_SESSION", Some(value)),
            ];
            with_env(&vars, || assert!(!read(), "{value:?}"));
        }
        let vars = [
            ("APP_ENV", Some("production")),
            ("BROWSER_AI_RUN_ATTACH_SESSION", Some("true")),
        ];
        with_env(&vars, || assert!(read()));
    }

    /// R-bff-01: the seven switches that the BFF used to read per request are read once, at startup, into
    /// `SpaceFeatureFlags`, and each is on only for the exact literal `true`. Unset and every other spelling,
    /// including `1`, `yes`, `on`, `TRUE`, `True`, a padded `true` and an empty value, is off, in production too.
    #[test]
    fn bff_switches_read_once_at_startup_are_on_only_for_the_exact_true() {
        /// The field of `SpaceFeatureFlags` that a switch's name is read into.
        fn field(flags: &super::SpaceFeatureFlags, name: &str) -> bool {
            match name {
                "ASSISTANT_NAVIGATION_ENABLED" => flags.assistant_navigation_enabled,
                "BROWSER_AI_RUN_REAL_GRANT" => flags.browser_ai_run_real_grant,
                "BROWSER_ASSISTANT_TASK_GRANTS" => flags.browser_assistant_task_grants,
                "BROWSER_DO_ENABLED" => flags.browser_do_enabled,
                "SANDBOX_THREAD_WORKSPACE_ENABLED" => flags.sandbox_thread_workspace_enabled,
                "SCHEDULE_CONTRACT_ENABLED" => flags.schedule_contract_enabled,
                "SKILL_MEMBER_AUTHORING" => flags.skill_member_authoring,
                other => panic!("{other} is not one of the seven switches"),
            }
        }
        for name in [
            "ASSISTANT_NAVIGATION_ENABLED",
            "BROWSER_AI_RUN_REAL_GRANT",
            "BROWSER_ASSISTANT_TASK_GRANTS",
            "BROWSER_DO_ENABLED",
            "SANDBOX_THREAD_WORKSPACE_ENABLED",
            "SCHEDULE_CONTRACT_ENABLED",
            "SKILL_MEMBER_AUTHORING",
        ] {
            assert!(
                super::BFF_EXACT_TRUE_SWITCHES.contains(&name)
                    || super::BROWSER_EXACT_TRUE_SWITCHES.contains(&name),
                "{name} is not in an exact-true table, so the startup report does not name it"
            );
            with_env(&[("APP_ENV", Some("production")), (name, None)], || {
                assert!(
                    !field(&super::SpaceFeatureFlags::from_env(), name),
                    "{name} unset"
                );
            });
            for value in ["", "1", "yes", "on", "TRUE", "True", " true", "true "] {
                let vars = [("APP_ENV", Some("production")), (name, Some(value))];
                with_env(&vars, || {
                    assert!(
                        !field(&super::SpaceFeatureFlags::from_env(), name),
                        "{name}={value:?}"
                    );
                });
            }
            let vars = [("APP_ENV", Some("production")), (name, Some("true"))];
            with_env(&vars, || {
                assert!(
                    field(&super::SpaceFeatureFlags::from_env(), name),
                    "{name}=true"
                );
            });
        }
    }

    /// R-bff-02: `SCIM_EDGE_ENABLED` is read with the exact literal reader. Only `true`, with a secure
    /// `ORG_CORE_SCIM_EDGE_TOKEN`, arms the edge. `1`, `yes`, `on`, `TRUE` and a padded `true` leave it off, and the
    /// startup report in `switch_spellings` names the first of them.
    #[test]
    fn scim_edge_is_on_only_for_the_exact_true_with_a_secure_credential() {
        let secure = "scim-edge-fixture-aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let armed = |flag: Option<&str>, token: Option<&str>| {
            let vars = [
                ("SCIM_EDGE_ENABLED", flag),
                ("ORG_CORE_SCIM_EDGE_TOKEN", token),
            ];
            with_env(&vars, || super::ScimEdgeConfig::from_env().enabled)
        };
        assert!(armed(Some("true"), Some(secure)));
        for flag in [
            None,
            Some(""),
            Some("1"),
            Some("yes"),
            Some("on"),
            Some("TRUE"),
            Some(" true"),
            Some("true "),
        ] {
            assert!(!armed(flag, Some(secure)), "{flag:?}");
        }
        // The exact `true` still needs a secure credential: an absent, short or placeholder one leaves it off.
        for token in [
            None,
            Some(""),
            Some("short"),
            Some("test-scim-edge-fixture-aaaaaaaaaaaaaaaaaaaaaa"),
        ] {
            assert!(!armed(Some("true"), token), "{token:?}");
        }
    }

    /// R-bff-02: the SCIM misconfiguration line is a startup line on stderr, not a `tracing` event that the
    /// default log level drops. It is written only when the switch is on and the credential is not secure.
    #[test]
    fn the_scim_misconfiguration_line_goes_to_stderr_only_when_it_applies() {
        let mut out = Vec::new();
        super::warn_scim_edge_misconfiguration(&mut out, true, false);
        assert_eq!(
            String::from_utf8(out).expect("the line is UTF-8"),
            "SCIM_EDGE_ENABLED is on but ORG_CORE_SCIM_EDGE_TOKEN is not a secure secret; the SCIM edge stays off\n",
        );
        for (flag, secure) in [(false, false), (false, true), (true, true)] {
            let mut out = Vec::new();
            super::warn_scim_edge_misconfiguration(&mut out, flag, secure);
            assert!(out.is_empty(), "flag={flag} secure={secure}");
        }
    }

    /// WS04-S9 / WS16-S6 / WS16-S8 / WS12-S3: the resolver, the locale hint, the sidebar and the DM
    /// flags default OFF, are OFF under `APP_ENV=production` when unset, ON in dev, and an
    /// explicit value wins.
    #[test]
    fn w2_gateway_flags_default_off_and_follow_their_variables() {
        let default = super::SpaceFeatureFlags::default();
        assert!(!default.directory_resolver_api);
        assert!(!default.sidebar);
        assert!(!default.chat_locale_hint);
        assert!(!default.dm_api);
        let vars = |env: &'static str, value: Option<&'static str>| {
            vec![
                ("APP_ENV", Some(env)),
                ("SPACE_DIRECTORY_RESOLVER_API", value),
                ("SPACE_SIDEBAR_API", value),
                ("CHAT_LOCALE_HINT", value),
                ("SPACE_DM_API", value),
            ]
        };
        let read = || {
            let flags = super::SpaceFeatureFlags::from_env();
            [flags.directory_resolver_api, flags.sidebar, flags.chat_locale_hint, flags.dm_api]
        };
        with_env(&vars("production", None), || assert_eq!(read(), [false; 4]));
        with_env(&vars("development", None), || assert_eq!(read(), [true; 4]));
        with_env(&vars("production", Some("true")), || assert_eq!(read(), [true; 4]));
        with_env(&vars("development", Some("false")), || assert_eq!(read(), [false; 4]));
    }

    /// WS10-S4 / WS13-S3 / WS14-S7 / WS15-S7 / WS15-S11: the approval-scope, message-search UI,
    /// entry-link and command-palette flags are default ON in the base compose, so they are ON in
    /// development when unset and OFF under `APP_ENV=production`, and an explicit value wins. The
    /// thread-fork, room-file, turn-attachment and system-entry switches default OFF in every
    /// environment; see `bff_default_off_space_switches_are_on_only_for_the_exact_literal_true`. The
    /// message-search API and timeline switches mirror Model Plane, so they are off in development
    /// too; see `model_plane_mirrored_space_flags_are_off_in_development_when_unset`.
    #[test]
    fn ws10_ws13_ws14_ws15_gateway_flags_default_off_and_follow_their_variables() {
        let default = super::SpaceFeatureFlags::default();
        assert!(!default.approval_scopes);
        assert!(!default.message_search_api);
        assert!(!default.message_search_ui);
        assert!(!default.entry_links_ui);
        assert!(!default.command_palette_ui);
        assert!(!default.thread_fork_api);
        assert!(!default.room_files_api);
        assert!(!default.room_files_ui);
        assert!(!default.turn_attachments_ui);
        assert!(!default.timeline_api);
        assert!(!default.system_entries_api);
        let keys = [
            "SPACE_APPROVAL_SCOPES_ENABLED",
            "SPACE_MESSAGE_SEARCH_UI",
            "SPACE_ENTRY_LINKS_UI",
            "SPACE_COMMAND_PALETTE_UI",
        ];
        let vars = |env: &'static str, value: Option<&'static str>| {
            let mut vars = vec![("APP_ENV", Some(env))];
            vars.extend(keys.iter().map(|key| (*key, value)));
            vars
        };
        let read = || {
            let flags = super::SpaceFeatureFlags::from_env();
            [
                flags.approval_scopes,
                flags.message_search_ui,
                flags.entry_links_ui,
                flags.command_palette_ui,
            ]
        };
        with_env(&vars("production", None), || assert_eq!(read(), [false; 4]));
        with_env(&vars("development", None), || assert_eq!(read(), [true; 4]));
        with_env(&vars("production", Some("true")), || {
            assert_eq!(read(), [true; 4])
        });
        with_env(&vars("development", Some("false")), || {
            assert_eq!(read(), [false; 4])
        });
    }

    /// WS07-S6: `CSP_REPORT_LOG_HOSTS` is a dev-only flag: it follows its variable outside
    /// production and stays off under `APP_ENV=production` or `prod` even when set to true.
    #[test]
    fn csp_report_log_hosts_is_a_dev_only_flag() {
        with_env(&[("APP_ENV", Some("production")), ("CSP_REPORT_LOG_HOSTS", Some("true"))], || {
            assert!(!super::SpaceFeatureFlags::from_env().csp_report_log_hosts);
        });
        with_env(&[("APP_ENV", Some("prod")), ("CSP_REPORT_LOG_HOSTS", Some("true"))], || {
            assert!(!super::SpaceFeatureFlags::from_env().csp_report_log_hosts);
        });
        with_env(&[("APP_ENV", Some("development")), ("CSP_REPORT_LOG_HOSTS", Some("true"))], || {
            assert!(super::SpaceFeatureFlags::from_env().csp_report_log_hosts);
        });
        // Unset is off in every environment: it is not a `space_flag` (no dev default of on).
        with_env(&[("APP_ENV", Some("development")), ("CSP_REPORT_LOG_HOSTS", None)], || {
            assert!(!super::SpaceFeatureFlags::from_env().csp_report_log_hosts);
        });
    }

    /// WS01-S5 and WS03-S4: the thread-pane, send-idempotency, connectivity-banner, policy-admin and
    /// realtime-stream flags are off in production when their variable is unset, on in development,
    /// and honour an explicit value. The human-post switch mirrors Model Plane, so it is off in
    /// development too; see `model_plane_mirrored_space_flags_are_off_in_development_when_unset`.
    #[test]
    fn ws01_and_ws03_flags_follow_the_space_flag_rule() {
        let keys = [
            "SPACE_THREAD_PANE_UI",
            "SPACE_SEND_IDEMPOTENCY_API",
            "SPACE_CONNECTIVITY_BANNER_UI",
            "SPACE_POLICY_ADMIN_API",
            "SPACE_REALTIME_STREAM",
        ];
        let unset = |app_env: &'static str| -> Vec<(&'static str, Option<&'static str>)> {
            let mut vars = vec![("APP_ENV", Some(app_env))];
            vars.extend(keys.iter().map(|key| (*key, None)));
            vars
        };
        with_env(&unset("production"), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(!flags.thread_pane_ui);
            assert!(!flags.send_idempotency_api);
            assert!(!flags.connectivity_banner_ui);
            assert!(!flags.policy_admin_api);
            assert!(!flags.realtime_stream);
        });
        with_env(&unset("development"), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(flags.thread_pane_ui);
            assert!(flags.send_idempotency_api);
            assert!(flags.connectivity_banner_ui);
            assert!(flags.policy_admin_api);
            assert!(flags.realtime_stream);
        });
        let mut explicit = unset("production");
        explicit.retain(|(key, _)| *key != "SPACE_POLICY_ADMIN_API");
        explicit.push(("SPACE_POLICY_ADMIN_API", Some("true")));
        with_env(&explicit, || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(flags.policy_admin_api, "explicit true wins in production");
            assert!(!flags.thread_pane_ui, "the others stay off");
        });
        let mut explicit_off = unset("development");
        explicit_off.retain(|(key, _)| *key != "SPACE_POLICY_ADMIN_API");
        explicit_off.push(("SPACE_POLICY_ADMIN_API", Some("false")));
        with_env(&explicit_off, || {
            assert!(!super::SpaceFeatureFlags::from_env().policy_admin_api);
        });
    }

    /// WS03-S5, WS04-S2, WS04-S3 and WS04-S6: the typed-denial, room-details, people-search and
    /// membership-writer flags are off in production when unset, on in development, and honour an
    /// explicit value in both.
    #[test]
    fn ws03_s5_and_ws04_flags_follow_the_space_flag_rule() {
        let keys = [
            "SPACE_DENIALS_TYPED",
            "SPACE_ROOM_DETAILS_UI",
            "SPACE_PEOPLE_SEARCH_API",
            "SPACE_MEMBERSHIP_WRITER_API",
            "SPACE_ROOM_ARCHIVE_API",
        ];
        let vars = |app_env: &'static str, value: Option<&'static str>| {
            let mut vars = vec![("APP_ENV", Some(app_env))];
            vars.extend(keys.iter().map(|key| (*key, value)));
            vars
        };
        with_env(&vars("production", None), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(!flags.denials_typed);
            assert!(!flags.room_details_ui);
            assert!(!flags.people_search_api);
            assert!(!flags.membership_writer_api);
            assert!(!flags.room_archive_api);
        });
        with_env(&vars("development", None), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(flags.denials_typed);
            assert!(flags.room_details_ui);
            assert!(flags.people_search_api);
            assert!(flags.membership_writer_api);
            assert!(flags.room_archive_api);
        });
        with_env(&vars("production", Some("true")), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(flags.denials_typed);
            assert!(flags.room_details_ui);
            assert!(flags.people_search_api);
            assert!(flags.membership_writer_api);
            assert!(flags.room_archive_api);
        });
        with_env(&vars("development", Some("false")), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(!flags.denials_typed);
            assert!(!flags.room_details_ui);
            assert!(!flags.people_search_api);
            assert!(!flags.membership_writer_api);
            assert!(!flags.room_archive_api);
        });
    }

    /// WS04-S8.gw / WS08-S4: the room-directory, mention-record and addressed-wake flags default
    /// OFF, are OFF under `APP_ENV=production` when unset, ON in dev, and an explicit value wins.
    #[test]
    fn ws04_ws08_room_directory_and_mention_flags_default_off_in_production_and_on_in_dev() {
        let default = super::SpaceFeatureFlags::default();
        assert!(!default.room_directory_api);
        assert!(!default.mention_records_api);
        assert!(!default.addressed_wake_api);
        let cases: [(&str, fn(&super::SpaceFeatureFlags) -> bool); 3] = [
            ("SPACE_ROOM_DIRECTORY_API", |flags| flags.room_directory_api),
            ("SPACE_MENTION_RECORDS_API", |flags| flags.mention_records_api),
            ("SPACE_ADDRESSED_WAKE_API", |flags| flags.addressed_wake_api),
        ];
        for (key, read) in cases {
            let vars = |env: &'static str, value: Option<&'static str>| {
                vec![("APP_ENV", Some(env)), (key, value)]
            };
            let flag = || read(&super::SpaceFeatureFlags::from_env());
            with_env(&vars("production", None), || assert!(!flag(), "{key} production default"));
            with_env(&vars("prod", None), || assert!(!flag(), "{key} prod default"));
            with_env(&vars("development", None), || assert!(flag(), "{key} dev default"));
            with_env(&vars("production", Some("true")), || assert!(flag(), "{key} explicit on"));
            with_env(&vars("development", Some("false")), || assert!(!flag(), "{key} explicit off wins in dev"));
            with_env(&vars("development", Some("0")), || assert!(!flag(), "{key} 0 is off"));
        }
    }
}

#[cfg(test)]
mod human_browse_flag_tests {
    use super::{env_exact_true, exact_true_flag, BROWSER_EXACT_TRUE_SWITCHES};

    /// Runs `body` with `key` set to `value` (or unset), then restores it, on the shared env lock.
    fn with_var<T>(key: &str, value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let _guard = super::TEST_ENV_LOCK.blocking_lock();
        let saved = std::env::var_os(key);
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        let out = body();
        match saved {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        out
    }

    /// The browser switches that `build_state` reads from the environment read only the exact literal
    /// `true` (V3e-bff-3). Unset is off everywhere; `1`, `yes`, `on`, `TRUE`, `True`, a padded `true`
    /// and an empty value are off too. The flag is never read against `APP_ENV`, so production is the same.
    #[test]
    fn browser_switches_read_from_the_environment_are_on_only_for_the_exact_true() {
        for key in BROWSER_EXACT_TRUE_SWITCHES {
            assert!(!with_var(key, None, || env_exact_true(key)), "{key} unset");
            assert!(
                with_var(key, Some("true"), || env_exact_true(key)),
                "{key}=true"
            );
            for value in ["", "   ", "0", "1", "yes", "on", "TRUE", "True", " true", "true "] {
                assert!(
                    !with_var(key, Some(value), || env_exact_true(key)),
                    "{key}={value:?}"
                );
            }
        }
    }

    /// BROWSER_HUMAN_BROWSE and BROWSER_RECIPES read only the literal `true` (V3d-bff-3): the padded,
    /// cased and other-word spellings that the retired lenient readers took are off here.
    #[test]
    fn exact_true_browser_flags_read_only_the_literal_true() {
        assert!(exact_true_flag(Some("true")));
        for value in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some("yes"),
            Some("on"),
            Some(" true"),
            Some("true "),
        ] {
            assert!(!exact_true_flag(value), "{value:?}");
        }
    }
}

#[cfg(test)]
#[path = "config_zdr_switch_tests.rs"]
mod zdr_switch_tests;
