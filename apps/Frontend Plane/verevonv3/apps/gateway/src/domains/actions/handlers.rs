use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{config::AppState, envelope::error, middleware::AuthenticatedUser};

use super::dispatchers::{
    dispatch_audio_dictate, dispatch_audio_transcribe, dispatch_browser_close_session,
    dispatch_browser_close_tab, dispatch_browser_create_profile, dispatch_browser_create_session,
    dispatch_browser_create_tab, dispatch_browser_delete_profile,
    dispatch_browser_probe_profile_restore, dispatch_browser_rename_profile,
    dispatch_browser_run_action, dispatch_browser_run_control, dispatch_browser_run_start,
    dispatch_browser_select_tab, dispatch_browser_set_control_mode,
    dispatch_browser_suggest_action, dispatch_chat_approve_plan, dispatch_chat_cancel_invocation,
    dispatch_chat_clear_threads, dispatch_chat_delete_thread, dispatch_chat_queue_invocation_input,
    dispatch_chat_save_thread_snapshot, dispatch_chat_submit_feedback,
    dispatch_finetune_cancel_job, dispatch_finetune_create_job, dispatch_finetune_deploy_job,
    dispatch_inbox_add_tag, dispatch_inbox_claim_draft_lease,
    dispatch_inbox_create_draft_reply_proposal, dispatch_inbox_create_incident_proposal,
    dispatch_inbox_create_internal_note_proposal, dispatch_inbox_create_problem_proposal,
    dispatch_inbox_create_ticket_update_proposal, dispatch_inbox_delete_draft,
    dispatch_inbox_release_draft_lease, dispatch_inbox_remove_tag, dispatch_inbox_review_ai_action,
    dispatch_inbox_save_draft, dispatch_inbox_send_reply, dispatch_inbox_set_assignment,
    dispatch_inbox_set_status, dispatch_inbox_submit_feedback, dispatch_inbox_workspace_set_pinned,
    dispatch_inbox_workspace_set_read, dispatch_ingestions_create_run,
    dispatch_ingestions_create_schedule, dispatch_ingestions_create_source,
    dispatch_ingestions_delete_source, dispatch_ingestions_run_schedule_action,
    dispatch_integrations_disconnect, dispatch_integrations_extend_inbox_history,
    dispatch_integrations_start_connect_session, dispatch_integrations_trigger_inbox_sync,
    dispatch_integrations_trigger_sync, dispatch_knowledge_create_document,
    dispatch_knowledge_extract_products, dispatch_knowledge_summarize_products,
    dispatch_leads_create_list, dispatch_leads_delete_list, dispatch_mcp_delete_server,
    dispatch_mcp_share_server, dispatch_membership_invite_member,
    dispatch_membership_remove_member, dispatch_membership_update_member_role,
    dispatch_memory_delete, dispatch_monitoring_check_now, dispatch_navbar_create_calendar_event,
    dispatch_navbar_create_calendar_note, dispatch_navbar_mark_notification_read,
    dispatch_navbar_save_theme, dispatch_navbar_submit_support_request,
    dispatch_notification_delete, dispatch_notification_mark_all_read,
    dispatch_notification_mark_read, dispatch_notification_update_preference,
    dispatch_orchestration_cancel_run, dispatch_orchestration_decide_approval,
    dispatch_orchestration_resume_run, dispatch_org_acknowledge_deletion,
    dispatch_org_mark_exported, dispatch_org_restore, dispatch_org_set_quota,
    dispatch_org_soft_delete, dispatch_org_update_instructions,
    dispatch_org_update_support_ai_mode, dispatch_org_update_zdr,
    dispatch_organization_switch_active, dispatch_ownership_revoke_document_share,
    dispatch_ownership_share_document, dispatch_privacy_erase_my_account,
    dispatch_router_policy_update, dispatch_settings_create_api_key,
    dispatch_settings_delete_api_key, dispatch_settings_update_me,
    dispatch_settings_update_preferences, dispatch_settings_update_setting,
    dispatch_social_create_campaign, dispatch_social_create_draft_from_inbox,
    dispatch_social_decide_approval, dispatch_space_bind_agent, dispatch_space_create_agent,
    dispatch_space_create_personal, dispatch_space_ensure_organization_room,
    dispatch_space_request_personal_deletion, dispatch_space_update_instructions,
    dispatch_studio_create_project, dispatch_studio_export_social_draft,
    dispatch_studio_save_project,
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

pub(super) async fn execute_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<ExecuteActionRequest>,
) -> Response {
    match body.action_id.as_str() {
        "knowledge.recrawl_source" => dispatch_recrawl(&state, &user, &headers, &body.input).await,
        "knowledge.scrape_url" => dispatch_scrape_url(&state, &user, &headers, &body.input).await,
        "knowledge.crawl_site" => dispatch_crawl_site(&state, &user, &headers, &body.input).await,
        "knowledge.import_source" => {
            dispatch_import_source(&state, &user, &headers, &body.input).await
        }
        "knowledge.connect_source" => {
            dispatch_connect_source(&state, &user, &headers, &body.input, &body.idempotency_key)
                .await
        }
        "knowledge.upload_files" => dispatch_upload_files().await,
        "knowledge.create_document" => {
            dispatch_knowledge_create_document(&state, &user, &headers, &body.input).await
        }
        "knowledge.extract_products" => {
            dispatch_knowledge_extract_products(&state, &user, &headers, &body.input).await
        }
        "knowledge.summarize_products" => {
            dispatch_knowledge_summarize_products(&state, &user, &headers, &body.input).await
        }
        "brreg_lookup_organization" | "brreg.lookup_organization" => {
            dispatch_brreg_lookup(&state, &user, &body.input).await
        }
        "shipping.get_quotes" => {
            dispatch_shipping_quotes(&state, &user, &headers, &body.input).await
        }
        "operating_map.generate" | "operating_map.refresh" => {
            dispatch_operating_map_generate(&state, &user, &headers, &body.input, &body.action_id)
                .await
        }
        "operating_map.review_proposal" => {
            dispatch_operating_map_review(&state, &user, &headers, &body.input).await
        }
        "operating_map.create_agent_blueprint" => {
            dispatch_operating_map_blueprint(&state, &user, &headers, &body.input).await
        }
        "workflows.toggle_policy" => dispatch_toggle_policy(&state, &user, &body.input).await,
        "inbox.follow_conversation" => {
            dispatch_conversation_follow(&state, &user, &body.input).await
        }
        "inbox.set_csat_preference" => {
            dispatch_conversation_csat_preference(&state, &user, &body.input).await
        }
        "inbox.add_tag" => dispatch_inbox_add_tag(&state, &user, &body.input).await,
        "inbox.remove_tag" => dispatch_inbox_remove_tag(&state, &user, &body.input).await,
        "inbox.claim_draft_lease" => {
            dispatch_inbox_claim_draft_lease(&state, &user, &body.input).await
        }
        "inbox.release_draft_lease" => {
            dispatch_inbox_release_draft_lease(&state, &user, &body.input).await
        }
        "inbox.save_draft" => dispatch_inbox_save_draft(&state, &user, &body.input).await,
        "inbox.delete_draft" => dispatch_inbox_delete_draft(&state, &user, &body.input).await,
        "inbox.set_status" => dispatch_inbox_set_status(&state, &user, &body.input).await,
        "inbox.set_assignment" => dispatch_inbox_set_assignment(&state, &user, &body.input).await,
        "inbox.send_reply" => dispatch_inbox_send_reply(&state, &user, &body.input).await,
        "inbox.submit_feedback" => dispatch_inbox_submit_feedback(&state, &user, &body.input).await,
        "inbox.review_ai_action" => {
            dispatch_inbox_review_ai_action(&state, &user, &body.input).await
        }
        "inbox.create_draft_reply_proposal" => {
            dispatch_inbox_create_draft_reply_proposal(&state, &user, &body.input).await
        }
        "inbox.create_internal_note_proposal" => {
            dispatch_inbox_create_internal_note_proposal(&state, &user, &body.input).await
        }
        "inbox.create_ticket_update_proposal" => {
            dispatch_inbox_create_ticket_update_proposal(&state, &user, &body.input).await
        }
        "inbox.create_incident_proposal" => {
            dispatch_inbox_create_incident_proposal(&state, &user, &body.input).await
        }
        "inbox.create_problem_proposal" => {
            dispatch_inbox_create_problem_proposal(&state, &user, &body.input).await
        }
        "inbox.set_conversation_pinned" => {
            dispatch_inbox_workspace_set_pinned(&state, &user, &body.input).await
        }
        "inbox.set_conversation_read" => {
            dispatch_inbox_workspace_set_read(&state, &user, &body.input).await
        }
        "notifications.mark_read" => {
            dispatch_notification_mark_read(&state, &user, &body.input).await
        }
        "notifications.mark_all_read" => {
            dispatch_notification_mark_all_read(&state, &user, &body.input).await
        }
        "notifications.delete" => dispatch_notification_delete(&state, &user, &body.input).await,
        "notifications.update_preference" => {
            dispatch_notification_update_preference(&state, &user, &body.input).await
        }
        "navbar.save_theme" => dispatch_navbar_save_theme(&state, &user, &body.input).await,
        "navbar.mark_notification_read" => {
            dispatch_navbar_mark_notification_read(&state, &user, &body.input).await
        }
        "navbar.create_calendar_event" => {
            dispatch_navbar_create_calendar_event(&state, &user, &body.input).await
        }
        "navbar.create_calendar_note" => {
            dispatch_navbar_create_calendar_note(&state, &user, &body.input).await
        }
        "navbar.submit_support_request" => {
            dispatch_navbar_submit_support_request(&state, &user, &body.input).await
        }
        "ownership.share_document" => {
            dispatch_ownership_share_document(&state, &user, &body.input).await
        }
        "ownership.revoke_document_share" => {
            dispatch_ownership_revoke_document_share(&state, &user, &body.input).await
        }
        "memory.delete" => dispatch_memory_delete(&state, &user, &headers, &body.input).await,
        "settings.update_me" => dispatch_settings_update_me(&state, &user, &body.input).await,
        "settings.update_preferences" => {
            dispatch_settings_update_preferences(&state, &user, &body.input).await
        }
        "settings.update_setting" => {
            dispatch_settings_update_setting(&state, &user, &body.input).await
        }
        "settings.create_api_key" => {
            dispatch_settings_create_api_key(&state, &user, &headers, &body.input).await
        }
        "settings.delete_api_key" => {
            dispatch_settings_delete_api_key(&state, &user, &headers, &body.input).await
        }
        "mcp.delete_server" => {
            dispatch_mcp_delete_server(&state, &user, &headers, &body.input).await
        }
        "mcp.share_server" => dispatch_mcp_share_server(&state, &user, &headers, &body.input).await,
        "monitoring.check_now" => {
            dispatch_monitoring_check_now(&state, &user, &headers, &body.input).await
        }
        "privacy.erase_my_account" => {
            dispatch_privacy_erase_my_account(&state, &user, &body.input).await
        }
        "leads.create_list" => dispatch_leads_create_list(&state, &user, &body.input).await,
        "leads.delete_list" => dispatch_leads_delete_list(&state, &user, &body.input).await,
        "finetune.create_job" => dispatch_finetune_create_job(&state, &user, &body.input).await,
        "finetune.cancel_job" => dispatch_finetune_cancel_job(&state, &user, &body.input).await,
        "finetune.deploy_job" => dispatch_finetune_deploy_job(&state, &user, &body.input).await,
        "studio.create_project" => dispatch_studio_create_project(&state, &user, &body.input).await,
        "studio.save_project" => dispatch_studio_save_project(&state, &user, &body.input).await,
        "studio.export_social_draft" => {
            dispatch_studio_export_social_draft(&state, &user, &body.input).await
        }
        "ingestions.create_run" => {
            dispatch_ingestions_create_run(&state, &user, &headers, &body.input).await
        }
        "ingestions.create_schedule" => {
            dispatch_ingestions_create_schedule(&state, &user, &headers, &body.input).await
        }
        "ingestions.run_schedule_action" => {
            dispatch_ingestions_run_schedule_action(&state, &user, &headers, &body.input).await
        }
        "ingestions.create_source" => {
            dispatch_ingestions_create_source(&state, &user, &headers, &body.input).await
        }
        "ingestions.delete_source" => {
            dispatch_ingestions_delete_source(&state, &user, &headers, &body.input).await
        }
        "integrations.start_connect_session" => {
            dispatch_integrations_start_connect_session(&state, &user, &headers, &body.input).await
        }
        "integrations.disconnect" => {
            dispatch_integrations_disconnect(&state, &user, &headers, &body.input).await
        }
        "integrations.trigger_sync" => {
            dispatch_integrations_trigger_sync(&state, &user, &headers, &body.input).await
        }
        "integrations.trigger_inbox_sync" => {
            dispatch_integrations_trigger_inbox_sync(&state, &user, &headers, &body.input).await
        }
        "integrations.extend_inbox_history" => {
            dispatch_integrations_extend_inbox_history(&state, &user, &headers, &body.input).await
        }
        "org.soft_delete" => dispatch_org_soft_delete(&state, &user, &body.input).await,
        "org.restore" => dispatch_org_restore(&state, &user, &body.input).await,
        "org.mark_exported" => dispatch_org_mark_exported(&state, &user, &body.input).await,
        "org.acknowledge_deletion" => {
            dispatch_org_acknowledge_deletion(&state, &user, &body.input).await
        }
        "org.set_quota" => dispatch_org_set_quota(&state, &user, &body.input).await,
        "org.update_instructions" => {
            dispatch_org_update_instructions(&state, &user, &body.input).await
        }
        "org.update_zdr" => dispatch_org_update_zdr(&state, &user, &body.input).await,
        "org.update_support_ai_mode" => {
            dispatch_org_update_support_ai_mode(&state, &user, &body.input).await
        }
        "membership.invite_member" => {
            dispatch_membership_invite_member(&state, &user, &headers, &body.input).await
        }
        "membership.remove_member" => {
            dispatch_membership_remove_member(&state, &user, &headers, &body.input).await
        }
        "membership.update_member_role" => {
            dispatch_membership_update_member_role(&state, &user, &headers, &body.input).await
        }
        "organization.switch_active" => {
            dispatch_organization_switch_active(&state, &user, &headers, &body.input).await
        }
        "chat.approve_plan" => {
            dispatch_chat_approve_plan(&state, &user, &headers, &body.input).await
        }
        "chat.cancel_invocation" => {
            dispatch_chat_cancel_invocation(&state, &user, &headers, &body.input).await
        }
        "chat.queue_invocation_input" => {
            dispatch_chat_queue_invocation_input(&state, &user, &headers, &body.input).await
        }
        "chat.clear_threads" => {
            dispatch_chat_clear_threads(&state, &user, &headers, &body.input).await
        }
        "chat.delete_thread" => {
            dispatch_chat_delete_thread(&state, &user, &headers, &body.input).await
        }
        "chat.save_thread_snapshot" => {
            dispatch_chat_save_thread_snapshot(&state, &user, &headers, &body.input).await
        }
        "chat.submit_feedback" => {
            dispatch_chat_submit_feedback(&state, &user, &headers, &body.input).await
        }
        "audio.transcribe" => dispatch_audio_transcribe(&state, &user, &headers, &body.input).await,
        "audio.dictate" => dispatch_audio_dictate(&state, &user, &headers, &body.input).await,
        "orchestration.decide_approval" => {
            dispatch_orchestration_decide_approval(&state, &user, &headers, &body.input).await
        }
        "orchestration.resume_run" => {
            dispatch_orchestration_resume_run(&state, &user, &headers, &body.input).await
        }
        "orchestration.cancel_run" => {
            dispatch_orchestration_cancel_run(&state, &user, &headers, &body.input).await
        }
        "browser_run.start" => {
            dispatch_browser_run_start(&state, &user, &headers, &body.input).await
        }
        "browser_run.control" => {
            dispatch_browser_run_control(&state, &user, &headers, &body.input).await
        }
        "router_policy.update" => dispatch_router_policy_update(&state, &user, &body.input).await,
        "browser_session.create" => {
            dispatch_browser_create_session(&state, &user, &headers, &body.input).await
        }
        "browser_session.close" => {
            dispatch_browser_close_session(&state, &user, &headers, &body.input).await
        }
        "browser_tab.create" => {
            dispatch_browser_create_tab(&state, &user, &headers, &body.input).await
        }
        "browser_tab.select" => {
            dispatch_browser_select_tab(&state, &user, &headers, &body.input).await
        }
        "browser_tab.close" => {
            dispatch_browser_close_tab(&state, &user, &headers, &body.input).await
        }
        "browser_action.run" => {
            dispatch_browser_run_action(&state, &user, &headers, &body.input).await
        }
        "browser_action.set_control_mode" => {
            dispatch_browser_set_control_mode(&state, &user, &headers, &body.input).await
        }
        "browser_action.suggest" => {
            dispatch_browser_suggest_action(&state, &user, &headers, &body.input).await
        }
        "browser_profile.create" => {
            dispatch_browser_create_profile(&state, &user, &headers, &body.input).await
        }
        "browser_profile.rename" => {
            dispatch_browser_rename_profile(&state, &user, &headers, &body.input).await
        }
        "browser_profile.delete" => {
            dispatch_browser_delete_profile(&state, &user, &headers, &body.input).await
        }
        "browser_profile.probe_restore" => {
            dispatch_browser_probe_profile_restore(&state, &user, &headers, &body.input).await
        }
        "tickets.record_csat_outcome" => {
            dispatch_ticket_record_csat_outcome(&state, &user, &body.input).await
        }
        "tickets.record_chat_handoff" => {
            dispatch_ticket_record_chat_handoff(&state, &user, &body.input).await
        }
        // Ticketing actions -> conversation-core-go (the same backend the dedicated
        // /api/v1/tickets/* routes proxy to), scoped to the caller's org.
        "tickets.create" => {
            dispatch_ticket_create(&state, &user, &body.input, &body.idempotency_key).await
        }
        "tickets.classify_conversation" => {
            dispatch_ticket_classify(&state, &user, &body.input).await
        }
        "tickets.update" => dispatch_ticket_update(&state, &user, &body.input).await,
        "tickets.assign" => dispatch_ticket_assign(&state, &user, &body.input).await,
        "tickets.link_resource" => dispatch_ticket_link_resource(&state, &user, &body.input).await,
        "tickets.resolve" => dispatch_ticket_resolve(&state, &user, &body.input).await,
        "tickets.run_macro" => dispatch_ticket_run_macro(&state, &user, &body.input).await,
        "tickets.create_macro" => dispatch_ticket_create_macro(&state, &user, &body.input).await,
        "tickets.create_checklist" => {
            dispatch_ticket_create_checklist(&state, &user, &body.input).await
        }
        "tickets.update_checklist_item" => {
            dispatch_ticket_update_checklist_item(&state, &user, &body.input).await
        }
        "tickets.create_side_conversation" => {
            dispatch_ticket_create_side_conversation(&state, &user, &body.input).await
        }
        "tickets.add_side_conversation_message" => {
            dispatch_ticket_add_side_conversation_message(&state, &user, &body.input).await
        }
        "tickets.update_side_conversation" => {
            dispatch_ticket_update_side_conversation(&state, &user, &body.input).await
        }
        "tickets.create_incident" => {
            dispatch_ticket_create_incident(&state, &user, &body.input).await
        }
        "tickets.update_incident" => {
            dispatch_ticket_update_incident(&state, &user, &body.input).await
        }
        "tickets.link_incident_ticket" => {
            dispatch_ticket_link_incident_ticket(&state, &user, &body.input).await
        }
        "tickets.create_problem" => {
            dispatch_ticket_create_problem(&state, &user, &body.input).await
        }
        "tickets.update_problem" => {
            dispatch_ticket_update_problem(&state, &user, &body.input).await
        }
        "tickets.create_sla_policy" => {
            dispatch_ticket_create_sla_policy(&state, &user, &body.input).await
        }
        "tickets.update_sla_policy" => {
            dispatch_ticket_update_sla_policy(&state, &user, &body.input).await
        }
        "tickets.create_automation_rule" => {
            dispatch_ticket_create_automation_rule(&state, &user, &body.input).await
        }
        "tickets.update_automation_rule" => {
            dispatch_ticket_update_automation_rule(&state, &user, &body.input).await
        }
        "tickets.create_team" => dispatch_ticket_create_team(&state, &user, &body.input).await,
        "tickets.update_team" => dispatch_ticket_update_team(&state, &user, &body.input).await,
        "tickets.create_view" => dispatch_ticket_create_view(&state, &user, &body.input).await,
        "tickets.update_view" => dispatch_ticket_update_view(&state, &user, &body.input).await,
        "tickets.update_macro" => dispatch_ticket_update_macro(&state, &user, &body.input).await,
        // Space actions call the same route handlers domains::spaces mounts for
        // its direct REST routes (not a parallel proxy), so a per-room grant
        // check or the create/bind-then-confirm-membership sequence can never
        // drift between the two callers.
        "spaces.create_personal_space" => {
            dispatch_space_create_personal(&state, &user, &body.input).await
        }
        "spaces.ensure_organization_room" => {
            dispatch_space_ensure_organization_room(&state, &user, &body.input).await
        }
        "spaces.update_space_instructions" => {
            dispatch_space_update_instructions(&state, &user, &body.input).await
        }
        "spaces.create_space_agent" => {
            dispatch_space_create_agent(&state, &user, &body.input).await
        }
        "spaces.bind_space_agent" => dispatch_space_bind_agent(&state, &user, &body.input).await,
        "spaces.request_personal_space_deletion" => {
            dispatch_space_request_personal_deletion(&state, &user, &body.input).await
        }
        // Social actions -> social-core via the dedicated dispatchers (real
        // ApprovalState is enforced by social-core before publish/schedule).
        "social.create_draft" => dispatch_social_create_draft(&state, &user, &body.input).await,
        "social.schedule_post" => dispatch_social_schedule_post(&state, &user, &body.input).await,
        "social.publish_post" => dispatch_social_publish_post(&state, &user, &body.input).await,
        "social.create_campaign" => {
            dispatch_social_create_campaign(&state, &user, &body.input).await
        }
        "social.create_draft_from_inbox" => {
            dispatch_social_create_draft_from_inbox(&state, &user, &body.input).await
        }
        "social.decide_approval" => {
            dispatch_social_decide_approval(&state, &user, &body.input).await
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
