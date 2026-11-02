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
    /// `human_browse` grant. Off unless set to a truthy value, in every environment.
    pub(crate) browser_human_browse: bool,
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
    /// WS10-S5: whether the org model-entitlement admin routes (`/api/v1/orgs/{id}/model-entitlement`
    /// and its `explain`), the `org.set_model_entitlement` action's write and the ZDR switch
    /// gate's `hidden_count` read are live. Read once from `MODEL_ENTITLEMENT_ADMIN_ENABLED`
    /// (exactly `true`, default off) in `build_state`, never in a handler. Off, GET answers
    /// `{"enabled": false}` so the SPA hides the section, every write and explain is a 404, and
    /// the ZDR gate answers as it did before the policy existed.
    pub(crate) model_entitlement_admin_enabled: bool,
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
    /// `CHAT_LOCALE_HINT` (WS16-S6): stamp the viewer's interface language (`nb-NO` or `en-US`) on
    /// the model-bound body as the request-scoped `locale` field. Off removes the field, whatever
    /// the browser sent. A rollout control, never an authorization input: the field is a framing
    /// hint, is never stored, and grants nothing.
    pub(crate) chat_locale_hint: bool,
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
    /// Convex `SPACE_REACTIONS` on and model-gateway `SPACE_MESSAGE_REVISE_API` (target route). Production default off.
    pub(crate) reactions_api: bool,
    /// `CLIENT_VERSION_POLICY` / `CLIENT_MIN_CONTRACT` (WS07-S3): the stale-client contract the
    /// public `/api/v1/client-config` route reports. Unlike the booleans its default is `banner`,
    /// never off. Carried here (not as an `AppState` field) so no test literal needs an edit.
    pub(crate) client_policy: crate::client_contract::ClientPolicy,
    /// `CSP_REPORT_LOG_HOSTS` (WS07-S6): log the blocked ORIGIN (scheme and host, never a path
    /// or query) of each CSP report, for a host inventory. Forced off when `APP_ENV` is
    /// production (see `dev_only_flag`); the counters never carry a URL either way.
    pub(crate) csp_report_log_hosts: bool,
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
    /// control, never an authorization input: org-core resolves the token and the organization.
    pub(crate) scim_edge: ScimEdgeConfig,
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
    pub(crate) fn from_env() -> Self {
        let flag = env_bool("SCIM_EDGE_ENABLED", false);
        let token = env::var("ORG_CORE_SCIM_EDGE_TOKEN").unwrap_or_default();
        let token = token.trim();
        let secure = service_token_is_secure(token);
        if flag && !secure {
            tracing::warn!("SCIM_EDGE_ENABLED is on but ORG_CORE_SCIM_EDGE_TOKEN is not a secure secret; the SCIM edge stays off");
        }
        Self {
            enabled: flag && secure,
            token: if secure { token.to_owned() } else { String::new() },
        }
    }
}

impl SpaceFeatureFlags {
    pub(crate) fn from_env() -> Self {
        Self {
            agent_wake_policy_api: space_flag("SPACE_AGENT_WAKE_POLICY_API"),
            addressed_wake_api: space_flag("SPACE_ADDRESSED_WAKE_API"),
            mention_records_api: space_flag("SPACE_MENTION_RECORDS_API"),
            approval_scopes: space_flag("SPACE_APPROVAL_SCOPES_ENABLED"),
            append_retrieval: env::var("GATEWAY_SPACE_APPEND_RETRIEVAL").is_ok_and(|value| value == "true"),
            chat_locale_hint: space_flag("CHAT_LOCALE_HINT"),
            denials_typed: space_flag("SPACE_DENIALS_TYPED"),
            dm_api: space_flag("SPACE_DM_API"),
            message_revise_api: space_flag("SPACE_MESSAGE_REVISE_API"),
            reactions_api: space_flag("SPACE_REACTIONS_API"),
            client_policy: crate::client_contract::ClientPolicy::from_env(),
            csp_report_log_hosts: dev_only_flag("CSP_REPORT_LOG_HOSTS"),
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
        }
    }
}

/// One Space flag: `1|true|yes|on` turns it on; unset falls back to ON in development and OFF
/// when `APP_ENV` is production. Same rule as every other dev-defaulted gateway flag.
#[allow(dead_code, reason = "first consumer is the next Space slice that adds a flag")]
pub(crate) fn space_flag(key: &str) -> bool {
    env_bool(
        key,
        dev_flags_allowed(&env::var("APP_ENV").unwrap_or_default()),
    )
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
        browser_human_browse: human_browse_enabled(env::var("BROWSER_HUMAN_BROWSE").ok().as_deref()),
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
        anbud_api_key: env::var("ANBUD_API_KEY").unwrap_or_default().trim().to_owned(),
        anbud_api_key_header: env::var("ANBUD_API_KEY_HEADER")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "X-API-Key".to_owned()),
        marketing_core_url: env_url("MARKETING_CORE_URL", "http://marketing-core:3165"),
        marketing_core_service_key: env::var("MARKETING_CORE_SERVICE_KEY").unwrap_or_default().trim().to_owned(),
        marketing_core_organization_id: env::var("MARKETING_CORE_ORGANIZATION_ID").unwrap_or_default().trim().to_owned(),
        // shipping-core (Ingestion Plane) — freight aggregator; on the
        // inter-plane bus, so the in-network name resolves from the gateway.
        shipping_core_url: env_url("SHIPPING_CORE_URL", "http://shipping-core:8080"),
        user_core_url: env_url("USER_CORE_URL", "http://user-core:3012"),
        application_convex_url: std::env::var("APPLICATION_CONVEX_URL").unwrap_or_default(),
        application_convex_service_key: std::env::var("APPLICATION_CONVEX_SERVICE_KEY")
            .unwrap_or_default(),
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
        autocomplete_token: env::var("AUTOCOMPLETE_INTERNAL_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_owned(),
        zammad_api_url: env_url("ZAMMAD_API_URL", "http://zammad-railsserver:3000"),
        remote_support_rendezvous_url: optional_env("REMOTE_SUPPORT_RENDEZVOUS_URL"),
        remote_support_relay_url: optional_env("REMOTE_SUPPORT_RELAY_URL"),
        remote_support_server_public_key: optional_env("REMOTE_SUPPORT_SERVER_PUBLIC_KEY"),
        zammad_api_token: env::var("ZAMMAD_API_TOKEN")
            .unwrap_or_default()
            .trim()
            .to_owned(),
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
        model_entitlement_admin_enabled: model_entitlement_admin_enabled(),
        mail_task_ingest_service_token: optional_env("MAIL_TASK_INGEST_SERVICE_TOKEN"),
        mail_task_nats_url: optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_URL"),
        mail_task_nats_user: optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_USER"),
        mail_task_nats_password: optional_env("VEREVON_GATEWAY_MAIL_TASK_NATS_PASSWORD"),
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
            if !trimmed.is_empty() && env_bool("ALLOW_INSECURE_DEV_DEFAULTS", false) {
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
            if cfg!(debug_assertions) || env_bool("ALLOW_INSECURE_DEV_DEFAULTS", false) {
                return Ok("change-me".into());
            }
            anyhow::bail!("INTERNAL_API_KEY must be set and cannot be a change-me* placeholder");
        }
    }
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
/// over the flag.
fn dev_only_flag(key: &str) -> bool {
    let enabled = env_bool(key, false);
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

/// Parses `BROWSER_HUMAN_BROWSE` (WS8.D). Unset, blank or anything but a truthy word is off;
/// the flag is never read against `APP_ENV`, so every environment defaults to off.
pub(crate) fn human_browse_enabled(raw: Option<&str>) -> bool {
    raw.map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

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
        canonical_public_origin, continue_park_form_from, dev_flags_allowed,
        model_entitlement_admin_from, service_token_is_secure, standard_http_client,
        validate_service_token_distinctness,
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

    /// WS01-S5 and WS03-S4: the human-post, thread-pane, send-idempotency, connectivity-banner and
    /// policy-admin flags are off in production when their variable is unset, on in development,
    /// and honour an explicit value.
    #[test]
    fn ws01_and_ws03_flags_follow_the_space_flag_rule() {
        let keys = [
            "SPACE_HUMAN_POSTS_API",
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
            assert!(!flags.human_posts_api);
            assert!(!flags.thread_pane_ui);
            assert!(!flags.send_idempotency_api);
            assert!(!flags.connectivity_banner_ui);
            assert!(!flags.policy_admin_api);
            assert!(!flags.realtime_stream);
        });
        with_env(&unset("development"), || {
            let flags = super::SpaceFeatureFlags::from_env();
            assert!(flags.human_posts_api);
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
            assert!(!flags.human_posts_api, "the others stay off");
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
}

#[cfg(test)]
mod human_browse_flag_tests {
    use super::human_browse_enabled;

    #[test]
    fn human_browse_flag_defaults_off_in_every_environment() {
        // The flag is read without APP_ENV: unset is off everywhere, production included.
        assert!(!human_browse_enabled(None));
        assert!(!human_browse_enabled(Some("")));
        assert!(!human_browse_enabled(Some("   ")));
        assert!(!human_browse_enabled(Some("0")));
        assert!(!human_browse_enabled(Some("off")));
        assert!(!human_browse_enabled(Some("garbage")));
        assert!(human_browse_enabled(Some("1")));
        assert!(human_browse_enabled(Some(" TRUE ")));
        assert!(human_browse_enabled(Some("yes")));
    }
}
