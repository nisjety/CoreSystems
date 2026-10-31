//! What Zero Data Retention does to each part of the Quarry surface, today.
//!
//! The table [`ROWS`] is the source of truth. `docs/ZDR_MATRIX.md` and
//! `crates/quarry-runtime/contracts/zdr-feature-matrix.json` are generated from
//! it ([`to_markdown`], [`to_json`]) and a test fails when either differs, so the
//! published description cannot be hand-edited away from the code. A test in
//! `quarry-edge` (`tests/zdr_feature_matrix.rs`) scans the router for every
//! `.route(...)` and fails when a route has no row, or a row names a route that
//! does not exist, so a new endpoint cannot ship without a ZDR classification.
//!
//! This documents behaviour; it changes none. A row whose [`Status`] is
//! [`Status::Gap`] records a place where ZDR is **not** honoured today, with the
//! work that closes it, rather than hiding it.
//!
//! Adopted defaults: an existing refusal is never loosened (a test pins the rows
//! that must stay refused), durable resources are to be refused, and nothing is
//! "hash only": a hash of a known URL is a membership oracle.

use serde_json::{json, Value};

/// What ZDR does to a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    /// The feature runs and nothing about the request is retained.
    Honored,
    /// The result is returned in the response only; nothing is stored.
    InlineOnly,
    /// The feature runs with less than it has outside ZDR.
    Degraded { warning: &'static str },
    /// The request is refused (a 4xx with a message naming ZDR).
    Refused {
        code: &'static str,
        feature: &'static str,
    },
    /// ZDR does not apply: the route stores nothing of the caller's content, or
    /// is not reachable with a ZDR request.
    Inert { reason: &'static str },
}

/// Whether the row describes what the code does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Enforced,
    /// ZDR is not honoured here today; `issue` names what closes it.
    Gap { issue: &'static str },
}

/// How the row's log behaviour is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Log {
    /// A test captures the process log for a ZDR request on this feature.
    Canary { test: &'static str },
    /// No test looks at the log for this feature yet.
    Unverified,
}

#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub id: &'static str,
    /// The routes this row classifies (`{param}` spelling). A row whose route is
    /// shared with another row lists none, and says so in `basis`.
    pub routes: &'static [&'static str],
    pub treatment: Treatment,
    pub status: Status,
    /// Tests that fail if the treatment stops being true, as `file::test_name`
    /// (a file under `tests/`, or a module path for a unit test).
    pub enforced_by: &'static [&'static str],
    /// Why the treatment is what it is: the code that makes it true.
    pub basis: &'static str,
    pub log: Log,
}

pub const ROWS: &[Row] = &[
    Row {
        id: "scrape",
        routes: &["/v1/scrape"],
        treatment: Treatment::InlineOnly,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::scrape_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service",
            "zdr_log_capture::scrape_meter_metadata_under_zdr_has_no_url_or_fingerprint",
            "routes::tests::a_zdr_scrape_has_no_scrape_id_and_stores_no_artifact",
        ],
        basis: "no artifact, page-cache entry, scrape record or durable event; the text comes back in the response (default format markdown), cut at max_inline_bytes. The AI formats (json, summary, question, highlights; INNH-1, dark behind QUARRY_EDGE__AI_FORMATS) are refused under ZDR (ADR-4): each is answered status skipped, code ZDR_UNAVAILABLE inside the 200, with no Model Plane call, so no page text leaves for a model; QUARRY_EDGE__AI_FORMATS_ZDR accepts only refuse in v1 and the engine refuses a ZDR input as well",
        log: Log::Canary {
            test: "zdr_log_capture::a_zdr_scrape_logs_no_url_and_no_title_even_at_debug",
        },
    },
    Row {
        id: "scrape_stream",
        routes: &["/v1/scrape/stream"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::scrape_stream_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service",
        ],
        basis: "the run's posture is the request flag raised by the token floor; page events go to live subscribers only. The AI formats are not served on the stream at all (400 unsupported_in_stream, INNH-1), so no model call can happen on this route",
        log: Log::Canary {
            test: "zdr_log_capture::a_zdr_streamed_scrape_logs_no_url_from_its_spawned_runner",
        },
    },
    Row {
        id: "scrape_record_read",
        routes: &["/v1/scrape/{id}"],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "scrape_record_read",
        },
        status: Status::Enforced,
        enforced_by: &["routes::tests::a_zdr_floored_reader_is_refused_even_for_an_earlier_record"],
        basis: "scrape_routes::get_scrape refuses a reader whose organization is under ZDR, even for a record written before it was",
        log: Log::Unverified,
    },
    Row {
        id: "durable_jobs",
        routes: &["/v1/crawl", "/v1/batch"],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "durable_job",
        },
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::durable_crawl_and_batch_are_refused_for_a_zdr_org_user_and_open_otherwise",
            "zdr_gap_rows::enforced_refusals_hold_for_durable_jobs",
        ],
        basis: "routes::refuse_durable_job_under_zdr: the job record and per-page results would be retained",
        log: Log::Unverified,
    },
    Row {
        id: "search",
        routes: &["/v1/search"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::a_user_token_from_a_zdr_org_runs_search_as_zdr_even_when_the_body_says_false",
            "zdr_claim_floor::a_user_token_from_a_zdr_org_runs_search_as_zdr_when_the_body_omits_the_flag",
        ],
        basis: "paid providers closed, search cache bypassed, no durable SearchIssued event; the provider failure is logged as a class only",
        log: Log::Canary {
            test: "zdr_log_capture::a_zdr_search_with_a_failing_provider_logs_no_query",
        },
    },
    Row {
        id: "search_verticals",
        routes: &["/v1/search/images", "/v1/search/videos"],
        treatment: Treatment::Inert {
            reason: "a provider call and a usage count only; the query is not cached, logged or stored",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "search_routes::images and ::videos touch no store; meter_vertical_search records user, kind and result count, not the query",
        log: Log::Unverified,
    },
    Row {
        id: "search_similar",
        routes: &["/v1/search/similar"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[],
        basis: "the page fetches run with the token's ZDR floor (effective_zdr_mode); the request has no zdr field of its own",
        log: Log::Unverified,
    },
    Row {
        id: "search_suggest",
        routes: &["/v1/search/suggest"],
        treatment: Treatment::Degraded {
            warning: "the partial query is sent to the Model Plane without the ZDR posture",
        },
        status: Status::Gap {
            issue: "unassigned: forward the posture, or refuse under ZDR",
        },
        enforced_by: &["zdr_gap_rows::gap_rows_currently_violate_search_suggest"],
        basis: "search_routes::suggest calls the Model Plane and never reads the token's ZDR floor",
        log: Log::Unverified,
    },
    Row {
        id: "answer",
        routes: &["/v1/answer", "/v1/answer/stream"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::answer_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise",
            "zdr_claim_floor::answer_stream_runs_as_zdr_for_a_zdr_org_user_and_as_before_otherwise",
        ],
        basis: "the answer pipeline runs with the effective ZDR posture: no paid providers, no durable search event",
        log: Log::Unverified,
    },
    Row {
        id: "extract",
        routes: &["/v1/extract"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::extract_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service",
        ],
        basis: "the page runs carry the effective ZDR posture, so no artifact or durable page event is written",
        log: Log::Unverified,
    },
    Row {
        id: "map",
        routes: &["/v1/map"],
        treatment: Treatment::Inert {
            reason: "reads robots and sitemaps and returns links; nothing is stored",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "map_routes::map touches no cache, artifact store, index or event sink",
        log: Log::Unverified,
    },
    Row {
        id: "audio",
        routes: &["/v1/audio"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::audio_forwards_the_floored_posture_to_the_model_plane",
            "zdr_claim_floor::audio_transcription_forwards_the_floored_posture_to_the_model_plane",
        ],
        basis: "audio_routes::raise_to_zdr_floor forwards the posture to the Model Plane, which owns retention for audio",
        log: Log::Unverified,
    },
    Row {
        id: "artifacts",
        routes: &["/v1/artifacts", "/v1/artifacts/{id}"],
        treatment: Treatment::Inert {
            reason: "reads artifacts that already exist; a ZDR run writes none",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "resource_routes list and get are tenant-bound reads of the artifact store",
        log: Log::Unverified,
    },
    Row {
        id: "control_records",
        routes: &[
            "/v1/sources",
            "/v1/sources/{id}",
            "/v1/snapshots",
            "/v1/{kind}/jobs",
            "/v1/request-queues",
            "/v1/benchmarks",
            "/v1/team/credit-usage",
            "/v1/team/token-usage",
            "/v1/team/concurrency",
            "/v1/team/queue-status",
            "/v1/team/activity",
            "/v1/runs/{id}/events",
            "/v1/jobs/{id}/events",
            "/v1/crawl/{id}/events",
            "/v1/batch/{id}/events",
        ],
        treatment: Treatment::Inert {
            reason: "reads or proxies Control Plane records; Quarry stores nothing for the caller",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "resource_routes forwards to Control Plane with the verified identity and keeps no copy (the crawl and batch event tails are the same handler as the job event tail, scoped to one kind; a ZDR organization can only hold jobs that pre-date its ZDR, and reads them as before); EXCEPT POST /v1/sources with monitor:true, which registers a recurring change-monitor schedule in Control Plane and is classified by the schedules row (it is refused or observed under the same flag and stamps the posture on the schedule)",
        log: Log::Unverified,
    },
    Row {
        id: "job_lifecycle_reads",
        routes: &[
            "/v1/crawl/ongoing",
            "/v1/crawl/{id}",
            "/v1/batch/ongoing",
            "/v1/batch/{id}",
            "/v1/jobs/{id}",
        ],
        treatment: Treatment::Inert {
            reason: "reads Control Plane's job record (status, phase, counts, closed-vocabulary warnings); Quarry stores nothing and the view names no page URL",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "job_lifecycle_routes get_job, get_crawl, get_batch and ongoing_* are org-scoped reads through control_client; the page URLs of a job are on the errors routes (job_errors row) and the event tails (control_records row). DELETE /v1/crawl/{id} and DELETE /v1/batch/{id} share these paths and are the cancel verb of the job_control row",
        log: Log::Unverified,
    },
    Row {
        id: "job_errors",
        routes: &[
            "/v1/crawl/{id}/errors",
            "/v1/batch/{id}/errors",
            "/v1/jobs/{id}/errors",
        ],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "job_error_urls",
        },
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::job_errors_are_refused_for_a_zdr_org_user_and_open_to_a_standard_org",
            "job_lifecycle::zdr_caller_can_cancel_but_not_read_errors",
        ],
        basis: "job_lifecycle_routes::errors_for refuses a caller whose token floor is ZDR before it asks Control Plane anything: the error list names the URL of every page that failed. Unconditional, not behind the durable-resource flag",
        log: Log::Unverified,
    },
    Row {
        id: "job_control",
        routes: &[
            "/v1/crawl/{id}/pause",
            "/v1/crawl/{id}/resume",
            "/v1/batch/{id}/pause",
            "/v1/batch/{id}/resume",
        ],
        treatment: Treatment::Inert {
            reason: "a control verb carries an action word and the actor id, no page content; a ZDR organization can only hold jobs that pre-date its ZDR, and cancelling one is wind-down",
        },
        status: Status::Enforced,
        enforced_by: &["job_lifecycle::zdr_caller_can_cancel_but_not_read_errors"],
        basis: "job_lifecycle_routes::control_request writes {action, actor, actor_type} to Control Plane and answers the job record; dark behind QUARRY_EDGE__JOB_CONTROL (501 off); a service principal is refused 403. Cancel is DELETE on the crawl or batch path shared with the lifecycle read",
        log: Log::Unverified,
    },
    Row {
        id: "change_history",
        routes: &["/v1/change/check", "/v1/change/latest", "/v1/change/history"],
        treatment: Treatment::Degraded {
            warning: "a change check keeps a baseline and history for the URL",
        },
        status: Status::Gap {
            issue: "WS4.H: refuse under ZDR, observe first",
        },
        enforced_by: &["zdr_gap_rows::gap_rows_currently_violate_change_history"],
        basis: "change_routes::check calls zdr_refusal::refuse behind QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES (default off: observe, zdr.would_refuse, served as before); latest and history are reads and stay open",
        log: Log::Unverified,
    },
    Row {
        id: "change_snapshot",
        routes: &["/v1/change/snapshot", "/v1/change/refresh"],
        treatment: Treatment::Degraded {
            warning: "promoting a snapshot and scheduling a refresh both retain the page",
        },
        status: Status::Gap {
            issue: "WS4.H: refuse under ZDR, observe first",
        },
        enforced_by: &["zdr_gap_rows::gap_rows_currently_violate_change_snapshot"],
        basis: "change_routes::snapshot and ::refresh call zdr_refusal::refuse behind QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES (default off: observe, zdr.would_refuse, served as before)",
        log: Log::Unverified,
    },
    Row {
        id: "change_record_internal",
        routes: &["/v1/internal/change/record"],
        treatment: Treatment::Degraded {
            warning: "the change event and webhook are suppressed; the baseline is still stored",
        },
        status: Status::Gap {
            issue: "WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, a body that declares zdr is refused before the baseline is stored; the flag default stays off until ADR-4 sign-off and the customer notice",
        },
        // No test here fails if the baseline write stops being true: this binary wires
        // no baseline store, so the write is unreachable (see the test's own doc).
        enforced_by: &[],
        basis: "record_internal calls zdr_refusal::refuse on the body's declared zdr before the store (flag on: 403; flag off: zdr.would_refuse and the baseline is stored as before); emit_change_side_effects suppresses the change event and webhook for a declared zdr in both modes. The orchestrator declares the posture quarry-control stamped on the schedule at create (migration 023); a schedule created before that (NULL) or before its org turned ZDR on declares nothing or false and is stored (pinned by zdr_posture_stamp::record_trusts_the_stamp_not_the_current_posture); the orchestrator also declares the stamp on the run_page fetch that precedes the record call, so a stamped-ZDR monitor's fetch runs in ZDR mode and persists no page artifact, event or local-index write-back; ChangeMonitorWF still emits run_started and change_detected with the URL. No test observes the baseline write, because the harness wires no baseline store",
        log: Log::Unverified,
    },
    Row {
        id: "schedules",
        routes: &[
            "/v1/schedules",
            "/v1/schedules/{id}",
            "/v1/schedules/{id}/pause",
            "/v1/schedules/{id}/unpause",
            "/v1/schedules/{id}/trigger",
            "/v1/schedules/{id}/backfill",
        ],
        treatment: Treatment::Degraded {
            warning: "a schedule is a durable resource; a ZDR organization can create and run one",
        },
        status: Status::Gap {
            issue: "WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, create, trigger, backfill and unpause are refused and list, get, pause and delete stay open so an organization can wind down; the flag default stays off until ADR-4 sign-off and the customer notice",
        },
        enforced_by: &["zdr_gap_rows::gap_rows_currently_violate_schedules"],
        basis: "schedule_routes call zdr_refusal::refuse on create, unpause, trigger and backfill (default off: observe, zdr.would_refuse, forwarded as before); POST /v1/sources with monitor:true is covered by this row; the caller's posture is stamped on the change-monitor schedule at create (quarry-control migration 023) from the verified claims, raised (never lowered) by a zdr:true declared in the body. Known gaps: an org that turns ZDR on later keeps its stamp, the legacy non-preset create is not stamped",
        log: Log::Unverified,
    },
    Row {
        id: "profiles",
        routes: &[
            "/v1/profiles",
            "/v1/profiles/create",
            "/v1/profiles/{id}",
            "/v1/profiles/{id}/restore_probe",
        ],
        treatment: Treatment::Degraded {
            warning: "a browser profile (cookies, storage) is a durable resource a ZDR organization can create",
        },
        status: Status::Gap {
            issue: "WS4.H stage B: with QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES on, create, save, update, read and restore_probe are refused and only list and delete stay open so profiles can be erased; the flag default stays off until ADR-4 sign-off and the customer notice",
        },
        enforced_by: &["zdr_gap_rows::gap_rows_currently_violate_profiles"],
        basis: "profile_routes call profile_guard::precheck, whose ZDR half is zdr_refusal::refuse, on create, save, update, read and restore_probe (default off: observe, zdr.would_refuse, served as before); list and delete are never refused. precheck also refuses a service principal on every operation and a save without the quarry:profiles:import scope (WS17.P0), before the ZDR decision, whatever the flag; the agent start refuses a persistent profile in both modes",
        log: Log::Unverified,
    },
    Row {
        id: "agent_run",
        routes: &["/v1/agent/runs"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &["agent_routes::tests::zdr_expiry_writes_no_timeline_row_and_no_checkpoint"],
        basis: "a ZDR run writes no checkpoint, timeline row or receipt and logs under a ZDR span",
        log: Log::Canary {
            test: "zdr_log_capture::a_zdr_agent_start_logs_no_url",
        },
    },
    Row {
        id: "agent_persistent_profile",
        routes: &[],
        treatment: Treatment::Refused {
            code: "BAD_REQUEST",
            feature: "persistent_profile",
        },
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::a_zdr_org_users_agent_run_cannot_take_a_persistent_profile",
            "zdr_gap_rows::a_service_principal_with_body_zdr_gets_the_same_persistent_profile_refusal",
        ],
        basis: "same route as agent_run: lifecycle::start_run refuses persist_profile or a profile id under ZDR",
        log: Log::Unverified,
    },
    Row {
        id: "agent_resume",
        routes: &[],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "resume",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "same route as agent_run: lifecycle::start_run refuses a resume of a ZDR checkpoint (none is written)",
        log: Log::Unverified,
    },
    Row {
        id: "agent_attach",
        routes: &[
            "/v1/agent/runs/{run_id}/attach",
            "/v1/agent/runs/{run_id}/attach/{attach_id}",
        ],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "agent_attach",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "attach::attach_run refuses a ZDR run (reason attach_zdr_refused) before it reads the grant or opens a tab, in both modes; the route is also dark behind QUARRY_EDGE__ATTACH. The release route has nothing to release for a ZDR run. No test drives a ZDR run through the attach route yet: the refusal is a three-line guard in the handler",
        log: Log::Unverified,
    },
    Row {
        id: "agent_run_surface",
        routes: &[
            "/v1/agent/runs/{run_id}",
            "/v1/agent/runs/{run_id}/step",
            "/v1/agent/runs/{run_id}/interact",
            "/v1/agent/runs/{run_id}/browser-session",
            "/v1/agent/runs/{run_id}/browser-session/control",
            "/v1/agent/runs/{run_id}/browser-session/timeline",
            "/v1/agent/runs/{run_id}/egress-receipts",
            "/v1/agent/runs/{run_id}/tabs",
            "/v1/agent/runs/{run_id}/tabs/{tab_id}",
            "/v1/agent/runs/{run_id}/tabs/{tab_id}/select",
            "/v1/agent/runs/{run_id}/tabs/{tab_id}/control",
            "/v1/agent/runs/{run_id}/devtools",
            "/v1/agent/runs/{run_id}/frames/ws",
            "/v1/agent/runs/{run_id}/frames/stream",
            "/v1/agent/runs/{run_id}/frame",
            "/v1/agent/runs/{run_id}/live-view",
            "/v1/agent/runs/{run_id}/events/stream",
        ],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &["agent_routes::tests::zdr_expiry_writes_no_timeline_row_and_no_checkpoint"],
        basis: "every step, tab and control change skips receipts and timeline rows when the run is ZDR (step.rs, control.rs, projection.rs; the person's tab hand-over, attach::control_tab, records through control::record_browser_timeline_event, which returns before any write for a ZDR run)",
        log: Log::Unverified,
    },
    Row {
        id: "agent_receipts_and_procedures",
        routes: &[
            "/v1/agent/runs/{run_id}/receipts",
            "/v1/agent/runs/{run_id}/procedure",
            "/v1/agent/procedures/replay-check",
            "/v1/agent/procedures/quality-check",
            "/v1/agent/procedures/impact-check",
        ],
        treatment: Treatment::Inert {
            reason: "reads receipts a ZDR run never wrote; the checks are pure functions of the request body",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "receipts.rs lists from the receipt store; replay, quality and impact checks hold no state",
        log: Log::Unverified,
    },
    Row {
        id: "fleets",
        routes: &[
            "/v1/fleets",
            "/v1/fleets/{fleet_id}",
            "/v1/fleets/{fleet_id}/members",
            "/v1/fleets/{fleet_id}/budget",
        ],
        treatment: Treatment::Inert {
            reason: "an in-memory registry; nothing is persisted, and the console is hidden (WS2.E)",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "fleet_routes keep fleets in the process (new_fleet_state)",
        log: Log::Unverified,
    },
    Row {
        id: "graphql",
        routes: &["/graphql", "/graphql/schema", "/graphql/playground"],
        treatment: Treatment::Inert {
            reason: "a read-only API over records that exist",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "the GraphQL schema resolves queries from existing stores and mutates none",
        log: Log::Unverified,
    },
    Row {
        id: "operations",
        routes: &["/health", "/ready", "/version"],
        treatment: Treatment::Inert {
            reason: "process state; carries no caller content",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "routes::health, ready and version",
        log: Log::Unverified,
    },
    Row {
        id: "internal_run_page",
        routes: &["/v1/internal/run_page"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[],
        basis: "routes::internal_run_page builds the runner with ZdrMode from the orchestrator's request",
        log: Log::Unverified,
    },
    Row {
        id: "internal_schedule_preflight",
        routes: &["/v1/internal/schedules/preflight"],
        treatment: Treatment::Inert {
            reason: "answers allow with the posture unchecked and stores nothing; it carries a schedule id and an org id",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "schedule_preflight::preflight checks the runtime token and the run binding, then returns the S2 answer. It does NOT check the ZDR posture at fire time (WS15.S4 is not built), so an organization that turns ZDR on later keeps its schedules firing: that is the schedules row's gap, not a second one",
        log: Log::Unverified,
    },
    Row {
        id: "internal_org_profile_purge",
        routes: &["/v1/internal/orgs/{org}/profiles"],
        treatment: Treatment::Inert {
            reason: "erases the organization's profile families and answers counts only; it stores nothing",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "internal_routes::purge_org_profiles checks the shared internal token, walks the org erasure families in AppState and reports row, tombstone and cache-key counts (WS17.E3); it is how a ZDR organization's erasure is carried out, so it is never refused",
        log: Log::Unverified,
    },
    Row {
        id: "internal_page_images",
        routes: &["/v1/internal/page-images/{org}/{doc}/{page}/{hash}"],
        treatment: Treatment::Inert {
            reason: "serves a stored page image by content hash; a ZDR run stores none",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "resource_routes::get_page_image reads the content-addressed store",
        log: Log::Unverified,
    },
    Row {
        id: "internal_queues",
        routes: &[
            "/v1/internal/queues/{queue_name}/enqueue",
            "/v1/internal/queues/{queue_name}/enqueue_batch",
            "/v1/internal/queues/{queue_name}/pop",
            "/v1/internal/queues/{queue_name}/ack",
        ],
        treatment: Treatment::Inert {
            reason: "the worker queue behind crawl and batch, which refuse ZDR at the public edge",
        },
        status: Status::Enforced,
        enforced_by: &["zdr_claim_floor::durable_crawl_and_batch_are_refused_for_a_zdr_org_user_and_open_otherwise"],
        basis: "queue_routes are internal and carry no ZDR flag; nothing ZDR can reach them through the public routes",
        log: Log::Unverified,
    },
];

/// Rows that must stay refused and enforced: loosening one is a regression this
/// table cannot make quietly.
pub const MUST_STAY_REFUSED: &[&str] = &[
    "durable_jobs",
    "agent_persistent_profile",
    "agent_resume",
    "scrape_record_read",
];

fn normalize(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                }
                out.push_str("{}");
            }
            ':' if chars.peek().is_some_and(|n| n.is_ascii_alphabetic() || *n == '_') => {
                while chars.peek().is_some_and(|n| n.is_ascii_alphanumeric() || *n == '_') {
                    chars.next();
                }
                out.push_str("{}");
            }
            other => out.push(other),
        }
    }
    out
}

/// Every route in the table, normalized (`:id` and `{id}` both become `{}`).
pub fn classified_routes() -> Vec<String> {
    let mut routes: Vec<String> = ROWS
        .iter()
        .flat_map(|row| row.routes.iter().map(|route| normalize(route)))
        .collect();
    routes.sort();
    routes
}

/// Normalize a route the way [`classified_routes`] does.
pub fn normalize_route(path: &str) -> String {
    normalize(path)
}

fn treatment_name(treatment: &Treatment) -> &'static str {
    match treatment {
        Treatment::Honored => "honored",
        Treatment::InlineOnly => "inline_only",
        Treatment::Degraded { .. } => "degraded",
        Treatment::Refused { .. } => "refused",
        Treatment::Inert { .. } => "inert",
    }
}

fn treatment_detail(treatment: &Treatment) -> Value {
    match treatment {
        Treatment::Honored | Treatment::InlineOnly => Value::Null,
        Treatment::Degraded { warning } => json!({ "warning": warning }),
        Treatment::Refused { code, feature } => json!({ "code": code, "feature": feature }),
        Treatment::Inert { reason } => json!({ "reason": reason }),
    }
}

/// The published matrix as JSON.
pub fn to_json() -> Value {
    let rows: Vec<Value> = ROWS
        .iter()
        .map(|row| {
            let (status, issue) = match row.status {
                Status::Enforced => ("enforced", Value::Null),
                Status::Gap { issue } => ("gap", json!(issue)),
            };
            let (log, canary) = match row.log {
                Log::Canary { test } => ("canary", json!(test)),
                Log::Unverified => ("log_unverified", Value::Null),
            };
            json!({
                "id": row.id,
                "routes": row.routes,
                "treatment": treatment_name(&row.treatment),
                "detail": treatment_detail(&row.treatment),
                "status": status,
                "issue": issue,
                "enforced_by": row.enforced_by,
                "basis": row.basis,
                "log": log,
                "log_canary": canary,
            })
        })
        .collect();
    json!({
        "contract": "quarry.zdr-feature-matrix",
        "version": 1,
        "treatments": ["honored", "inline_only", "degraded", "refused", "inert"],
        "rows": rows,
    })
}

/// The published matrix as Markdown, for people (support, security review).
pub fn to_markdown() -> String {
    let mut out = String::new();
    out.push_str("# Zero Data Retention: what Quarry does today\n\n");
    out.push_str("Generated from `crates/quarry-core/src/zdr_matrix.rs`; do not edit by hand.\n");
    out.push_str("`QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix` rewrites this file and the JSON golden.\n\n");
    out.push_str("This describes behaviour. It does not change it. A **gap** is a place where ZDR is not honoured today.\n\n");
    out.push_str("| Treatment | Meaning |\n|---|---|\n");
    out.push_str("| honored | the feature runs and nothing about the request is retained |\n");
    out.push_str("| inline only | the result is in the response only; nothing is stored |\n");
    out.push_str("| degraded | the feature runs with less than it has outside ZDR |\n");
    out.push_str("| refused | the request is refused with a message naming ZDR |\n");
    out.push_str("| inert | ZDR does not apply: nothing of the caller's content is stored, or the route is not reachable with ZDR |\n\n");
    let gaps: Vec<&Row> = ROWS
        .iter()
        .filter(|row| matches!(row.status, Status::Gap { .. }))
        .collect();
    out.push_str(&format!("{} of {} rows are gaps.\n\n", gaps.len(), ROWS.len()));
    out.push_str("| Feature | Routes | Treatment | Status | Log |\n|---|---|---|---|---|\n");
    for row in ROWS {
        let routes = if row.routes.is_empty() {
            "(same route as another row)".to_owned()
        } else {
            row.routes
                .iter()
                .map(|route| format!("`{route}`"))
                .collect::<Vec<_>>()
                .join("<br>")
        };
        let treatment = match row.treatment {
            Treatment::Honored => "honored".to_owned(),
            Treatment::InlineOnly => "inline only".to_owned(),
            Treatment::Degraded { warning } => format!("degraded: {warning}"),
            Treatment::Refused { code, feature } => format!("refused (`{code}`, `{feature}`)"),
            Treatment::Inert { reason } => format!("inert: {reason}"),
        };
        let status = match row.status {
            Status::Enforced => "enforced".to_owned(),
            Status::Gap { issue } => format!("**gap**: {issue}"),
        };
        let log = match row.log {
            Log::Canary { test } => format!("canary `{test}`"),
            Log::Unverified => "log-unverified".to_owned(),
        };
        out.push_str(&format!(
            "| `{}` | {routes} | {treatment} | {status} | {log} |\n",
            row.id
        ));
    }
    out.push_str("\n## Basis\n\n");
    for row in ROWS {
        out.push_str(&format!("- `{}`: {}", row.id, row.basis));
        if !row.enforced_by.is_empty() {
            out.push_str(&format!(" Tests: {}.", row.enforced_by.iter().map(|test| format!("`{test}`")).collect::<Vec<_>>().join(", ")));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn crate_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn golden_json_path() -> std::path::PathBuf {
        crate_dir().join("../quarry-runtime/contracts/zdr-feature-matrix.json")
    }

    fn markdown_path() -> std::path::PathBuf {
        crate_dir().join("../../docs/ZDR_MATRIX.md")
    }

    fn json_text() -> String {
        serde_json::to_string_pretty(&to_json()).unwrap() + "\n"
    }

    #[test]
    fn the_golden_and_markdown_are_what_the_table_generates() {
        let json = json_text();
        let markdown = to_markdown();
        let sha = format!("{:x}  zdr-feature-matrix.json\n", Sha256::digest(json.as_bytes()));
        let sha_path = golden_json_path().with_extension("json.sha256");
        if std::env::var("QUARRY_UPDATE_GOLDEN").as_deref() == Ok("1") {
            std::fs::write(golden_json_path(), &json).unwrap();
            std::fs::write(&sha_path, &sha).unwrap();
            std::fs::write(markdown_path(), &markdown).unwrap();
            return;
        }
        let read = |path: std::path::PathBuf| {
            std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("{} is missing: run with QUARRY_UPDATE_GOLDEN=1", path.display()))
                .replace("\r\n", "\n")
        };
        assert_eq!(read(golden_json_path()), json, "zdr-feature-matrix.json drifted from the table");
        assert_eq!(read(sha_path), sha, "the sha256 pin drifted from the golden");
        assert_eq!(read(markdown_path()), markdown, "docs/ZDR_MATRIX.md drifted from the table");
    }

    #[test]
    fn ids_are_unique_and_every_row_explains_itself() {
        let mut ids: Vec<&str> = ROWS.iter().map(|row| row.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate row id");
        for row in ROWS {
            assert!(row.basis.len() >= 20, "{}: the basis says nothing", row.id);
            if let Status::Gap { issue } = row.status {
                assert!(issue.len() >= 8, "{}: a gap names what closes it", row.id);
                assert!(
                    !matches!(row.treatment, Treatment::Honored | Treatment::InlineOnly),
                    "{}: a row that honours ZDR is not a gap",
                    row.id
                );
            }
            if let Treatment::Inert { reason } = row.treatment {
                assert!(reason.len() >= 15, "{}: an inert row says why", row.id);
            }
        }
    }

    #[test]
    fn a_route_is_classified_once() {
        let routes = classified_routes();
        let mut deduped = routes.clone();
        deduped.dedup();
        assert_eq!(routes, deduped, "a route appears in two rows");
    }

    #[test]
    fn existing_refusals_are_never_loosened() {
        for id in MUST_STAY_REFUSED {
            let row = ROWS.iter().find(|row| row.id == *id).unwrap_or_else(|| panic!("{id} is gone"));
            assert!(matches!(row.treatment, Treatment::Refused { .. }), "{id} is no longer refused");
            assert_eq!(row.status, Status::Enforced, "{id} is no longer enforced");
        }
    }

    #[test]
    fn the_markdown_names_every_gap() {
        let markdown = to_markdown();
        for row in ROWS.iter().filter(|row| matches!(row.status, Status::Gap { .. })) {
            assert!(markdown.contains(&format!("`{}`", row.id)));
        }
        assert!(markdown.contains("rows are gaps"));
    }

    #[test]
    fn normalize_makes_both_spellings_equal() {
        assert_eq!(normalize_route("/v1/agent/runs/:run_id/tabs/:tab_id"), normalize_route("/v1/agent/runs/{x}/tabs/{y}"));
        assert_eq!(normalize_route("/v1/:kind/jobs"), "/v1/{}/jobs");
    }
}
