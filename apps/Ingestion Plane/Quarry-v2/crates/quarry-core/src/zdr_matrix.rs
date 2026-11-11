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
            "ai_formats::zdr_skips_every_format_with_no_model_call_and_stores_nothing",
            "ai_formats_real_token::a_zdr_org_user_gets_every_format_skipped_even_when_the_body_says_zdr_false",
        ],
        basis: "no artifact, page-cache entry, URL-result-cache entry (INNH-7, see scrape_max_age), scrape record or durable event; the text comes back in the response (default format markdown), cut at max_inline_bytes. The AI formats (json, summary, question, highlights; INNH-1, dark behind QUARRY_EDGE__AI_FORMATS) are refused under ZDR (ADR-4): each is answered status skipped, code ZDR_UNAVAILABLE inside the 200, with no Model Plane call, so no page text leaves for a model; QUARRY_EDGE__AI_FORMATS_ZDR accepts only refuse in v1 and the engine refuses a ZDR input as well",
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
            "ai_formats::the_stream_route_refuses_ai_formats_before_any_fetch",
        ],
        basis: "the run's posture is the request flag raised by the token floor; page events go to live subscribers only. The AI formats are not served on the stream at all (400 unsupported_in_stream, INNH-1), so no model call can happen on this route",
        log: Log::Canary {
            test: "zdr_log_capture::a_zdr_streamed_scrape_logs_no_url_from_its_spawned_runner",
        },
    },
    Row {
        id: "scrape_max_age",
        routes: &[],
        treatment: Treatment::Degraded {
            warning: "max_age_ignored_zdr",
        },
        status: Status::Enforced,
        enforced_by: &[
            "scrape_max_age::zdr_never_reads_or_writes_the_url_cache_and_says_so",
            "scrape_cache::tests::zdr_wins_over_everything_else",
            "scrape_cache::tests::a_zdr_response_carries_no_cache_freshness_fields",
            "scrape_cache::tests::monitor_paths_never_reach_the_url_cache",
        ],
        basis: "same route as scrape: INNH-7 max_age reads and writes quarry:scrape:{org}:{hash} (the output and warnings, no inline text; the text is rebuilt from artifacts), dark behind QUARRY_EDGE__SCRAPE_MAX_AGE. Under ZDR (body flag or the token's floor) the cache is neither read nor written and no key is computed, not even a hashed URL (a hash of a known URL is a membership oracle); the scrape runs as a fresh fetch and the response carries the warning max_age_ignored_zdr with cache_state bypass, no cached_at and no cache_age_ms, and a fetched_at that is the time of that fetch. The decision is made in scrape_cache::eligibility before any key exists. A hit carries the original fetched_at, cached_at (the same instant) and cache_age_ms. An entry lives for cache.max_age_s (default 3600 s), so a max_age longer than that behaves as the TTL, and the response does not say so yet. Monitors never reach the cache: a change monitor fetches through /v1/internal/run_page and records through /v1/internal/change/record, and only scrape_with calls result_key, get_result or put_result",
        log: Log::Unverified,
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
        id: "scrape_profile_pin",
        routes: &[],
        treatment: Treatment::Refused {
            code: "BAD_REQUEST",
            feature: "persistent_profile",
        },
        status: Status::Enforced,
        enforced_by: &[
            "routes::tests::a_pinned_scrape_is_refused_under_zdr_before_any_fetch",
            "routes::tests::a_pinned_scrape_with_ingest_is_refused_and_nothing_is_ingested",
            "routes::tests::a_service_principal_or_api_key_cannot_pin_a_profile",
            "routes::tests::the_stream_route_refuses_a_profile_id_while_the_pin_is_on",
        ],
        basis: "same route as scrape: scrape_with refuses a profile_id under ZDR with 400 profile_zdr_forbidden, by the request's zdr flag or the token's ZDR floor, before any profile is read or any page fetched, and refuses ingest with 400 profile_ingest_forbidden; profile_guard::precheck refuses a service principal with 403, an API key is refused with 403 api_key_profile_refused, profile_guard::authorize decides the owner check (it denies a non-owner only under QUARRY_EDGE__PROFILE_OWNER_ENFORCE=enforce), and scrape_with answers 404 for a profile the organization does not hold (a scrape never creates one). The restore is read-only, so nothing is written back to the profile, and a pinned scrape is never cached (it takes the browser plan, which both cache gates refuse) or ingested",
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
            "answer_provider_status::include_answer_under_zdr_is_skipped_with_no_model_call_and_no_meter",
        ],
        basis: "paid providers closed, search cache bypassed, no durable SearchIssued event; the provider failure is logged as a class only. include_answer (INNH-8a) sends nothing to a model under ZDR: answer_status skipped_zdr, the answer pipeline never starts, no answer usage is metered",
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
            warning: "no suggestions under ZDR: the partial query never leaves Quarry",
        },
        status: Status::Enforced,
        enforced_by: &["zdr_gap_rows::a_zdr_caller_gets_no_suggestions_and_the_model_plane_sees_no_request"],
        basis: "search_routes::suggest (dark behind QUARRY_EDGE__SEARCH_SUGGEST) reads the token's ZDR floor (effective_zdr) after the switch check and, under ZDR, answers an empty 200 before any Model Plane call, because the Model Plane hop carries no ZDR posture (ADR-4, the INNH-8 /v1/answer precedent); the route's contract is always 200 so the UI degrades instead of seeing a refusal; the skip is logged as the row id only, never the query",
        log: Log::Unverified,
    },
    Row {
        id: "answer",
        routes: &["/v1/answer", "/v1/answer/stream"],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "answer_synthesis",
        },
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::answer_is_refused_for_a_zdr_org_user_and_runs_as_before_otherwise",
            "zdr_claim_floor::answer_stream_is_refused_for_a_zdr_org_user_and_runs_as_before_otherwise",
            "answer_provider_status::a_zdr_answer_is_refused_before_the_pipeline_starts_and_nothing_reaches_the_model",
        ],
        basis: "answer_routes::answer_run and ::answer_stream refuse a request under effective ZDR (the body flag or the token's floor) with a 403 FORBIDDEN, reason zdr_unavailable, before the deployment's gate, the answer cache and the pipeline start: no search, no page fetch and no Model Plane call, because the model hop carries no ZDR posture (ADR-4) and would take the query and the page text. A refused ZDR run leaves only a metadata-only tombstone (outcome refused). include_answer on /v1/search is not refused: it is answered answer_status skipped_zdr, with the search itself run under ZDR and no model call",
        log: Log::Unverified,
    },
    Row {
        id: "extract",
        routes: &["/v1/extract"],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "zdr_claim_floor::extract_runs_as_zdr_for_a_zdr_org_user_and_not_for_a_registry_zdr_service",
            "zdr_claim_floor::extract_with_a_prompt_or_schema_never_reaches_the_model_for_a_zdr_org_user",
            "extract_routes::a_zdr_request_never_reaches_the_model",
            "ai_formats::extraction_under_zdr_is_refused_before_any_request",
        ],
        basis: "the page runs carry the effective ZDR posture, so no artifact or durable page event is written. A schema or a prompt (INNH-2) would send page text to the Model Plane, whose invoke call has no ZDR posture (ADR-4), so under ZDR each source is answered status error, ZDR_UNAVAILABLE, with no Model Plane call; the harvested structured data still returns; the engine refuses a ZDR call as well",
        log: Log::Unverified,
    },
    Row {
        id: "parse",
        routes: &["/v1/parse"],
        treatment: Treatment::Inert {
            reason: "reads the PDF or DOCX bytes the caller uploaded, answers their text and stores nothing: no artifact, cache entry, index row, event, usage record or ingest; the handler takes no AppState",
        },
        status: Status::Enforced,
        enforced_by: &[
            "parse_route::nothing_is_stored_for_a_standard_or_a_zdr_caller",
            "parse_route::the_handler_cannot_reach_a_store",
        ],
        basis: "parse_routes::parse (INNH-11, dark behind QUARRY_EDGE__PARSE_ROUTE) reads the body, sniffs it and parses it through content_route in the isolated child; it logs nothing about the request and reads no filename header. The response carries Cache-Control: no-store. No redaction is applied to the returned text",
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
        id: "audit_page",
        routes: &["/v1/audit/page"],
        treatment: Treatment::Inert {
            reason: "fetches robots and one page and returns bounded evidence; nothing is stored",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "audit_page::capture touches no cache, artifact store, index or event sink",
        log: Log::Unverified,
    },
    Row {
        id: "browser_redirect_probe",
        routes: &["/v1/browser/redirect-hop"],
        treatment: Treatment::Inert {
            reason: "checks one public HTTP response header for a redirect and stores nothing",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "browser_redirect::probe uses credentialless pinned transport and touches no cache, artifact store, index or event sink",
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
            "/v1/sources/{id}/definition",
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
        id: "job_results",
        routes: &[
            "/v1/crawl/{id}/results",
            "/v1/batch/{id}/results",
            "/v1/jobs/{id}/results",
        ],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "job_result_urls",
        },
        status: Status::Enforced,
        enforced_by: &[
            "job_results::a_zdr_token_is_403_with_the_reason_and_control_is_not_called",
            "job_results::flag_off_is_501_and_control_is_not_called",
        ],
        basis: "job_results_routes::gate refuses a caller whose token floor is ZDR before it asks Control Plane anything: a row is the URL, title and artifact ids of a page the job fetched (INNH-12), read from control's page_fetched events. Dark behind QUARRY_EDGE__JOB_RESULTS (501 off, which wins over the refusal); a service principal is refused 403 by the route scope. No ZDR job exists to have results (crawl and batch are refused at handoff) and a ZDR run stores no artifact, so the ids are absent for it as well. Page content is not in the response: it is read by id through the artifacts row, which binds the read to the org",
        log: Log::Unverified,
    },
    Row {
        id: "webhook_subscriptions",
        routes: &[
            "/v1/webhook-subscriptions",
            "/v1/webhook-subscriptions/{id}",
            "/v1/webhook-subscriptions/{id}/rotate-secret",
            "/v1/webhook-subscriptions/{id}/deliveries",
            "/v1/webhook-subscriptions/{id}/deliveries/{did}/retry",
        ],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "webhook_off_process",
        },
        status: Status::Enforced,
        enforced_by: &[
            "webhook_subscriptions::a_zdr_token_cannot_create_patch_or_retry_and_control_is_not_called",
            "webhook_subscriptions::flag_off_is_501_and_control_is_not_called",
            "webhook_subscription_routes::tests::zdr_refuses_what_pushes_and_leaves_wind_down_open",
        ],
        basis: "webhook_subscription_routes::gate refuses create, patch and delivery retry for a caller whose token floor is ZDR before it asks Control Plane anything (403, details.reason webhook_off_process): a subscription pushes the page URL and fingerprints of every detected change to a third party. Reads, delete and secret rotation stay open so a ZDR organization can wind down what it holds. Dark behind QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS (501 off, which wins over the refusal); a service principal is refused 403 by the route scope and by the gate. Nothing is sent for a ZDR run in any case: emit_change_side_effects suppresses the change event and webhook for a declared zdr, so a subscription an organization made before it turned ZDR receives nothing from a ZDR check. Control keeps a subscription (URL and the HMAC secret, the secret stored in plaintext: see the owner decision in docs/RUNBOOKS.md) and each delivery row (the event payload) until delete, org erasure or the delivery retention sweep (default 14 days)",
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
        basis: "record_internal calls zdr_refusal::refuse on the body's declared zdr before the store (flag on: 403; flag off: zdr.would_refuse and the baseline is stored as before); emit_change_side_effects suppresses the change event and webhook for a declared zdr in both modes; with QUARRY_EDGE__CHANGE_TEXT_DIFF on (INNH-4) record_core still keeps no page text, markdown artifact id or text diff for a declared zdr in either mode (change_routes::record_tests::a_zdr_declared_check_never_stores_text_even_with_the_flag_on). The orchestrator declares the posture quarry-control stamped on the schedule at create (migration 023); a schedule created before that (NULL) or before its org turned ZDR on declares nothing or false and is stored (pinned by zdr_posture_stamp::record_trusts_the_stamp_not_the_current_posture); the orchestrator also declares the stamp on the run_page fetch that precedes the record call, so a stamped-ZDR monitor's fetch runs in ZDR mode and persists no page artifact, event or local-index write-back; ChangeMonitorWF still emits run_started and change_detected with the URL. No test observes the baseline write, because the harness wires no baseline store",
        log: Log::Unverified,
    },
    Row {
        id: "change_diff_read",
        routes: &["/v1/change/diff"],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "change_diff_read",
        },
        status: Status::Enforced,
        enforced_by: &[
            "change_diff::tests::the_gate_names_the_variable_off_and_refuses_zdr",
            "change_diff_route::the_route_is_off_by_default_then_gates_zdr_and_the_query",
        ],
        basis: "change_routes::diff calls change_diff::gate, which refuses a reader whose organization is under ZDR (the token's floor or the body flag) before the store is asked, even for a baseline stored before it was; dark behind QUARRY_EDGE__CHANGE_TEXT_DIFF (501 off, which wins over the refusal). The route returns stored page text (the markdown of two baselines and a paragraph diff of it). The text exists only for a change monitor that was not ZDR when it ran: a ZDR run stores no markdown artifact and run_page returns no markdown_artifact_id, and record_core keeps no text or diff for a body that declares zdr in either refusal mode. No hash of a page is returned in its place",
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
        id: "agent_run_api_key",
        routes: &[],
        treatment: Treatment::Honored,
        status: Status::Enforced,
        enforced_by: &[
            "api_key_agent_runs::a_zdr_key_token_makes_a_zdr_run_and_a_zdr_grant_needs_one",
            "api_key_agent_runs::a_key_run_takes_no_profile_and_no_resume",
            "api_key_fence::switch_on_with_the_scope_refuses_every_route_off_the_allow_list",
        ],
        basis: "an API-key principal (INNH-10, dark behind QUARRY_EDGE__API_KEY_AGENT_RUNS) starts a run on the agent_run route and keeps exactly the persistence of agent_run: a ZDR run writes no checkpoint, timeline row or receipt. On top of that, in every posture: no resume and no profile or persist_profile (api_key_resume_refused, api_key_profile_refused, so a key run never reads or writes a durable profile or checkpoint of its own), no artifacts route (the route fence refuses /v1/artifacts*, api_key_route_refused), no sensitive action. The token's zdr claim is the organization's posture raised by the key's own flag and counts as a floor like a user's (auth::Claims::zdr_floor), and a ZDR grant cannot back a non-ZDR run. Quarry sends nothing to a model on these routes",
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
        id: "agent_hit_test",
        routes: &["/v1/agent/runs/{run_id}/hit-test"],
        treatment: Treatment::Inert {
            reason: "a read-only point lookup on the caller's own live run; it writes no receipt, timeline row, artifact or cache entry, and logs no page text",
        },
        status: Status::Enforced,
        enforced_by: &["hit_test::hit_test_tests::the_wire_answer_carries_refs_and_no_browser_handles"],
        basis: "agent_routes::hit_test::hit_test_point (E3.3-h, dark behind QUARRY_EDGE__HIT_TEST: 403 hit_test_disabled when off, and 200 when on, through BrowserDriver::point_hit) reads DOM.getNodeForLocation and the partial accessibility tree, maps the hit to the person's lane's last observed @e ref, and answers the run's zdr flag with it. The AX name it returns is page text the same caller's observation already carries under ZDR; no backend node id or frame id is on the wire. Nothing is stored, so ZDR has nothing to suppress and the route is not refused",
        log: Log::Unverified,
    },
    Row {
        id: "agent_downloads_list",
        routes: &["/v1/agent/runs/{run_id}/downloads"],
        treatment: Treatment::Inert {
            reason: "reads downloads a ZDR run never admitted; a ZDR caller or run gets an empty list without any receipt or artifact read",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "agent_routes::downloads::list_run_downloads (Q3-E3.4-7a, dark behind QUARRY_EDGE__DOWNLOADS: 403 downloads_disabled when off) lists the run's admitted downloads from the caller's own receipts and the tenant-scoped artifact store. It writes nothing; the answer names the artifact id, type, size and digest, never a URL or a file name. A ZDR run stores no download (the step is refused under ZDR), so there is nothing to list",
        log: Log::Unverified,
    },
    Row {
        id: "agent_download_handoff",
        routes: &["/v1/agent/runs/{run_id}/downloads/{artifact_id}/knowledge"],
        treatment: Treatment::Refused {
            code: "FORBIDDEN",
            feature: "download_handoff",
        },
        status: Status::Enforced,
        enforced_by: &[],
        basis: "agent_routes::downloads::send_download_to_knowledge (Q3-E3.4-7a, dark behind QUARRY_EDGE__DOWNLOADS and QUARRY_EDGE__DOWNLOAD_HANDOFF) persists a download's text to Data Plane, a durable write, so a ZDR token floor or a ZDR run is refused with download_handoff_zdr_refused (403) before any receipt or byte is read; the ingest request builder and the transport repeat the guard. The org and user come from the verified claim only",
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
        id: "internal_crawl_sitemap",
        routes: &["/v1/internal/crawl/sitemap"],
        treatment: Treatment::Inert {
            reason: "reads the public sitemap files of a crawl's site and answers URLs and counts; it stores nothing, and a crawl is never ZDR (the handoff refuses it), so a body that declares zdr is a 400",
        },
        status: Status::Enforced,
        enforced_by: &[
            "crawl_sitemap::the_route_checks_the_token_the_org_binding_and_the_body",
        ],
        basis: "crawl_sitemap::internal_sitemap (INNH-6a, dark behind QUARRY_EDGE__CRAWL_SITEMAP) fetches robots.txt through the shared gate and each sitemap file through the guarded driver chain, then returns the URLs the crawl scope and robots allow; it writes no artifact, event, cache entry or record. The robots rules it reads are cached by the gate as for any page",
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
        id: "internal_user_receipt_purge",
        routes: &["/v1/internal/orgs/{org}/users/{user}/receipts"],
        treatment: Treatment::Inert {
            reason: "erases one user's step receipts and browser timeline events in one organization and answers counts only; it stores nothing",
        },
        status: Status::Enforced,
        enforced_by: &[
            "internal_routes::tests::the_user_report_is_one_counts_only_family",
            "step_receipts::user_erasure_tests::purge_for_actor_removes_only_that_user_in_that_org_and_is_idempotent",
        ],
        basis: "internal_routes::purge_user_receipts (W2-U49, X-19 condition c) checks the shared internal token and both path ids, deletes the rows whose org_id and actor_id match (Postgres over the queue pool when present, else the receipt store's purge_for_actor) and reports one counts-only step_receipts family. quarry-control calls it for a user or user_anonymize erasure event only when QUARRY_CONTROL_GDPR_USER_RECEIPTS is on (default off). A ZDR run writes no receipts, so there is nothing of it to find; like the org purge it is how erasure is carried out, so it is never refused",
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
            "/v1/internal/queues/{queue_name}/seen",
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
    "answer",
    "agent_persistent_profile",
    "agent_resume",
    "scrape_record_read",
    "change_diff_read",
    "webhook_subscriptions",
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

    /// The `scrape_max_age` row keeps the monitor bypass and the freshness fields in its
    /// text: the row and its basis are what a support or security reader sees first.
    #[test]
    fn the_scrape_max_age_row_names_the_monitor_bypass_and_the_freshness_fields() {
        let row = ROWS.iter().find(|row| row.id == "scrape_max_age").expect("a scrape_max_age row");
        for needle in [
            "/v1/internal/run_page",
            "/v1/internal/change/record",
            "only scrape_with",
            "cached_at",
            "cache_age_ms",
            "fetched_at",
            "bypass",
            "max_age_ignored_zdr",
        ] {
            assert!(row.basis.contains(needle), "the scrape_max_age basis no longer says `{needle}`");
        }
        for test in [
            "scrape_cache::tests::monitor_paths_never_reach_the_url_cache",
            "scrape_cache::tests::a_zdr_response_carries_no_cache_freshness_fields",
        ] {
            assert!(row.enforced_by.contains(&test), "the scrape_max_age row does not cite {test}");
        }
    }
}
