use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use std::{future::Future, pin::Pin};

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::browser_omnibox::{dispatch_browser_omnibox_search, dispatch_browser_omnibox_suggest};

use super::dispatchers::{
    dispatch_audio_dictate, dispatch_audio_transcribe, dispatch_browser_blocked_try_other_source,
    dispatch_browser_close_session, dispatch_browser_keepalive_session,
    dispatch_browser_close_tab, dispatch_browser_create_profile, dispatch_browser_create_session,
    dispatch_browser_create_tab, dispatch_browser_delete_profile,
    dispatch_browser_probe_profile_restore, dispatch_browser_rename_profile,
    dispatch_browser_run_action, dispatch_browser_run_control, dispatch_browser_run_start,
    dispatch_browser_run_steer, dispatch_browser_download_send_to_knowledge,
    dispatch_browser_list_origin_consents, dispatch_browser_revoke_origin_consent,
    dispatch_browser_select_tab, dispatch_browser_set_control_mode,
    dispatch_browser_suggest_action, dispatch_chat_approve_plan, dispatch_chat_cancel_invocation,
    dispatch_chat_clear_threads, dispatch_chat_delete_thread, dispatch_chat_queue_invocation_input,
    dispatch_chat_save_thread_snapshot, dispatch_chat_submit_feedback,
    dispatch_chat_upload_document, dispatch_chat_extract_document,
    dispatch_finetune_cancel_job, dispatch_finetune_create_job, dispatch_finetune_deploy_job,
    dispatch_inbox_add_tag, dispatch_inbox_claim_draft_lease,
    dispatch_inbox_create_draft_reply_proposal, dispatch_inbox_create_incident_proposal,
    dispatch_inbox_create_internal_note_proposal, dispatch_inbox_create_problem_proposal,
    dispatch_inbox_create_ticket_update_proposal, dispatch_inbox_delete_draft,
    dispatch_inbox_release_draft_lease, dispatch_inbox_remove_tag, dispatch_inbox_review_ai_action,
    dispatch_inbox_save_draft, dispatch_inbox_send_reply, dispatch_inbox_set_assignment,
    dispatch_inbox_set_status, dispatch_inbox_submit_feedback, dispatch_inbox_workspace_set_pinned,
    dispatch_inbox_workspace_set_read, dispatch_ingestions_cancel_run,
    dispatch_ingestions_change_schedule_state, dispatch_ingestions_create_run,
    dispatch_ingestions_job_control, dispatch_ingestions_start_agent_run,
    dispatch_ingestions_create_schedule, dispatch_ingestions_create_source,
    dispatch_ingestions_delete_source, dispatch_ingestions_get_run_status,
    dispatch_ingestions_pause_run, dispatch_ingestions_pause_schedule,
    dispatch_ingestions_resume_run,
    dispatch_integrations_disconnect, dispatch_integrations_disconnect_chatgpt_subscription,
    dispatch_integrations_extend_inbox_history, dispatch_integrations_start_chatgpt_subscription,
    dispatch_integrations_start_connect_session, dispatch_integrations_trigger_inbox_sync,
    dispatch_integrations_trigger_sync, dispatch_knowledge_create_document,
    dispatch_knowledge_extract_products, dispatch_knowledge_register_sharepoint_source,
    dispatch_knowledge_summarize_products,
    dispatch_leads_create_list, dispatch_leads_delete_list,
    dispatch_learning_accept_skill, dispatch_learning_learn_from_thread,
    dispatch_learning_reject_skill, dispatch_learning_teach_skill_from_run,
    dispatch_learning_undo, dispatch_mcp_delete_server,
    dispatch_mcp_share_server, dispatch_membership_invite_member,
    dispatch_membership_remove_member, dispatch_membership_update_member_role,
    dispatch_memory_delete, dispatch_memory_remember, dispatch_memory_reset,
    dispatch_memory_set_state, dispatch_monitoring_check_now,
    dispatch_monitoring_create_monitor, dispatch_monitoring_delete_monitor,
    dispatch_navbar_create_calendar_event,
    dispatch_navbar_create_calendar_note, dispatch_navbar_mark_notification_read,
    dispatch_navbar_save_theme, dispatch_navbar_submit_support_request,
    dispatch_notification_delete, dispatch_notification_mark_all_read,
    dispatch_notification_mark_read, dispatch_notification_update_preference,
    dispatch_orchestration_cancel_run, dispatch_orchestration_decide_approval,
    dispatch_orchestration_resume_run, dispatch_org_acknowledge_deletion,
    dispatch_org_mark_exported, dispatch_org_restore, dispatch_org_set_model_entitlement,
    dispatch_org_set_quota,
    dispatch_org_soft_delete, dispatch_org_update_instructions,
    dispatch_org_update_agent_learning_policy, dispatch_org_update_support_ai_mode,
    dispatch_org_update_zdr,
    dispatch_organization_switch_active, dispatch_ownership_revoke_document_share,
    dispatch_ownership_share_document, dispatch_privacy_erase_my_account,
    dispatch_router_policy_update, dispatch_settings_create_api_key,
    dispatch_settings_delete_api_key, dispatch_settings_set_tone_preset,
    dispatch_settings_complete_personal_setup, dispatch_settings_skip_personal_setup,
    dispatch_settings_update_me,
    dispatch_settings_update_preferences, dispatch_settings_update_setting,
    dispatch_social_create_campaign, dispatch_social_create_draft_from_inbox,
    dispatch_social_decide_approval, dispatch_space_add_member, dispatch_space_bind_agent,
    dispatch_space_create_agent, dispatch_space_create_personal, dispatch_space_create_room,
    dispatch_space_ensure_organization_room, dispatch_space_mark_read,
    dispatch_space_record_presence, dispatch_space_remove_member,
    dispatch_space_request_personal_deletion, dispatch_space_revoke_agent,
    dispatch_space_set_agent_state, dispatch_space_update_instructions,
    dispatch_space_update_thread_presentation,
    dispatch_studio_create_project, dispatch_studio_export_social_draft,
    dispatch_studio_save_project,
};
use super::dispatchers_client_routed::{
    dispatch_agents_admitted_actions_list, dispatch_client_executed, dispatch_owner_not_wired,
};
use super::dispatchers_spaces::ws01::{dispatch_spaces_create_post, dispatch_spaces_reply_in_thread};
use super::dispatchers_spaces::ws04::{
    dispatch_spaces_change_member_role, dispatch_spaces_join_room, dispatch_spaces_leave_room,
    dispatch_spaces_update_room_details, dispatch_spaces_update_room_settings,
};
use super::dispatchers_spaces::ws08::{
    dispatch_spaces_set_agent_pin, dispatch_spaces_set_agent_wake_policy,
};
use super::dispatchers_spaces::ws09::dispatch_spaces_stop_run;
use super::dispatchers_spaces::ws10::{
    dispatch_spaces_decide_approval, dispatch_spaces_revoke_approval_grant,
};
use super::dispatchers_spaces::ws10b::dispatch_spaces_set_tool_policy;
use super::dispatchers_spaces::ep08::{dispatch_spaces_memory_delete, dispatch_spaces_memory_list};
use super::dispatchers_spaces::ws11::{
    dispatch_spaces_mark_all_read, dispatch_spaces_mark_unread, dispatch_spaces_set_attention_item_state,
    dispatch_spaces_set_notify_level, dispatch_spaces_set_status, dispatch_spaces_set_thread_follow,
    dispatch_spaces_cancel_reminder, dispatch_spaces_create_reminder,
};
use super::dispatchers_spaces::ws12::{
    dispatch_spaces_delete_message, dispatch_spaces_edit_message, dispatch_spaces_hide_space,
    dispatch_spaces_open_dm, dispatch_spaces_toggle_reaction,
};
use super::dispatchers_spaces::ws14::{
    dispatch_space_files_attach, dispatch_space_files_delete, dispatch_space_files_list,
    dispatch_space_files_read_text,
};
use super::dispatchers_spaces::ws16::dispatch_spaces_update_sidebar;
use super::dispatchers_browser_policy::{
    dispatch_browser_get_org_policy, dispatch_browser_set_org_policy,
};
use super::dispatchers_spaces::ws03::{
    dispatch_spaces_apply_effect_preset, dispatch_spaces_set_effect_entitlements,
};
use super::dispatchers::{
    dispatch_browser_control_tab, dispatch_browser_stop_attach, dispatch_chat_cancel_research_run,
};
use super::dispatchers::{
    dispatch_brreg_lookup, dispatch_connect_source, dispatch_conversation_csat_preference,
    dispatch_conversation_follow, dispatch_crawl_site, dispatch_import_source,
    dispatch_operating_map_blueprint, dispatch_operating_map_generate,
    dispatch_operating_map_review, dispatch_recrawl, dispatch_scrape_url, dispatch_shipping_quotes,
    dispatch_social_create_draft, dispatch_social_publish_post, dispatch_social_schedule_post,
    dispatch_ticket_add_side_conversation_message, dispatch_ticket_assign,
    dispatch_ticket_classify, dispatch_ticket_create, dispatch_ticket_create_automation_rule,
    dispatch_ticket_create_checklist, dispatch_ticket_create_incident,
    dispatch_ticket_create_macro, dispatch_ticket_create_problem,
    dispatch_ticket_create_side_conversation, dispatch_ticket_create_sla_policy,
    dispatch_ticket_create_team, dispatch_ticket_create_view, dispatch_ticket_link_incident_ticket,
    dispatch_ticket_link_resource, dispatch_ticket_record_chat_handoff,
    dispatch_ticket_record_csat_outcome, dispatch_ticket_resolve, dispatch_ticket_run_macro,
    dispatch_ticket_update, dispatch_ticket_update_automation_rule,
    dispatch_ticket_update_checklist_item, dispatch_ticket_update_incident,
    dispatch_ticket_update_macro, dispatch_ticket_update_problem,
    dispatch_ticket_update_side_conversation, dispatch_ticket_update_sla_policy,
    dispatch_ticket_update_team, dispatch_ticket_update_view, dispatch_toggle_policy,
    dispatch_upload_files, list_owner_action_contracts,
    reconcile_ticket_create as reconcile_ticket_create_dispatch,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecuteActionRequest {
    action_id: String,
    #[serde(default)]
    idempotency_key: String,
    input: Value,
}

/// `VEREVON_GATEWAY_AGENT_WEB_TOOLSET` (E3.7.10, R1 user-session path): turns on the
/// gateway arms of the Model-facing agent web toolset. Off unless it is exactly `true`.
/// Read per request, and only when the action id is one of the four `web.*` ids.
pub(super) const AGENT_WEB_TOOLSET_FLAG: &str = "VEREVON_GATEWAY_AGENT_WEB_TOOLSET";

fn agent_web_toolset_enabled() -> bool {
    agent_web_toolset_from(std::env::var(AGENT_WEB_TOOLSET_FLAG).ok().as_deref())
}

/// Exactly the word `true`, like the other gateway feature switches; anything else,
/// including an unset variable, is off.
fn agent_web_toolset_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// The legacy `web.*` ids of the agent web toolset (E3.7.10), each with the registry action it
/// runs. Each id runs the existing user-session dispatcher of the action it names, so it carries
/// exactly that action's authority and checks and nothing more. `web.crawl_site` is
/// `knowledge.crawl_site`, so the E3.7.9 approval above the page threshold (enforced on the crawl
/// dispatch path) applies to it unchanged; the job verbs are the WS15.L5 run-control dispatchers
/// (the caller's own org, crawl jobs only). There is deliberately no resume entry: the toolset
/// names status, pause and cancel. These keys are not registry contract ids, so the action-surface
/// contract test (`scripts/action-surface-contract.test.mjs`) reads past this table.
const LEGACY_ACTION_ALIASES: [(&str, &str); 4] = [
    ("web.crawl_site", "knowledge.crawl_site"),
    ("web.job_status", "ingestions.get_run_status"),
    ("web.pause_job", "ingestions.pause_run"),
    ("web.cancel_job", "ingestions.cancel_run"),
];

/// The registry action a legacy toolset id runs, or `None` for any other id.
fn agent_web_tool_target(action_id: &str) -> Option<&'static str> {
    LEGACY_ACTION_ALIASES
        .iter()
        .find(|(alias, _)| *alias == action_id)
        .map(|(_, target)| *target)
}

/// The page count above which a Model-started crawl needs the person's E3.7.9 approval.
pub(super) const AGENT_CRAWL_APPROVAL_PAGES: u64 = 50;

/// Whether a `web.crawl_site` call must stop for the E3.7.9 approval. The approval prompt and
/// its record (E3.7.9a/b) are not built yet, so until they are the toolset fails closed: a call
/// that names no page bound (the edge would apply its own default) or a bound above
/// [`AGENT_CRAWL_APPROVAL_PAGES`] is refused, and only a crawl the run budget covers runs.
fn agent_crawl_needs_approval(input: &Value) -> bool {
    match input
        .get("max_pages")
        .or_else(|| input.get("maxPages"))
        .and_then(Value::as_u64)
    {
        Some(pages) => pages > AGENT_CRAWL_APPROVAL_PAGES,
        None => true,
    }
}

/// The id `execute_action` dispatches on. With the toolset off a `web.*` id stays as it is
/// and falls through to the 501 of an unknown action, exactly as before the switch existed.
fn resolve_action_id(action_id: &str, toolset_enabled: bool) -> &str {
    match agent_web_tool_target(action_id) {
        Some(target) if toolset_enabled => target,
        _ => action_id,
    }
}

/// Boxes one dispatcher's future for `execute_action`. The dispatcher is called inside this
/// helper, so the future is built in this helper's frame and then moved to the heap. Awaited
/// directly, each arm's future lived in `execute_action`'s own frame; in a debug build that
/// frame was about 2 MiB, which with the callers' frames overflowed the 2 MiB default thread
/// stack. The boxed future is polled exactly as the plain one was.
fn boxed_dispatch<'a, F>(
    make: impl FnOnce() -> F,
) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>>
where
    F: Future<Output = Response> + Send + 'a,
{
    Box::pin(make())
}

pub(super) async fn execute_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<ExecuteActionRequest>,
) -> Response {
    let toolset_enabled =
        agent_web_tool_target(&body.action_id).is_some() && agent_web_toolset_enabled();
    if toolset_enabled
        && agent_web_tool_target(&body.action_id) == Some("knowledge.crawl_site")
        && agent_crawl_needs_approval(&body.input)
    {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "crawl_approval_required",
                format!(
                    "a crawl started by the agent above {AGENT_CRAWL_APPROVAL_PAGES} pages, or without a page bound, needs the person's approval"
                ),
            )),
        )
            .into_response();
    }
    match resolve_action_id(&body.action_id, toolset_enabled) {
        "knowledge.recrawl_source" => {
            boxed_dispatch(|| dispatch_recrawl(&state, &user, &headers, &body.input)).await
        }
        "knowledge.scrape_url" => {
            boxed_dispatch(|| dispatch_scrape_url(&state, &user, &headers, &body.input)).await
        }
        "knowledge.crawl_site" => {
            boxed_dispatch(|| dispatch_crawl_site(&state, &user, &headers, &body.input)).await
        }
        "knowledge.import_source" => {
            boxed_dispatch(|| dispatch_import_source(&state, &user, &headers, &body.input)).await
        }
        "knowledge.connect_source" => {
            boxed_dispatch(|| {
                dispatch_connect_source(&state, &user, &headers, &body.input, &body.idempotency_key)
            })
            .await
        }
        "knowledge.upload_files" => boxed_dispatch(dispatch_upload_files).await,
        "knowledge.create_document" => {
            boxed_dispatch(|| {
                dispatch_knowledge_create_document(&state, &user, &headers, &body.input)
            })
            .await
        }
        "knowledge.extract_products" => {
            boxed_dispatch(|| {
                dispatch_knowledge_extract_products(&state, &user, &headers, &body.input)
            })
            .await
        }
        "knowledge.summarize_products" => {
            boxed_dispatch(|| {
                dispatch_knowledge_summarize_products(&state, &user, &headers, &body.input)
            })
            .await
        }
        "knowledge.register_sharepoint_source" => {
            boxed_dispatch(|| {
                dispatch_knowledge_register_sharepoint_source(&state, &user, &body.input)
            })
            .await
        }
        "brreg_lookup_organization" | "brreg.lookup_organization" => {
            boxed_dispatch(|| dispatch_brreg_lookup(&state, &user, &body.input)).await
        }
        "shipping.get_quotes" => {
            boxed_dispatch(|| dispatch_shipping_quotes(&state, &user, &headers, &body.input)).await
        }
        "operating_map.generate" | "operating_map.refresh" => {
            boxed_dispatch(|| {
                dispatch_operating_map_generate(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                    &body.action_id,
                )
            })
            .await
        }
        "operating_map.review_proposal" => {
            boxed_dispatch(|| dispatch_operating_map_review(&state, &user, &headers, &body.input))
                .await
        }
        "operating_map.create_agent_blueprint" => {
            boxed_dispatch(|| {
                dispatch_operating_map_blueprint(&state, &user, &headers, &body.input)
            })
            .await
        }
        "workflows.toggle_policy" => {
            boxed_dispatch(|| dispatch_toggle_policy(&state, &user, &body.input)).await
        }
        "inbox.follow_conversation" => {
            boxed_dispatch(|| dispatch_conversation_follow(&state, &user, &body.input)).await
        }
        "inbox.set_csat_preference" => {
            boxed_dispatch(|| dispatch_conversation_csat_preference(&state, &user, &body.input))
                .await
        }
        "inbox.add_tag" => {
            boxed_dispatch(|| dispatch_inbox_add_tag(&state, &user, &body.input)).await
        }
        "inbox.remove_tag" => {
            boxed_dispatch(|| dispatch_inbox_remove_tag(&state, &user, &body.input)).await
        }
        "inbox.claim_draft_lease" => {
            boxed_dispatch(|| dispatch_inbox_claim_draft_lease(&state, &user, &body.input)).await
        }
        "inbox.release_draft_lease" => {
            boxed_dispatch(|| dispatch_inbox_release_draft_lease(&state, &user, &body.input)).await
        }
        "inbox.save_draft" => {
            boxed_dispatch(|| dispatch_inbox_save_draft(&state, &user, &body.input)).await
        }
        "inbox.delete_draft" => {
            boxed_dispatch(|| dispatch_inbox_delete_draft(&state, &user, &body.input)).await
        }
        "inbox.set_status" => {
            boxed_dispatch(|| dispatch_inbox_set_status(&state, &user, &body.input)).await
        }
        "inbox.set_assignment" => {
            boxed_dispatch(|| dispatch_inbox_set_assignment(&state, &user, &body.input)).await
        }
        "inbox.send_reply" => {
            boxed_dispatch(|| dispatch_inbox_send_reply(&state, &user, &body.input)).await
        }
        "inbox.submit_feedback" => {
            boxed_dispatch(|| dispatch_inbox_submit_feedback(&state, &user, &body.input)).await
        }
        "inbox.review_ai_action" => {
            boxed_dispatch(|| dispatch_inbox_review_ai_action(&state, &user, &body.input)).await
        }
        "inbox.create_draft_reply_proposal" => {
            boxed_dispatch(|| {
                dispatch_inbox_create_draft_reply_proposal(&state, &user, &body.input)
            })
            .await
        }
        "inbox.create_internal_note_proposal" => {
            boxed_dispatch(|| {
                dispatch_inbox_create_internal_note_proposal(&state, &user, &body.input)
            })
            .await
        }
        "inbox.create_ticket_update_proposal" => {
            boxed_dispatch(|| {
                dispatch_inbox_create_ticket_update_proposal(&state, &user, &body.input)
            })
            .await
        }
        "inbox.create_incident_proposal" => {
            boxed_dispatch(|| dispatch_inbox_create_incident_proposal(&state, &user, &body.input))
                .await
        }
        "inbox.create_problem_proposal" => {
            boxed_dispatch(|| dispatch_inbox_create_problem_proposal(&state, &user, &body.input))
                .await
        }
        "inbox.set_conversation_pinned" => {
            boxed_dispatch(|| dispatch_inbox_workspace_set_pinned(&state, &user, &body.input)).await
        }
        "inbox.set_conversation_read" => {
            boxed_dispatch(|| dispatch_inbox_workspace_set_read(&state, &user, &body.input)).await
        }
        "notifications.mark_read" => {
            boxed_dispatch(|| dispatch_notification_mark_read(&state, &user, &body.input)).await
        }
        "notifications.mark_all_read" => {
            boxed_dispatch(|| dispatch_notification_mark_all_read(&state, &user, &body.input)).await
        }
        "notifications.delete" => {
            boxed_dispatch(|| dispatch_notification_delete(&state, &user, &body.input)).await
        }
        "notifications.update_preference" => {
            boxed_dispatch(|| dispatch_notification_update_preference(&state, &user, &body.input))
                .await
        }
        "navbar.save_theme" => {
            boxed_dispatch(|| dispatch_navbar_save_theme(&state, &user, &body.input)).await
        }
        "navbar.mark_notification_read" => {
            boxed_dispatch(|| dispatch_navbar_mark_notification_read(&state, &user, &body.input))
                .await
        }
        "navbar.create_calendar_event" => {
            boxed_dispatch(|| dispatch_navbar_create_calendar_event(&state, &user, &body.input))
                .await
        }
        "navbar.create_calendar_note" => {
            boxed_dispatch(|| dispatch_navbar_create_calendar_note(&state, &user, &body.input))
                .await
        }
        "navbar.submit_support_request" => {
            boxed_dispatch(|| dispatch_navbar_submit_support_request(&state, &user, &body.input))
                .await
        }
        "ownership.share_document" => {
            boxed_dispatch(|| {
                dispatch_ownership_share_document(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ownership.revoke_document_share" => {
            boxed_dispatch(|| {
                dispatch_ownership_revoke_document_share(&state, &user, &headers, &body.input)
            })
            .await
        }
        "memory.delete" => {
            boxed_dispatch(|| dispatch_memory_delete(&state, &user, &headers, &body.input)).await
        }
        "memory.set_state" => {
            boxed_dispatch(|| dispatch_memory_set_state(&state, &user, &headers, &body.input)).await
        }
        "memory.reset" => {
            boxed_dispatch(|| dispatch_memory_reset(&state, &user, &headers, &body.input)).await
        }
        "memory.remember" => {
            boxed_dispatch(|| dispatch_memory_remember(&state, &user, &headers, &body.input)).await
        }
        "learning.accept_skill" => {
            boxed_dispatch(|| dispatch_learning_accept_skill(&state, &user, &headers, &body.input))
                .await
        }
        "learning.reject_skill" => {
            boxed_dispatch(|| dispatch_learning_reject_skill(&state, &user, &headers, &body.input))
                .await
        }
        "learning.undo" => {
            boxed_dispatch(|| dispatch_learning_undo(&state, &user, &headers, &body.input)).await
        }
        "learning.learn_from_thread" => {
            boxed_dispatch(|| {
                dispatch_learning_learn_from_thread(&state, &user, &headers, &body.input)
            })
            .await
        }
        "learning.teach_skill_from_run" => {
            boxed_dispatch(|| {
                dispatch_learning_teach_skill_from_run(&state, &user, &headers, &body.input)
            })
            .await
        }
        "settings.update_me" => {
            boxed_dispatch(|| dispatch_settings_update_me(&state, &user, &body.input)).await
        }
        "settings.update_preferences" => {
            boxed_dispatch(|| dispatch_settings_update_preferences(&state, &user, &body.input))
                .await
        }
        "settings.update_setting" => {
            boxed_dispatch(|| dispatch_settings_update_setting(&state, &user, &body.input)).await
        }
        "settings.set_tone_preset" => {
            boxed_dispatch(|| dispatch_settings_set_tone_preset(&state, &user, &body.input)).await
        }
        "settings.complete_personal_setup" => {
            boxed_dispatch(|| dispatch_settings_complete_personal_setup(&state, &user, &body.input))
                .await
        }
        "settings.skip_personal_setup" => {
            boxed_dispatch(|| dispatch_settings_skip_personal_setup(&state, &user)).await
        }
        "settings.create_api_key" => {
            boxed_dispatch(|| {
                dispatch_settings_create_api_key(&state, &user, &headers, &body.input)
            })
            .await
        }
        "settings.delete_api_key" => {
            boxed_dispatch(|| {
                dispatch_settings_delete_api_key(&state, &user, &headers, &body.input)
            })
            .await
        }
        "mcp.delete_server" => {
            boxed_dispatch(|| dispatch_mcp_delete_server(&state, &user, &headers, &body.input))
                .await
        }
        "mcp.share_server" => {
            boxed_dispatch(|| dispatch_mcp_share_server(&state, &user, &headers, &body.input)).await
        }
        "ingestions.job_control" => {
            boxed_dispatch(|| dispatch_ingestions_job_control(&state, &user, &headers, &body.input))
                .await
        }
        "ingestions.start_agent_run" => {
            boxed_dispatch(|| {
                dispatch_ingestions_start_agent_run(&state, &user, &headers, &body.input)
            })
            .await
        }
        "monitoring.check_now" => {
            boxed_dispatch(|| dispatch_monitoring_check_now(&state, &user, &headers, &body.input))
                .await
        }
        "monitoring.create_monitor" => {
            boxed_dispatch(|| {
                dispatch_monitoring_create_monitor(&state, &user, &headers, &body.input)
            })
            .await
        }
        "monitoring.delete_monitor" => {
            boxed_dispatch(|| {
                dispatch_monitoring_delete_monitor(&state, &user, &headers, &body.input)
            })
            .await
        }
        "privacy.erase_my_account" => {
            boxed_dispatch(|| dispatch_privacy_erase_my_account(&state, &user, &body.input)).await
        }
        "leads.create_list" => {
            boxed_dispatch(|| dispatch_leads_create_list(&state, &user, &body.input)).await
        }
        "leads.delete_list" => {
            boxed_dispatch(|| dispatch_leads_delete_list(&state, &user, &body.input)).await
        }
        "finetune.create_job" => {
            boxed_dispatch(|| dispatch_finetune_create_job(&state, &user, &body.input)).await
        }
        "finetune.cancel_job" => {
            boxed_dispatch(|| dispatch_finetune_cancel_job(&state, &user, &body.input)).await
        }
        "finetune.deploy_job" => {
            boxed_dispatch(|| dispatch_finetune_deploy_job(&state, &user, &body.input)).await
        }
        "studio.create_project" => {
            boxed_dispatch(|| dispatch_studio_create_project(&state, &user, &body.input)).await
        }
        "studio.save_project" => {
            boxed_dispatch(|| dispatch_studio_save_project(&state, &user, &body.input)).await
        }
        "studio.export_social_draft" => {
            boxed_dispatch(|| dispatch_studio_export_social_draft(&state, &user, &body.input)).await
        }
        "ingestions.create_run" => {
            boxed_dispatch(|| dispatch_ingestions_create_run(&state, &user, &headers, &body.input))
                .await
        }
        "ingestions.create_schedule" => {
            boxed_dispatch(|| {
                dispatch_ingestions_create_schedule(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ingestions.pause_schedule" => {
            boxed_dispatch(|| {
                dispatch_ingestions_pause_schedule(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ingestions.change_schedule_state" => {
            boxed_dispatch(|| {
                dispatch_ingestions_change_schedule_state(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ingestions.get_run_status" => {
            boxed_dispatch(|| {
                dispatch_ingestions_get_run_status(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ingestions.pause_run" => {
            boxed_dispatch(|| dispatch_ingestions_pause_run(&state, &user, &headers, &body.input))
                .await
        }
        "ingestions.resume_run" => {
            boxed_dispatch(|| dispatch_ingestions_resume_run(&state, &user, &headers, &body.input))
                .await
        }
        "ingestions.cancel_run" => {
            boxed_dispatch(|| dispatch_ingestions_cancel_run(&state, &user, &headers, &body.input))
                .await
        }
        "ingestions.create_source" => {
            boxed_dispatch(|| {
                dispatch_ingestions_create_source(&state, &user, &headers, &body.input)
            })
            .await
        }
        "ingestions.delete_source" => {
            boxed_dispatch(|| {
                dispatch_ingestions_delete_source(&state, &user, &headers, &body.input)
            })
            .await
        }
        "integrations.start_connect_session" => {
            boxed_dispatch(|| {
                dispatch_integrations_start_connect_session(&state, &user, &headers, &body.input)
            })
            .await
        }
        "integrations.disconnect" => {
            boxed_dispatch(|| {
                dispatch_integrations_disconnect(&state, &user, &headers, &body.input)
            })
            .await
        }
        "integrations.start_chatgpt_subscription" => {
            boxed_dispatch(|| {
                dispatch_integrations_start_chatgpt_subscription(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "integrations.disconnect_chatgpt_subscription" => {
            boxed_dispatch(|| {
                dispatch_integrations_disconnect_chatgpt_subscription(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "integrations.trigger_sync" => {
            boxed_dispatch(|| {
                dispatch_integrations_trigger_sync(&state, &user, &headers, &body.input)
            })
            .await
        }
        "integrations.trigger_inbox_sync" => {
            boxed_dispatch(|| {
                dispatch_integrations_trigger_inbox_sync(&state, &user, &headers, &body.input)
            })
            .await
        }
        "integrations.extend_inbox_history" => {
            boxed_dispatch(|| {
                dispatch_integrations_extend_inbox_history(&state, &user, &headers, &body.input)
            })
            .await
        }
        "org.soft_delete" => {
            boxed_dispatch(|| dispatch_org_soft_delete(&state, &user, &body.input)).await
        }
        "org.restore" => boxed_dispatch(|| dispatch_org_restore(&state, &user, &body.input)).await,
        "org.mark_exported" => {
            boxed_dispatch(|| dispatch_org_mark_exported(&state, &user, &body.input)).await
        }
        "org.acknowledge_deletion" => {
            boxed_dispatch(|| dispatch_org_acknowledge_deletion(&state, &user, &body.input)).await
        }
        "org.set_quota" => {
            boxed_dispatch(|| dispatch_org_set_quota(&state, &user, &body.input)).await
        }
        "org.set_model_entitlement" => {
            boxed_dispatch(|| {
                dispatch_org_set_model_entitlement(&state, &user, &headers, &body.input)
            })
            .await
        }
        "org.update_instructions" => {
            boxed_dispatch(|| dispatch_org_update_instructions(&state, &user, &body.input)).await
        }
        "org.update_zdr" => {
            boxed_dispatch(|| dispatch_org_update_zdr(&state, &user, &headers, &body.input)).await
        }
        "org.update_support_ai_mode" => {
            boxed_dispatch(|| {
                dispatch_org_update_support_ai_mode(&state, &user, &headers, &body.input)
            })
            .await
        }
        "org.update_agent_learning_policy" => {
            boxed_dispatch(|| {
                dispatch_org_update_agent_learning_policy(&state, &user, &headers, &body.input)
            })
            .await
        }
        "membership.invite_member" => {
            boxed_dispatch(|| {
                dispatch_membership_invite_member(&state, &user, &headers, &body.input)
            })
            .await
        }
        "membership.remove_member" => {
            boxed_dispatch(|| {
                dispatch_membership_remove_member(&state, &user, &headers, &body.input)
            })
            .await
        }
        "membership.update_member_role" => {
            boxed_dispatch(|| {
                dispatch_membership_update_member_role(&state, &user, &headers, &body.input)
            })
            .await
        }
        "organization.switch_active" => {
            boxed_dispatch(|| {
                dispatch_organization_switch_active(&state, &user, &headers, &body.input)
            })
            .await
        }
        "chat.approve_plan" => {
            boxed_dispatch(|| dispatch_chat_approve_plan(&state, &user, &headers, &body.input))
                .await
        }
        "chat.cancel_invocation" => {
            boxed_dispatch(|| dispatch_chat_cancel_invocation(&state, &user, &headers, &body.input))
                .await
        }
        "chat.research_run.cancel" => {
            boxed_dispatch(|| {
                dispatch_chat_cancel_research_run(&state, &user, &headers, &body.input)
            })
            .await
        }
        "chat.queue_invocation_input" => {
            boxed_dispatch(|| {
                dispatch_chat_queue_invocation_input(&state, &user, &headers, &body.input)
            })
            .await
        }
        "chat.clear_threads" => {
            boxed_dispatch(|| dispatch_chat_clear_threads(&state, &user, &headers, &body.input))
                .await
        }
        "chat.delete_thread" => {
            boxed_dispatch(|| dispatch_chat_delete_thread(&state, &user, &headers, &body.input))
                .await
        }
        "chat.save_thread_snapshot" => {
            boxed_dispatch(|| {
                dispatch_chat_save_thread_snapshot(&state, &user, &headers, &body.input)
            })
            .await
        }
        "chat.submit_feedback" => {
            boxed_dispatch(|| dispatch_chat_submit_feedback(&state, &user, &headers, &body.input))
                .await
        }
        "chat.upload_document" => {
            boxed_dispatch(|| dispatch_chat_upload_document(&state, &user, &headers, &body.input))
                .await
        }
        "chat.extract_document" => {
            boxed_dispatch(|| dispatch_chat_extract_document(&state, &user, &headers, &body.input))
                .await
        }
        "audio.transcribe" => {
            boxed_dispatch(|| dispatch_audio_transcribe(&state, &user, &headers, &body.input)).await
        }
        "audio.dictate" => {
            boxed_dispatch(|| dispatch_audio_dictate(&state, &user, &headers, &body.input)).await
        }
        "orchestration.decide_approval" => {
            boxed_dispatch(|| {
                dispatch_orchestration_decide_approval(&state, &user, &headers, &body.input)
            })
            .await
        }
        "orchestration.resume_run" => {
            boxed_dispatch(|| {
                dispatch_orchestration_resume_run(&state, &user, &headers, &body.input)
            })
            .await
        }
        "orchestration.cancel_run" => {
            boxed_dispatch(|| {
                dispatch_orchestration_cancel_run(&state, &user, &headers, &body.input)
            })
            .await
        }
        "browser_run.start" => {
            boxed_dispatch(|| {
                dispatch_browser_run_start(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "browser_run.control" => {
            boxed_dispatch(|| dispatch_browser_run_control(&state, &user, &headers, &body.input))
                .await
        }
        "browser.blocked.try_other_source" => {
            boxed_dispatch(|| {
                dispatch_browser_blocked_try_other_source(&state, &user, &headers, &body.input)
            })
            .await
        }
        "router_policy.update" => {
            boxed_dispatch(|| dispatch_router_policy_update(&state, &user, &body.input)).await
        }
        "browser_session.create" => {
            boxed_dispatch(|| dispatch_browser_create_session(&state, &user, &headers, &body.input))
                .await
        }
        "browser_session.close" => {
            boxed_dispatch(|| dispatch_browser_close_session(&state, &user, &headers, &body.input))
                .await
        }
        "browser_session.keepalive" => {
            boxed_dispatch(|| {
                dispatch_browser_keepalive_session(&state, &user, &headers, &body.input)
            })
            .await
        }
        "browser_tab.create" => {
            boxed_dispatch(|| dispatch_browser_create_tab(&state, &user, &headers, &body.input))
                .await
        }
        "browser_tab.select" => {
            boxed_dispatch(|| dispatch_browser_select_tab(&state, &user, &headers, &body.input))
                .await
        }
        "browser_tab.close" => {
            boxed_dispatch(|| dispatch_browser_close_tab(&state, &user, &headers, &body.input))
                .await
        }
        "browser_tab.control" => {
            boxed_dispatch(|| dispatch_browser_control_tab(&state, &user, &headers, &body.input))
                .await
        }
        "browser_attach.stop" => {
            boxed_dispatch(|| dispatch_browser_stop_attach(&state, &user, &headers, &body.input))
                .await
        }
        "browser_action.run" => {
            boxed_dispatch(|| dispatch_browser_run_action(&state, &user, &headers, &body.input))
                .await
        }
        "browser_action.set_control_mode" => {
            boxed_dispatch(|| {
                dispatch_browser_set_control_mode(&state, &user, &headers, &body.input)
            })
            .await
        }
        "browser_action.suggest" => {
            boxed_dispatch(|| dispatch_browser_suggest_action(&state, &user, &headers, &body.input))
                .await
        }
        "browser_omnibox.search" => {
            boxed_dispatch(|| dispatch_browser_omnibox_search(&state, &user, &headers, &body.input))
                .await
        }
        "browser_omnibox.suggest" => {
            boxed_dispatch(|| {
                dispatch_browser_omnibox_suggest(&state, &user, &headers, &body.input)
            })
            .await
        }
        "browser_evidence.compile_procedure" => {
            boxed_dispatch(|| {
                crate::domains::browser_evidence::dispatch_compile_procedure(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "browser_evidence.save_recipe" => {
            boxed_dispatch(|| {
                crate::domains::browser_evidence::dispatch_save_recipe(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "browser_region.attach" => {
            boxed_dispatch(|| {
                crate::domains::browser_evidence::dispatch_region_attach(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "browser_context.resolve_tab_mentions" => {
            boxed_dispatch(|| {
                crate::domains::browser_context::dispatch_resolve_tab_mentions(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                )
            })
            .await
        }
        "browser_profile.create" => {
            boxed_dispatch(|| dispatch_browser_create_profile(&state, &user, &headers, &body.input))
                .await
        }
        "browser_profile.rename" => {
            boxed_dispatch(|| dispatch_browser_rename_profile(&state, &user, &headers, &body.input))
                .await
        }
        "browser_profile.delete" => {
            boxed_dispatch(|| dispatch_browser_delete_profile(&state, &user, &headers, &body.input))
                .await
        }
        "browser_profile.probe_restore" => {
            boxed_dispatch(|| {
                dispatch_browser_probe_profile_restore(&state, &user, &headers, &body.input)
            })
            .await
        }
        "tickets.record_csat_outcome" => {
            boxed_dispatch(|| dispatch_ticket_record_csat_outcome(&state, &user, &body.input)).await
        }
        "tickets.record_chat_handoff" => {
            boxed_dispatch(|| dispatch_ticket_record_chat_handoff(&state, &user, &body.input)).await
        }
        // Ticketing actions -> conversation-core-go (the same backend the dedicated
        // /api/v1/tickets/* routes proxy to), scoped to the caller's org.
        "tickets.create" => {
            boxed_dispatch(|| {
                dispatch_ticket_create(&state, &user, &body.input, &body.idempotency_key)
            })
            .await
        }
        "tickets.classify_conversation" => {
            boxed_dispatch(|| dispatch_ticket_classify(&state, &user, &body.input)).await
        }
        "tickets.update" => {
            boxed_dispatch(|| dispatch_ticket_update(&state, &user, &body.input)).await
        }
        "tickets.assign" => {
            boxed_dispatch(|| dispatch_ticket_assign(&state, &user, &body.input)).await
        }
        "tickets.link_resource" => {
            boxed_dispatch(|| dispatch_ticket_link_resource(&state, &user, &body.input)).await
        }
        "tickets.resolve" => {
            boxed_dispatch(|| dispatch_ticket_resolve(&state, &user, &body.input)).await
        }
        "tickets.run_macro" => {
            boxed_dispatch(|| dispatch_ticket_run_macro(&state, &user, &body.input)).await
        }
        "tickets.create_macro" => {
            boxed_dispatch(|| dispatch_ticket_create_macro(&state, &user, &body.input)).await
        }
        "tickets.create_checklist" => {
            boxed_dispatch(|| dispatch_ticket_create_checklist(&state, &user, &body.input)).await
        }
        "tickets.update_checklist_item" => {
            boxed_dispatch(|| dispatch_ticket_update_checklist_item(&state, &user, &body.input))
                .await
        }
        "tickets.create_side_conversation" => {
            boxed_dispatch(|| dispatch_ticket_create_side_conversation(&state, &user, &body.input))
                .await
        }
        "tickets.add_side_conversation_message" => {
            boxed_dispatch(|| {
                dispatch_ticket_add_side_conversation_message(&state, &user, &body.input)
            })
            .await
        }
        "tickets.update_side_conversation" => {
            boxed_dispatch(|| dispatch_ticket_update_side_conversation(&state, &user, &body.input))
                .await
        }
        "tickets.create_incident" => {
            boxed_dispatch(|| dispatch_ticket_create_incident(&state, &user, &body.input)).await
        }
        "tickets.update_incident" => {
            boxed_dispatch(|| dispatch_ticket_update_incident(&state, &user, &body.input)).await
        }
        "tickets.link_incident_ticket" => {
            boxed_dispatch(|| dispatch_ticket_link_incident_ticket(&state, &user, &body.input))
                .await
        }
        "tickets.create_problem" => {
            boxed_dispatch(|| dispatch_ticket_create_problem(&state, &user, &body.input)).await
        }
        "tickets.update_problem" => {
            boxed_dispatch(|| dispatch_ticket_update_problem(&state, &user, &body.input)).await
        }
        "tickets.create_sla_policy" => {
            boxed_dispatch(|| dispatch_ticket_create_sla_policy(&state, &user, &body.input)).await
        }
        "tickets.update_sla_policy" => {
            boxed_dispatch(|| dispatch_ticket_update_sla_policy(&state, &user, &body.input)).await
        }
        "tickets.create_automation_rule" => {
            boxed_dispatch(|| dispatch_ticket_create_automation_rule(&state, &user, &body.input))
                .await
        }
        "tickets.update_automation_rule" => {
            boxed_dispatch(|| dispatch_ticket_update_automation_rule(&state, &user, &body.input))
                .await
        }
        "tickets.create_team" => {
            boxed_dispatch(|| dispatch_ticket_create_team(&state, &user, &body.input)).await
        }
        "tickets.update_team" => {
            boxed_dispatch(|| dispatch_ticket_update_team(&state, &user, &body.input)).await
        }
        "tickets.create_view" => {
            boxed_dispatch(|| dispatch_ticket_create_view(&state, &user, &body.input)).await
        }
        "tickets.update_view" => {
            boxed_dispatch(|| dispatch_ticket_update_view(&state, &user, &body.input)).await
        }
        "tickets.update_macro" => {
            boxed_dispatch(|| dispatch_ticket_update_macro(&state, &user, &body.input)).await
        }
        // Space actions call the same route handlers domains::spaces mounts for
        // its direct REST routes (not a parallel proxy), so a per-room grant
        // check or the create/bind-then-confirm-membership sequence can never
        // drift between the two callers.
        "spaces.create_personal_space" => {
            boxed_dispatch(|| dispatch_space_create_personal(&state, &user, &body.input)).await
        }
        "spaces.ensure_organization_room" => {
            boxed_dispatch(|| dispatch_space_ensure_organization_room(&state, &user, &body.input))
                .await
        }
        "spaces.update_space_instructions" => {
            boxed_dispatch(|| dispatch_space_update_instructions(&state, &user, &body.input)).await
        }
        "spaces.create_space_agent" => {
            boxed_dispatch(|| dispatch_space_create_agent(&state, &user, &body.input)).await
        }
        "spaces.bind_space_agent" => {
            boxed_dispatch(|| dispatch_space_bind_agent(&state, &user, &body.input)).await
        }
        "spaces.request_personal_space_deletion" => {
            boxed_dispatch(|| dispatch_space_request_personal_deletion(&state, &user, &body.input))
                .await
        }
        "spaces.update_thread_presentation" => {
            boxed_dispatch(|| {
                dispatch_space_update_thread_presentation(&state, &user, &headers, &body.input)
            })
            .await
        }
        "spaces.mark_read" => {
            boxed_dispatch(|| dispatch_space_mark_read(&state, &user, &body.input)).await
        }
        "spaces.record_presence" => {
            boxed_dispatch(|| dispatch_space_record_presence(&state, &user, &body.input)).await
        }
        "spaces.set_agent_state" => {
            boxed_dispatch(|| dispatch_space_set_agent_state(&state, &user, &body.input)).await
        }
        "spaces.revoke_agent" => {
            boxed_dispatch(|| {
                dispatch_space_revoke_agent(&state, &user, &headers, &body.input)
            })
            .await
        }
        "spaces.create_room" => {
            boxed_dispatch(|| dispatch_space_create_room(&state, &user, &body.input)).await
        }
        "spaces.add_member" => {
            boxed_dispatch(|| dispatch_space_add_member(&state, &user, &body.input)).await
        }
        "spaces.remove_member" => {
            boxed_dispatch(|| dispatch_space_remove_member(&state, &user, &body.input)).await
        }
        "spaces.create_post" => {
            boxed_dispatch(|| dispatch_spaces_create_post(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.reply_in_thread" => {
            boxed_dispatch(|| dispatch_spaces_reply_in_thread(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.change_member_role" => {
            boxed_dispatch(|| {
                dispatch_spaces_change_member_role(
                    &state,
                    &user,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "spaces.leave_room" => {
            boxed_dispatch(|| dispatch_spaces_leave_room(&state, &user, &body.input)).await
        }
        "spaces.open_dm" => {
            boxed_dispatch(|| dispatch_spaces_open_dm(&state, &user, &headers, &body.input)).await
        }
        "spaces.hide_space" => {
            boxed_dispatch(|| dispatch_spaces_hide_space(&state, &user, &body.input)).await
        }
        "spaces.edit_message" => {
            boxed_dispatch(|| dispatch_spaces_edit_message(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.delete_message" => {
            boxed_dispatch(|| dispatch_spaces_delete_message(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.toggle_reaction" => {
            boxed_dispatch(|| dispatch_spaces_toggle_reaction(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.update_sidebar" => {
            boxed_dispatch(|| dispatch_spaces_update_sidebar(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.set_agent_wake_policy" => {
            boxed_dispatch(|| dispatch_spaces_set_agent_wake_policy(&state, &user, &body.input))
                .await
        }
        "spaces.agent_binding.pin" => {
            boxed_dispatch(|| dispatch_spaces_set_agent_pin(&state, &user, &body.input)).await
        }
        "spaces.decide_approval" => {
            boxed_dispatch(|| {
                dispatch_spaces_decide_approval(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "spaces.revoke_approval_grant" => {
            boxed_dispatch(|| dispatch_spaces_revoke_approval_grant(&state, &user, &body.input))
                .await
        }
        "spaces.set_tool_policy" => {
            boxed_dispatch(|| dispatch_spaces_set_tool_policy(&state, &user, &body.input)).await
        }
        "spaces.memory.list" => {
            boxed_dispatch(|| {
                dispatch_spaces_memory_list(&state, &user, &headers, &body.input)
            })
            .await
        }
        "spaces.memory.delete" => {
            boxed_dispatch(|| {
                dispatch_spaces_memory_delete(&state, &user, &headers, &body.input)
            })
            .await
        }
        "spaces.set_attention_item_state" => {
            boxed_dispatch(|| dispatch_spaces_set_attention_item_state(&state, &user, &body.input))
                .await
        }
        "spaces.stop_run" => {
            boxed_dispatch(|| dispatch_spaces_stop_run(&state, &user, &headers, &body.input)).await
        }
        "spaces.set_thread_follow" => {
            boxed_dispatch(|| {
                dispatch_spaces_set_thread_follow(&state, &user, &headers, &body.input)
            })
            .await
        }
        "spaces.mark_all_read" => {
            boxed_dispatch(|| dispatch_spaces_mark_all_read(&state, &user, &body.input)).await
        }
        "spaces.mark_unread" => {
            boxed_dispatch(|| dispatch_spaces_mark_unread(&state, &user, &body.input)).await
        }
        "spaces.set_notify_level" => {
            boxed_dispatch(|| dispatch_spaces_set_notify_level(&state, &user, &body.input)).await
        }
        "spaces.set_status" => {
            boxed_dispatch(|| dispatch_spaces_set_status(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.create_reminder" => {
            boxed_dispatch(|| dispatch_spaces_create_reminder(&state, &user, &headers, &body.input))
                .await
        }
        "spaces.cancel_reminder" => {
            boxed_dispatch(|| dispatch_spaces_cancel_reminder(&state, &user, &body.input)).await
        }
        "spaces.update_room_details" => {
            boxed_dispatch(|| dispatch_spaces_update_room_details(&state, &user, &body.input)).await
        }
        "spaces.join_room" => {
            boxed_dispatch(|| {
                dispatch_spaces_join_room(&state, &user, &body.input, &body.idempotency_key)
            })
            .await
        }
        "spaces.update_room_settings" => {
            boxed_dispatch(|| {
                dispatch_spaces_update_room_settings(
                    &state,
                    &user,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "spaces.set_effect_entitlements" => {
            boxed_dispatch(|| {
                dispatch_spaces_set_effect_entitlements(
                    &state,
                    &user,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "spaces.apply_effect_preset" => {
            boxed_dispatch(|| {
                dispatch_spaces_apply_effect_preset(
                    &state,
                    &user,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "browser.get_org_policy" => {
            boxed_dispatch(|| dispatch_browser_get_org_policy(&state, &user, &body.input)).await
        }
        "browser.set_org_policy" => {
            boxed_dispatch(|| dispatch_browser_set_org_policy(&state, &user, &body.input)).await
        }
        "browser.list_origin_consents" => {
            boxed_dispatch(|| dispatch_browser_list_origin_consents(&state, &user)).await
        }
        "browser.revoke_origin_consent" => {
            boxed_dispatch(|| dispatch_browser_revoke_origin_consent(&state, &user, &body.input))
                .await
        }
        "space.files.attach" => boxed_dispatch(dispatch_space_files_attach).await,
        "space.files.list" => {
            boxed_dispatch(|| dispatch_space_files_list(&state, &user, &headers, &body.input)).await
        }
        "space.files.read_text" => {
            boxed_dispatch(|| dispatch_space_files_read_text(&state, &user, &headers, &body.input))
                .await
        }
        "space.files.delete" => {
            boxed_dispatch(|| dispatch_space_files_delete(&state, &user, &headers, &body.input))
                .await
        }
        // Social actions -> social-core via the dedicated dispatchers (real
        // ApprovalState is enforced by social-core before publish/schedule).
        "social.create_draft" => {
            boxed_dispatch(|| dispatch_social_create_draft(&state, &user, &body.input)).await
        }
        "social.schedule_post" => {
            boxed_dispatch(|| dispatch_social_schedule_post(&state, &user, &body.input)).await
        }
        "social.publish_post" => {
            boxed_dispatch(|| dispatch_social_publish_post(&state, &user, &body.input)).await
        }
        "social.create_campaign" => {
            boxed_dispatch(|| dispatch_social_create_campaign(&state, &user, &body.input)).await
        }
        "social.create_draft_from_inbox" => {
            boxed_dispatch(|| dispatch_social_create_draft_from_inbox(&state, &user, &body.input))
                .await
        }
        "social.decide_approval" => {
            boxed_dispatch(|| dispatch_social_decide_approval(&state, &user, &body.input)).await
        }
        // FU-02 (D-F05-5): every id in the action registry has an arm here, so the action-surface
        // contract holds for all of them. These ids are not run through this endpoint by the
        // browser: each one runs on its own REST route or inside the page. The arm answers a typed
        // refusal that names that route and does nothing (`client_executed`); the two ids the
        // browser does send here but whose owner path is not wired yet answer `owner_not_wired`.
        // `agents.admitted_actions.list` is a real arm: it reads the same answer as its route.
        "memory.profile.save" => {
            boxed_dispatch(|| {
                dispatch_client_executed("memory.profile.save", "/api/v1/memory/profile")
            })
            .await
        }
        "memory.profile.rebuild" => {
            boxed_dispatch(|| {
                dispatch_client_executed("memory.profile.rebuild", "/api/v1/memory/profile/rebuild")
            })
            .await
        }
        "spaces.agent_memory.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "spaces.agent_memory.list",
                        "/api/v1/spaces/{space_ref}/memory",
                )
            })
            .await
        }
        "spaces.agent_memory.delete" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "spaces.agent_memory.delete",
                        "/api/v1/spaces/{space_ref}/memory/{memory_id}",
                )
            })
            .await
        }
        "skills.save_from_message" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.save_from_message", "/api/v1/skills")
            })
            .await
        }
        "skills.versions.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.versions.list", "/api/v1/skills/{skill_id}/versions")
            })
            .await
        }
        "skills.draft.save" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.draft.save", "/api/v1/skills/{skill_id}/draft")
            })
            .await
        }
        "skills.publish" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.publish", "/api/v1/skills/{skill_id}/publish")
            })
            .await
        }
        "skills.draft.discard" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.draft.discard", "/api/v1/skills/{skill_id}/draft")
            })
            .await
        }
        "skills.version.restore" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "skills.version.restore",
                        "/api/v1/skills/{skill_id}/versions/{version}/restore",
                )
            })
            .await
        }
        "skills.import" => {
            boxed_dispatch(|| {
                dispatch_client_executed("skills.import", "/api/v1/skills/import")
            })
            .await
        }
        "templates.create" => {
            boxed_dispatch(|| dispatch_client_executed("templates.create", "/api/v1/skills")).await
        }
        "templates.update" => {
            boxed_dispatch(|| {
                dispatch_client_executed("templates.update", "/api/v1/skills/{skill_id}")
            })
            .await
        }
        "presets.create" => {
            boxed_dispatch(|| dispatch_client_executed("presets.create", "/api/v1/skills")).await
        }
        "presets.update" => {
            boxed_dispatch(|| {
                dispatch_client_executed("presets.update", "/api/v1/skills/{skill_id}")
            })
            .await
        }
        "mcp.probe_server" => {
            boxed_dispatch(|| {
                dispatch_client_executed("mcp.probe_server", "/api/v1/mcp/servers/probe")
            })
            .await
        }
        "mcp.approve_tool" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "mcp.approve_tool",
                        "/api/v1/mcp/servers/{server_id}/tools/{tool}/approve",
                )
            })
            .await
        }
        "tools.set_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed("tools.set_policy", "/api/v1/tool-policies/{capability_id}")
            })
            .await
        }
        "org.set_agent_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.set_agent_policy", "/api/v1/org/agent-policy")
            })
            .await
        }
        "org.set_web_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.set_web_policy", "/api/v1/org/agent-policy")
            })
            .await
        }
        "org.preview_effective_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.preview_effective_policy",
                        "/api/v1/org/agent-policy/effective",
                )
            })
            .await
        }
        "policy.read_effective" => {
            boxed_dispatch(|| {
                dispatch_client_executed("policy.read_effective", "/api/v1/policy/effective")
            })
            .await
        }
        "org.create_hook" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.create_hook", "/api/v1/org/hooks")
            })
            .await
        }
        "org.update_hook" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.update_hook", "/api/v1/org/hooks/{hook_id}")
            })
            .await
        }
        "org.delete_hook" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.delete_hook", "/api/v1/org/hooks/{hook_id}")
            })
            .await
        }
        "org.list_permission_rules" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.list_permission_rules", "/api/v1/org/permission-rules")
            })
            .await
        }
        "org.create_permission_rule" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.create_permission_rule", "/api/v1/org/permission-rules")
            })
            .await
        }
        "org.delete_permission_rule" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.delete_permission_rule",
                        "/api/v1/org/permission-rules/{rule_id}",
                )
            })
            .await
        }
        "integrations.browse_connection" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "integrations.browse_connection",
                        "/api/v1/integrations/connections/{id}/browse",
                )
            })
            .await
        }
        "integrations.fetch_connection_content" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "integrations.fetch_connection_content",
                        "/api/v1/integrations/connections/{id}/content",
                )
            })
            .await
        }
        "integrations.list_connector_policies" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "integrations.list_connector_policies",
                        "/api/v1/tool-policies",
                )
            })
            .await
        }
        "integrations.set_connector_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "integrations.set_connector_policy",
                        "/api/v1/tool-policies/{capability_id}",
                )
            })
            .await
        }
        "org.set_retention" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.set_retention", "/api/v1/orgs/{id}/retention")
            })
            .await
        }
        "org.place_legal_hold" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.place_legal_hold", "/api/v1/orgs/{id}/legal-holds")
            })
            .await
        }
        "org.release_legal_hold" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.release_legal_hold",
                        "/api/v1/orgs/{id}/legal-holds/{hold_id}/release",
                )
            })
            .await
        }
        "org.add_domain" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.add_domain", "/api/v1/orgs/{id}/domains")
            })
            .await
        }
        "org.verify_domain" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.verify_domain",
                        "/api/v1/orgs/{id}/domains/{domain}/verify",
                )
            })
            .await
        }
        "org.revoke_domain" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.revoke_domain", "/api/v1/orgs/{id}/domains/{domain}")
            })
            .await
        }
        "org.update_sso_policy" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.update_sso_policy", "/api/v1/orgs/{id}/sso/policy")
            })
            .await
        }
        "org.add_sso_provider" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.add_sso_provider", "/api/v1/orgs/{id}/sso/providers")
            })
            .await
        }
        "org.remove_sso_provider" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.remove_sso_provider",
                        "/api/v1/orgs/{id}/sso/providers/{provider_id}",
                )
            })
            .await
        }
        "org.create_scim_token" => {
            boxed_dispatch(|| {
                dispatch_client_executed("org.create_scim_token", "/api/v1/orgs/{id}/scim/tokens")
            })
            .await
        }
        "org.revoke_scim_token" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "org.revoke_scim_token",
                        "/api/v1/orgs/{id}/scim/tokens/{token_id}",
                )
            })
            .await
        }
        "chat.execute_approved_plan" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.execute_approved_plan",
                        "/api/v1/agents/runs/{run_id}/plan-approval",
                )
            })
            .await
        }
        "chat.run.continue" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.run.continue",
                        "/api/v1/orchestration/runs/{run_id}/continue",
                )
            })
            .await
        }
        "chat.answer_question" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.answer_question",
                        "/api/v1/orchestration/questions/{question_id}/answer",
                )
            })
            .await
        }
        "approvals.decide" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.decide",
                        "/api/v1/orchestration/approvals/{approval_id}/decide",
                )
            })
            .await
        }
        "approvals.preview" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.preview",
                        "/api/v1/orchestration/approvals/{approval_id}/preview",
                )
            })
            .await
        }
        "approvals.list_grants" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.list_grants",
                        "/api/v1/orchestration/threads/{thread_id}/approval-grants",
                )
            })
            .await
        }
        "approvals.revoke_grant" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.revoke_grant",
                        "/api/v1/orchestration/approval-grants/{grant_id}",
                )
            })
            .await
        }
        "approvals.list_rule_proposals" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.list_rule_proposals",
                        "/api/v1/orchestration/rule-proposals",
                )
            })
            .await
        }
        "approvals.confirm_rule" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.confirm_rule",
                        "/api/v1/orchestration/rule-proposals/{id}/confirm",
                )
            })
            .await
        }
        "approvals.dismiss_rule" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "approvals.dismiss_rule",
                        "/api/v1/orchestration/rule-proposals/{id}/dismiss",
                )
            })
            .await
        }
        "chat.mark_thread_seen" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.mark_thread_seen", "/api/v1/attention/seen")
            })
            .await
        }
        "chat.thread.archive" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.thread.archive",
                        "/api/v1/chat/threads/{thread_id}/archive",
                )
            })
            .await
        }
        "chat.thread.star" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.thread.star", "/api/v1/chat/threads/{thread_id}/flags")
            })
            .await
        }
        "chat.thread.mark_unread" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.thread.mark_unread",
                        "/api/v1/chat/threads/{thread_id}/flags",
                )
            })
            .await
        }
        "thread.export" => {
            boxed_dispatch(|| {
                dispatch_client_executed("thread.export", "/api/v1/chat/threads/{thread_id}/export")
            })
            .await
        }
        "thread.share_snapshot" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "thread.share_snapshot",
                        "/api/v1/chat/threads/{thread_id}/snapshots",
                )
            })
            .await
        }
        "thread.revoke_snapshot" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "thread.revoke_snapshot",
                        "/api/v1/chat/snapshots/{snapshot_id}",
                )
            })
            .await
        }
        "thread.grant_snapshot" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "thread.grant_snapshot",
                        "/api/v1/chat/snapshots/{snapshot_id}/grants",
                )
            })
            .await
        }
        "thread.import" => {
            boxed_dispatch(|| {
                dispatch_client_executed("thread.import", "/api/v1/chat/threads/import")
            })
            .await
        }
        "chat.save_artifact_revision" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.save_artifact_revision",
                        "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}",
                )
            })
            .await
        }
        "chat.restore_artifact_revision" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.restore_artifact_revision",
                        "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}/restore",
                )
            })
            .await
        }
        "chat.edit_artifact_selection" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.edit_artifact_selection", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.save_artifact_to_knowledge" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.save_artifact_to_knowledge", "/api/v1/chat/documents")
            })
            .await
        }
        "library.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed("library.list", "/api/v1/chat/library")
            })
            .await
        }
        "library.read_text" => {
            boxed_dispatch(|| {
                dispatch_client_executed("library.read_text", "/api/v1/chat/library")
            })
            .await
        }
        "library.open_origin" => {
            boxed_dispatch(|| {
                dispatch_client_executed("library.open_origin", "ui:chat-library-panel")
            })
            .await
        }
        "library.share" => {
            boxed_dispatch(|| {
                dispatch_client_executed("library.share", "/api/v1/chat/library")
            })
            .await
        }
        "review.run" => {
            boxed_dispatch(|| dispatch_client_executed("review.run", "/api/v1/ai/review")).await
        }
        "connection_zdr.read" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connection_zdr.read",
                        "/api/v1/model-connections/{connection_id}/zdr",
                )
            })
            .await
        }
        "connection_zdr.acknowledge" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connection_zdr.acknowledge",
                        "/api/v1/model-connections/{connection_id}/zdr-acknowledgement",
                )
            })
            .await
        }
        "connection_zdr.revoke" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connection_zdr.revoke",
                        "/api/v1/model-connections/{connection_id}/zdr-acknowledgement",
                )
            })
            .await
        }
        "chat.set_connector_scope" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.set_connector_scope", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.set_context_refs" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.set_context_refs", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.pick_context_ref" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.pick_context_ref", "ui:composer-context-picker")
            })
            .await
        }
        "chat.set_agent_mode" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.set_agent_mode", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.set_transcript_density" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.set_transcript_density", "/api/v1/settings/appearance")
            })
            .await
        }
        "chat.inspect_run_request" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.inspect_run_request",
                        "/api/v1/agents/runs/{run_id}/requests/{call_seq}",
                )
            })
            .await
        }
        "chat.open_cited_passage" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.open_cited_passage", "ui:cited-passage-viewer")
            })
            .await
        }
        "chat.list_artifact_revisions" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.list_artifact_revisions",
                        "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}/revisions",
                )
            })
            .await
        }
        "chat.wide_research_start" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.wide_research_start", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.research_plan_start" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.research_plan_start", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.research_plan_cancel" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.research_plan_cancel", "ui:research-plan-card")
            })
            .await
        }
        "chat.accept_connector_suggestion" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.accept_connector_suggestion",
                        "ui:connector-suggestion-card",
                )
            })
            .await
        }
        "chat.wide_research_retry_item" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.wide_research_retry_item", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.list_deck_templates" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.list_deck_templates", "/api/v1/deck-templates")
            })
            .await
        }
        "chat.apply_deck_template" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.apply_deck_template", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.open_deck_slide" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.open_deck_slide", "ui:deck-slide-strip")
            })
            .await
        }
        "chat.present_deck" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.present_deck", "ui:deck-present-mode")
            })
            .await
        }
        "chat.set_goal" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.set_goal", "/api/v1/chat/threads/{thread_id}/goal")
            })
            .await
        }
        "chat.update_goal" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.update_goal", "/api/v1/chat/threads/{thread_id}/goal")
            })
            .await
        }
        "chat.clear_goal" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.clear_goal", "/api/v1/chat/threads/{thread_id}/goal")
            })
            .await
        }
        "chat.continue_goal" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.continue_goal", "/api/v1/chat/stream")
            })
            .await
        }
        "chat.goal_auto_toggle" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.goal_auto_toggle", "ui:goal-bar")
            })
            .await
        }
        "chat.goal_mark_done" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "chat.goal_mark_done",
                        "/api/v1/chat/threads/{thread_id}/goal",
                )
            })
            .await
        }
        "chat.goal_reopen" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.goal_reopen", "/api/v1/chat/threads/{thread_id}/goal")
            })
            .await
        }
        "chat.goal_away_start" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.goal_away_start", "/api/v1/cron")
            })
            .await
        }
        "chat.stage_widget_followup" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.stage_widget_followup", "ui:inline-widget-bridge")
            })
            .await
        }
        "chat.stage_widget_selection" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.stage_widget_selection", "ui:inline-widget-bridge")
            })
            .await
        }
        "chat.open_widget_link" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.open_widget_link", "ui:inline-widget-bridge")
            })
            .await
        }
        "chat.open_mcp_app" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.open_mcp_app", "/api/v1/mcp-apps/resource")
            })
            .await
        }
        "chat.stage_mcp_app_message" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.stage_mcp_app_message", "ui:mcp-app-host")
            })
            .await
        }
        "chat.mcp_app_tool_call" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.mcp_app_tool_call", "/api/v1/mcp-apps/tool-call")
            })
            .await
        }
        "chat.open_computer_viewer" => {
            boxed_dispatch(|| {
                dispatch_client_executed("chat.open_computer_viewer", "ui:computer-panel")
            })
            .await
        }
        "browser_run.steer" => {
            boxed_dispatch(|| dispatch_browser_run_steer(&state, &user, &headers, &body.input))
                .await
        }
        "browser_download.send_to_knowledge" => {
            boxed_dispatch(|| {
                dispatch_browser_download_send_to_knowledge(
                    &state,
                    &user,
                    &headers,
                    &body.input,
                    &body.idempotency_key,
                )
            })
            .await
        }
        "models.create_customer_gateway" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "models.create_customer_gateway",
                        "/api/v1/model-connections/customer-gateway",
                )
            })
            .await
        }
        "models.delete_customer_gateway" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "models.delete_customer_gateway",
                        "/api/v1/model-connections/{connection_id}",
                )
            })
            .await
        }
        "connectors.create_credential_binding" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connectors.create_credential_binding",
                        "/api/v1/credential-bindings",
                )
            })
            .await
        }
        "connectors.revoke_credential_binding" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connectors.revoke_credential_binding",
                        "/api/v1/credential-bindings/{id}",
                )
            })
            .await
        }
        "connectors.create_custom_rest" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "connectors.create_custom_rest",
                        "/api/v1/connectors/custom-rest",
                )
            })
            .await
        }
        "audit.verify_chain" => {
            boxed_dispatch(|| {
                dispatch_client_executed("audit.verify_chain", "/api/v1/audit/compliance/verify")
            })
            .await
        }
        "audit.list_events" => {
            boxed_dispatch(|| dispatch_client_executed("audit.list_events", "/api/v1/audit")).await
        }
        "audit.export_log" => {
            boxed_dispatch(|| {
                dispatch_client_executed("audit.export_log", "/api/v1/audit/compliance/events")
            })
            .await
        }
        "audit.create_siem_destination" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "audit.create_siem_destination",
                        "/api/v1/audit/siem-destinations",
                )
            })
            .await
        }
        "audit.update_siem_destination" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "audit.update_siem_destination",
                        "/api/v1/audit/siem-destinations/{destination_id}",
                )
            })
            .await
        }
        "audit.delete_siem_destination" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "audit.delete_siem_destination",
                        "/api/v1/audit/siem-destinations/{destination_id}",
                )
            })
            .await
        }
        "schedule.list" => {
            boxed_dispatch(|| dispatch_client_executed("schedule.list", "/api/v1/cron")).await
        }
        "schedule.pause" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.pause", "/api/v1/cron/{cron_id}")
            })
            .await
        }
        "schedule.resume" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.resume", "/api/v1/cron/{cron_id}")
            })
            .await
        }
        "schedule.update_time" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.update_time", "/api/v1/cron/{cron_id}")
            })
            .await
        }
        "schedule.delete" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.delete", "/api/v1/cron/{cron_id}")
            })
            .await
        }
        "schedule.fires.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.fires.list", "/api/v1/cron/{cron_id}/fires")
            })
            .await
        }
        "schedule.run_now" => {
            boxed_dispatch(|| {
                dispatch_client_executed("schedule.run_now", "/api/v1/cron/{cron_id}/run")
            })
            .await
        }
        "automations.list_event_triggers" => {
            boxed_dispatch(|| {
                dispatch_client_executed("automations.list_event_triggers", "/api/v1/event-triggers")
            })
            .await
        }
        "automations.create_event_trigger" => {
            boxed_dispatch(|| {
                dispatch_client_executed("automations.create_event_trigger", "/api/v1/event-triggers")
            })
            .await
        }
        "automations.update_event_trigger" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "automations.update_event_trigger",
                        "/api/v1/event-triggers/{trigger_id}",
                )
            })
            .await
        }
        "automations.dry_run_event_trigger" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "automations.dry_run_event_trigger",
                        "/api/v1/event-triggers/{trigger_id}/dry-run",
                )
            })
            .await
        }
        "agents.installations.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed("agents.installations.list", "/api/v1/agents/installations")
            })
            .await
        }
        "agents.draft.save" => {
            boxed_dispatch(|| {
                dispatch_client_executed("agents.draft.save", "/api/v1/agents/{agent_id}/draft")
            })
            .await
        }
        "agents.publish" => {
            boxed_dispatch(|| {
                dispatch_client_executed("agents.publish", "/api/v1/agents/{agent_id}/publish")
            })
            .await
        }
        "agents.versions.list" => {
            boxed_dispatch(|| {
                dispatch_client_executed("agents.versions.list", "/api/v1/agents/{agent_id}/versions")
            })
            .await
        }
        "agents.preview" => {
            boxed_dispatch(|| {
                dispatch_client_executed("agents.preview", "/api/v1/chat/stream")
            })
            .await
        }
        "agents.admitted_actions.list" => {
            boxed_dispatch(|| dispatch_agents_admitted_actions_list(&state, &user, &headers)).await
        }
        "plugins.list_catalog" => {
            boxed_dispatch(|| {
                dispatch_client_executed("plugins.list_catalog", "/api/v1/plugin-catalog")
            })
            .await
        }
        "plugins.install" => {
            boxed_dispatch(|| {
                dispatch_client_executed("plugins.install", "/api/v1/plugins/{plugin_id}/install")
            })
            .await
        }
        "plugins.uninstall" => {
            boxed_dispatch(|| {
                dispatch_client_executed("plugins.uninstall", "/api/v1/plugins/{plugin_id}/uninstall")
            })
            .await
        }
        "plugins.approve_catalog" => {
            boxed_dispatch(|| {
                dispatch_client_executed(
                        "plugins.approve_catalog",
                        "/api/v1/plugin-catalog/{plugin_id}/approve",
                )
            })
            .await
        }
        "plugins.kill_digest" => {
            boxed_dispatch(|| {
                dispatch_client_executed("plugins.kill_digest", "/api/v1/plugin-kill-list")
            })
            .await
        }
        "plugins.lift_kill" => {
            boxed_dispatch(|| {
                dispatch_client_executed("plugins.lift_kill", "/api/v1/plugin-kill-list")
            })
            .await
        }
        other => (
            StatusCode::NOT_IMPLEMENTED,
            Json(error(
                "not_implemented",
                format!("no live dispatch for action '{other}'"),
            )),
        )
            .into_response(),
    }
}

/// Resolves the authenticated human's action view from a contract published by
/// its owning plane. The browser registry remains a UX helper during the
/// migration; it is not used here as an execution authority.
pub(super) async fn list_action_contracts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Response {
    list_owner_action_contracts(&state, &user).await
}

/// Resolve a caller-owned tickets.create key after an ambiguous response.
/// This read-only owner receipt lookup never routes through ticket creation.
pub(super) async fn reconcile_ticket_create(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(idempotency_key): Path<String>,
) -> Response {
    reconcile_ticket_create_dispatch(&state, &user, &idempotency_key).await
}

#[cfg(test)]
mod ingestions_dispatch_tests {
    use super::*;
    use serde_json::json;

    fn caller() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    /// Stack for one dispatch. The debug build of the ~370-arm `execute_action` match gives every
    /// arm its own slot in one poll frame, which needs more than the 2 MiB of a libtest thread
    /// (measured: 2 MiB overflows, 4 MiB passes), so the test binary aborts with
    /// STATUS_STACK_OVERFLOW. Boxing the future does not help: it is only ~20 KB, and the cost is
    /// the poll frame. 32 MiB matches the execution-core delegation tests. The image is built with
    /// `cargo build --release`, and production code is unchanged here.
    const DISPATCH_TEST_STACK_BYTES: usize = 32 * 1024 * 1024;

    /// One request through `execute_action` on a thread with [`DISPATCH_TEST_STACK_BYTES`] of
    /// stack, so the result does not depend on RUST_MIN_STACK. A dispatch panic is re-raised here.
    fn run(action_id: &str, input: Value) -> StatusCode {
        let action_id = action_id.to_owned();
        std::thread::Builder::new()
            .name("execute-action-test".to_owned())
            .stack_size(DISPATCH_TEST_STACK_BYTES)
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime")
                    .block_on(async move {
                        execute_action(
                            State(crate::tests::test_state(false)),
                            Extension(caller()),
                            HeaderMap::new(),
                            Json(ExecuteActionRequest {
                                action_id,
                                idempotency_key: String::new(),
                                input,
                            }),
                        )
                        .await
                        .status()
                    })
            })
            .expect("spawn the dispatch test thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    }

    /// The registry (WS15.L5) split `run_schedule_action` into two actions and added
    /// four run-control ones. Each new id has a live dispatch (an empty input is a
    /// 400 from its own validation, not the 501 of an unknown id); the old id has none.
    #[test]
    fn the_new_ids_dispatch_and_the_removed_id_does_not() {
        for id in [
            "ingestions.get_run_status",
            "ingestions.pause_run",
            "ingestions.resume_run",
            "ingestions.cancel_run",
            "ingestions.pause_schedule",
            "ingestions.change_schedule_state",
        ] {
            assert_eq!(run(id, json!({})), StatusCode::BAD_REQUEST, "{id}");
        }
        assert_eq!(
            run(
                REMOVED_SCHEDULE_ACTION_ID,
                json!({ "action": "delete_schedule", "scheduleId": "sch_1" })
            ),
            StatusCode::NOT_IMPLEMENTED
        );
    }

    /// The legacy toolset ids, each with the registry action it must resolve to. Test-only: the
    /// action-surface contract test does not read `#[cfg(test)]` code as dispatch surface.
    const AGENT_WEB_TOOL_TARGETS: [(&str, &str); 4] = [
        ("web.crawl_site", "knowledge.crawl_site"),
        ("web.job_status", "ingestions.get_run_status"),
        ("web.pause_job", "ingestions.pause_run"),
        ("web.cancel_job", "ingestions.cancel_run"),
    ];

    /// Ids that must stay unaliased: the toolset has no resume, map or batch verb, and a registry
    /// action is not an alias of itself.
    const UNALIASED_TEST_IDS: [&str; 4] = [
        "web.resume_job",
        "web.map_site",
        "web.batch_scrape",
        "knowledge.crawl_site",
    ];

    /// The id that WS15.L5 split away when it replaced `run_schedule_action`.
    const REMOVED_SCHEDULE_ACTION_ID: &str = "ingestions.run_schedule_action";

    /// E3.7.10: the switch is the exact word `true`; every other spelling is off.
    #[test]
    fn the_agent_web_toolset_switch_is_off_unless_exactly_true() {
        assert!(agent_web_toolset_from(Some("true")));
        for value in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some(" true")] {
            assert!(!agent_web_toolset_from(value), "{value:?}");
        }
    }

    /// Each toolset id names one existing action, and crawl_site is the crawl action
    /// itself, so the E3.7.9 page-threshold approval cannot be bypassed through it.
    #[test]
    fn each_agent_web_tool_runs_an_existing_action_and_crawl_site_keeps_its_approval_path() {
        for (id, target) in AGENT_WEB_TOOL_TARGETS {
            assert_eq!(resolve_action_id(id, true), target, "{id}");
        }
        // No resume, no map or batch, and no other prefix is aliased.
        for id in UNALIASED_TEST_IDS {
            assert_eq!(agent_web_tool_target(id), None, "{id}");
            assert_eq!(resolve_action_id(id, true), id, "{id}");
        }
    }

    /// Until the E3.7.9 approval exists, the agent's crawl runs only under an explicit page
    /// bound at or below the threshold; no bound, or a larger one, needs the approval.
    #[test]
    fn an_agent_crawl_above_the_threshold_or_without_a_bound_needs_the_approval() {
        assert!(!agent_crawl_needs_approval(&json!({ "url": "https://example.com", "max_pages": 50 })));
        assert!(!agent_crawl_needs_approval(&json!({ "url": "https://example.com", "maxPages": 1 })));
        assert!(agent_crawl_needs_approval(&json!({ "url": "https://example.com", "max_pages": 51 })));
        assert!(agent_crawl_needs_approval(&json!({ "url": "https://example.com" })));
        assert!(agent_crawl_needs_approval(&json!({ "url": "https://example.com", "max_pages": "10" })));
    }

    /// Off (the default), a toolset id is not resolved and the request gets the 501 of an
    /// unknown action, before any input is read.
    #[test]
    fn the_agent_web_tool_ids_are_unknown_actions_while_the_switch_is_off() {
        if agent_web_toolset_enabled() {
            // The surrounding environment switched it on; the off path is not observable here.
            return;
        }
        for (id, _) in AGENT_WEB_TOOL_TARGETS {
            assert_eq!(resolve_action_id(id, false), id, "{id}");
            assert_eq!(run(id, json!({})), StatusCode::NOT_IMPLEMENTED, "{id}");
        }
    }

    /// Pins the stack fix. [`DISPATCH_TEST_STACK_BYTES`] must stay at or above the 4 MiB that the
    /// debug poll frame was measured to need (2 MiB overflows), and a dispatch through the helper
    /// still answers as the dispatcher does. A smaller stack aborts the whole test binary rather
    /// than failing one test, so the compile-time check below is what names the cause.
    #[test]
    fn the_dispatch_test_stack_stays_above_the_measured_debug_need() {
        const MEASURED_DEBUG_NEED_BYTES: usize = 4 * 1024 * 1024;
        const _: () = assert!(DISPATCH_TEST_STACK_BYTES >= MEASURED_DEBUG_NEED_BYTES);
        assert_eq!(
            run("ingestions.get_run_status", json!({})),
            StatusCode::BAD_REQUEST
        );
    }
}
