import type { ActionId } from '@/shared/actions/action-registry'

/**
 * What an agent may do with a registered action (WS24-S5). The registry says what
 * an action does for people; this side map says whether an agent can reach it, and
 * when it cannot, the closed reason why. It is a typed map rather than two fields on
 * ActionDescriptor (DEC-25), so action-registry.ts stays untouched while other
 * workstreams edit it.
 *
 * The map is exhaustive by type: an id added to the registry without a row fails
 * `pnpm typecheck`. A `none` row cannot exist without a closed reason, so "not
 * offered" is never silent. `eligible` is the allowlist that feeds the catalog
 * manifest; it is set only where an owner has proven its path (model-eligibility.ts).
 * Every row is pinned by agent-action-admission.json, which the other planes vendor,
 * so changing a row is a deliberate, cross-plane change.
 */

export type ModelAccess = 'none' | 'read' | 'write_approval'

/** Why an action is closed to the agent. Closed: a new reason is a reviewed change here. */
export type ModelExemptReason =
  | 'human_identity_or_security'
  | 'approval_authority'
  | 'ui_state_only'
  | 'irreversible_destructive'
  | 'org_lifecycle_needs_control_review'
  | 'no_owner_executor'
  | 'agent_cannot_self_govern'

/** The WS05 side-effect vocabulary (mp-contracts::tool_class), as far as the registry uses it. */
export type ModelSideEffectClass = 'read' | 'context_write' | 'external_write' | 'destructive' | 'owner_effect'

/** Whose authority a call would run under: none, the person's own bearer, or a Control-signed run decision. */
export type ModelAuthority = 'none' | 'delegated_user_bearer' | 'control_run_decision'

/**
 * The Model Plane operation each surface executes the action with. An empty executor is
 * visible debt: the action is listed and has no owner path behind it.
 */
export type ModelExecutor = Readonly<{ ask?: string; do?: string; ownerEffect?: string }>

type ModelAccessBase = Readonly<{
  authority: ModelAuthority
  eligible: boolean
  executor: ModelExecutor
  hashBasis: 'catalog_entry' | 'owner_contract'
  /** Only for `owner_contract` rows: the owner's pinned schema digest, not one computed here. */
  ownerSchemaHash?: string
  sideEffectClass: ModelSideEffectClass
}>

export type NoneAccessEntry = ModelAccessBase & Readonly<{ access: 'none'; exemptReason: ModelExemptReason }>
export type ReadAccessEntry = ModelAccessBase & Readonly<{ access: 'read' }>
export type WriteApprovalAccessEntry = ModelAccessBase & Readonly<{ access: 'write_approval' }>
export type ModelAccessEntry = NoneAccessEntry | ReadAccessEntry | WriteApprovalAccessEntry

function exempt(
  exemptReason: ModelExemptReason,
  sideEffectClass: ModelSideEffectClass,
  options: { eligible?: boolean } = {},
): NoneAccessEntry {
  return {
    access: 'none',
    authority: 'control_run_decision',
    eligible: options.eligible ?? false,
    exemptReason,
    executor: {},
    hashBasis: 'catalog_entry',
    sideEffectClass,
  }
}

function readOf(executor: ModelExecutor, authority: ModelAuthority): ReadAccessEntry {
  return {
    access: 'read',
    authority,
    eligible: false,
    executor,
    hashBasis: 'catalog_entry',
    sideEffectClass: 'read',
  }
}

/**
 * One row per registered action, in registry order. The reason and class per row were
 * assigned by the rule table in the WS24-S5 build (human-only prefixes, approval
 * authorities, UI state, agent self-governance, irreversible high-risk, then "no owner
 * executor") and are reviewed row by row before any id is admitted.
 */
export const MODEL_ACCESS: Readonly<Record<ActionId, ModelAccessEntry>> = {
  'knowledge.recrawl_source': exempt('no_owner_executor', 'external_write'),
  'knowledge.scrape_url': exempt('no_owner_executor', 'external_write'),
  'knowledge.crawl_site': exempt('no_owner_executor', 'external_write'),
  'knowledge.import_source': exempt('no_owner_executor', 'external_write'),
  'knowledge.upload_files': exempt('no_owner_executor', 'external_write'),
  'knowledge.create_document': exempt('no_owner_executor', 'external_write'),
  'knowledge.extract_products': exempt('no_owner_executor', 'external_write'),
  'knowledge.summarize_products': exempt('no_owner_executor', 'external_write'),
  'knowledge.connect_source': exempt('no_owner_executor', 'external_write'),
  'knowledge.register_sharepoint_source': exempt('no_owner_executor', 'external_write'),
  'operating_map.generate': exempt('no_owner_executor', 'external_write'),
  'operating_map.refresh': exempt('no_owner_executor', 'external_write'),
  'operating_map.review_proposal': exempt('irreversible_destructive', 'destructive'),
  'operating_map.create_agent_blueprint': exempt('no_owner_executor', 'external_write'),
  'brreg.lookup_organization': readOf({ ask: 'brreg_lookup_organization', do: 'company_lookup' }, 'none'),
  'shipping.get_quotes': readOf({ ask: 'shipping_get_quotes', do: 'get_shipping_quotes' }, 'delegated_user_bearer'),
  'inbox.follow_conversation': exempt('no_owner_executor', 'external_write', { eligible: true }),
  'inbox.set_csat_preference': exempt('no_owner_executor', 'external_write', { eligible: true }),
  'inbox.add_tag': exempt('no_owner_executor', 'external_write'),
  'inbox.remove_tag': exempt('no_owner_executor', 'external_write'),
  'inbox.claim_draft_lease': exempt('no_owner_executor', 'external_write'),
  'inbox.release_draft_lease': exempt('no_owner_executor', 'external_write'),
  'inbox.save_draft': exempt('no_owner_executor', 'external_write'),
  'inbox.delete_draft': exempt('no_owner_executor', 'external_write'),
  'inbox.set_status': exempt('no_owner_executor', 'external_write'),
  'inbox.set_assignment': exempt('no_owner_executor', 'external_write'),
  'inbox.send_reply': exempt('irreversible_destructive', 'destructive'),
  'inbox.submit_feedback': exempt('no_owner_executor', 'external_write'),
  'inbox.review_ai_action': exempt('no_owner_executor', 'destructive', { eligible: true }),
  'inbox.create_draft_reply_proposal': exempt('no_owner_executor', 'external_write'),
  'inbox.create_internal_note_proposal': exempt('no_owner_executor', 'external_write'),
  'inbox.create_ticket_update_proposal': exempt('no_owner_executor', 'external_write'),
  'inbox.create_incident_proposal': exempt('no_owner_executor', 'external_write'),
  'inbox.create_problem_proposal': exempt('no_owner_executor', 'external_write'),
  'tickets.record_csat_outcome': exempt('no_owner_executor', 'external_write'),
  'tickets.create': {
    access: 'write_approval',
    authority: 'control_run_decision',
    eligible: true,
    executor: { ownerEffect: 'tickets.create' },
    hashBasis: 'owner_contract',
    ownerSchemaHash: 'sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df',
    sideEffectClass: 'owner_effect',
  },
  'tickets.classify_conversation': exempt('no_owner_executor', 'external_write'),
  'tickets.update': exempt('no_owner_executor', 'external_write'),
  'tickets.assign': exempt('no_owner_executor', 'external_write'),
  'tickets.link_resource': exempt('no_owner_executor', 'external_write'),
  'tickets.resolve': exempt('no_owner_executor', 'external_write'),
  'tickets.run_macro': exempt('no_owner_executor', 'external_write'),
  'tickets.create_macro': exempt('no_owner_executor', 'external_write'),
  'tickets.create_checklist': exempt('no_owner_executor', 'external_write'),
  'tickets.update_checklist_item': exempt('no_owner_executor', 'external_write'),
  'tickets.create_side_conversation': exempt('no_owner_executor', 'external_write'),
  'tickets.add_side_conversation_message': exempt('no_owner_executor', 'external_write'),
  'tickets.update_side_conversation': exempt('no_owner_executor', 'external_write'),
  'tickets.record_chat_handoff': exempt('no_owner_executor', 'external_write'),
  'tickets.create_incident': exempt('no_owner_executor', 'external_write'),
  'tickets.update_incident': exempt('no_owner_executor', 'external_write'),
  'tickets.link_incident_ticket': exempt('no_owner_executor', 'external_write'),
  'tickets.create_problem': exempt('no_owner_executor', 'external_write'),
  'tickets.update_problem': exempt('no_owner_executor', 'external_write'),
  'tickets.create_sla_policy': exempt('no_owner_executor', 'external_write'),
  'tickets.update_sla_policy': exempt('no_owner_executor', 'external_write'),
  'tickets.create_automation_rule': exempt('no_owner_executor', 'external_write'),
  'tickets.update_automation_rule': exempt('no_owner_executor', 'external_write'),
  'tickets.create_team': exempt('no_owner_executor', 'external_write'),
  'tickets.update_team': exempt('no_owner_executor', 'external_write'),
  'tickets.create_view': exempt('no_owner_executor', 'external_write'),
  'tickets.update_view': exempt('no_owner_executor', 'external_write'),
  'tickets.update_macro': exempt('no_owner_executor', 'external_write'),
  'spaces.create_personal_space': exempt('no_owner_executor', 'external_write'),
  'spaces.ensure_organization_room': exempt('no_owner_executor', 'external_write'),
  'spaces.update_space_instructions': exempt('no_owner_executor', 'external_write'),
  'spaces.create_space_agent': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.bind_space_agent': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.request_personal_space_deletion': exempt('irreversible_destructive', 'destructive'),
  'spaces.update_thread_presentation': exempt('no_owner_executor', 'external_write'),
  'spaces.mark_read': exempt('ui_state_only', 'external_write'),
  'spaces.record_presence': exempt('ui_state_only', 'external_write'),
  'spaces.set_agent_state': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.revoke_agent': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.set_agent_wake_policy': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.decide_approval': exempt('approval_authority', 'destructive'),
  'spaces.revoke_approval_grant': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.set_tool_policy': exempt('agent_cannot_self_govern', 'external_write'),
  'spaces.set_attention_item_state': exempt('ui_state_only', 'external_write'),
  'spaces.set_thread_follow': exempt('ui_state_only', 'external_write'),
  'spaces.mark_all_read': exempt('ui_state_only', 'external_write'),
  'spaces.mark_unread': exempt('ui_state_only', 'external_write'),
  'spaces.set_notify_level': exempt('ui_state_only', 'external_write'),
  'spaces.set_status': exempt('ui_state_only', 'external_write'),
  'spaces.create_reminder': exempt('ui_state_only', 'external_write'),
  'spaces.cancel_reminder': exempt('ui_state_only', 'external_write'),
  'spaces.stop_run': exempt('approval_authority', 'destructive'),
  'spaces.create_room': exempt('no_owner_executor', 'external_write'),
  'spaces.add_member': exempt('no_owner_executor', 'external_write'),
  'spaces.remove_member': exempt('no_owner_executor', 'external_write'),
  'spaces.set_effect_entitlements': exempt('human_identity_or_security', 'external_write'),
  'spaces.apply_effect_preset': exempt('human_identity_or_security', 'external_write'),
  'spaces.create_post': exempt('human_identity_or_security', 'external_write'),
  'spaces.reply_in_thread': exempt('human_identity_or_security', 'external_write'),
  'spaces.change_member_role': exempt('human_identity_or_security', 'external_write'),
  'spaces.leave_room': exempt('human_identity_or_security', 'external_write'),
  'spaces.update_sidebar': exempt('ui_state_only', 'external_write'),
  'spaces.open_dm': exempt('human_identity_or_security', 'external_write'),
  'spaces.hide_space': exempt('ui_state_only', 'external_write'),
  'spaces.edit_message': exempt('human_identity_or_security', 'external_write'),
  'spaces.delete_message': exempt('irreversible_destructive', 'destructive'),
  'spaces.toggle_reaction': exempt('human_identity_or_security', 'external_write'),
  'space.files.attach': exempt('human_identity_or_security', 'external_write'),
  'space.files.list': exempt('no_owner_executor', 'read'),
  'space.files.read_text': exempt('no_owner_executor', 'read'),
  'space.files.delete': exempt('irreversible_destructive', 'destructive'),
  'spaces.update_room_details': exempt('human_identity_or_security', 'external_write'),
  'spaces.join_room': exempt('human_identity_or_security', 'external_write'),
  'spaces.update_room_settings': exempt('human_identity_or_security', 'external_write'),
  'social.create_draft': exempt('no_owner_executor', 'external_write'),
  'social.schedule_post': exempt('no_owner_executor', 'external_write'),
  'social.publish_post': exempt('irreversible_destructive', 'destructive'),
  'social.create_campaign': exempt('no_owner_executor', 'external_write'),
  'social.create_draft_from_inbox': exempt('no_owner_executor', 'external_write'),
  'social.decide_approval': exempt('approval_authority', 'destructive'),
  'workflows.toggle_policy': exempt('no_owner_executor', 'external_write'),
  'notifications.mark_read': exempt('ui_state_only', 'external_write'),
  'notifications.mark_all_read': exempt('ui_state_only', 'external_write'),
  'notifications.delete': exempt('ui_state_only', 'external_write'),
  'notifications.update_preference': exempt('ui_state_only', 'external_write'),
  'navbar.save_theme': exempt('ui_state_only', 'external_write'),
  'navbar.mark_notification_read': exempt('ui_state_only', 'external_write'),
  'navbar.create_calendar_event': exempt('ui_state_only', 'external_write'),
  'navbar.create_calendar_note': exempt('ui_state_only', 'external_write'),
  'navbar.submit_support_request': exempt('ui_state_only', 'external_write'),
  'inbox.set_conversation_pinned': exempt('ui_state_only', 'external_write'),
  'inbox.set_conversation_read': exempt('ui_state_only', 'external_write'),
  'ownership.share_document': exempt('no_owner_executor', 'external_write'),
  'ownership.revoke_document_share': exempt('no_owner_executor', 'external_write'),
  'memory.delete': exempt('irreversible_destructive', 'destructive'),
  'memory.set_state': exempt('human_identity_or_security', 'external_write'),
  'memory.reset': exempt('irreversible_destructive', 'destructive'),
  'memory.remember': exempt('human_identity_or_security', 'external_write'),
  'memory.profile.save': exempt('human_identity_or_security', 'external_write'),
  'memory.profile.rebuild': exempt('human_identity_or_security', 'external_write'),
  'learning.accept_skill': exempt('agent_cannot_self_govern', 'context_write'),
  'learning.reject_skill': exempt('agent_cannot_self_govern', 'context_write'),
  'learning.undo': exempt('agent_cannot_self_govern', 'destructive'),
  'settings.update_me': exempt('human_identity_or_security', 'external_write'),
  'settings.update_preferences': exempt('human_identity_or_security', 'external_write'),
  'settings.update_setting': exempt('human_identity_or_security', 'external_write'),
  'settings.set_tone_preset': exempt('human_identity_or_security', 'external_write'),
  'settings.complete_personal_setup': exempt('human_identity_or_security', 'external_write'),
  'settings.skip_personal_setup': exempt('human_identity_or_security', 'external_write'),
  'settings.create_api_key': exempt('human_identity_or_security', 'external_write'),
  'settings.delete_api_key': exempt('human_identity_or_security', 'external_write'),
  'mcp.delete_server': exempt('no_owner_executor', 'external_write'),
  'mcp.share_server': exempt('no_owner_executor', 'external_write'),
  'mcp.approve_tool': exempt('irreversible_destructive', 'destructive'),
  'tools.set_policy': exempt('no_owner_executor', 'external_write'),
  'org.set_agent_policy': exempt('human_identity_or_security', 'external_write'),
  'org.preview_effective_policy': exempt('ui_state_only', 'read'),
  'org.create_hook': exempt('human_identity_or_security', 'external_write'),
  'org.update_hook': exempt('human_identity_or_security', 'external_write'),
  'org.delete_hook': exempt('human_identity_or_security', 'external_write'),
  'monitoring.check_now': exempt('no_owner_executor', 'external_write'),
  'monitoring.create_monitor': exempt('no_owner_executor', 'external_write'),
  'monitoring.delete_monitor': exempt('no_owner_executor', 'external_write'),
  'privacy.erase_my_account': exempt('human_identity_or_security', 'destructive'),
  'leads.create_list': exempt('no_owner_executor', 'external_write'),
  'leads.delete_list': exempt('no_owner_executor', 'external_write'),
  'finetune.create_job': exempt('no_owner_executor', 'external_write'),
  'finetune.cancel_job': exempt('no_owner_executor', 'external_write'),
  'finetune.deploy_job': exempt('no_owner_executor', 'external_write'),
  'studio.create_project': exempt('no_owner_executor', 'external_write'),
  'studio.save_project': exempt('no_owner_executor', 'external_write'),
  'studio.export_social_draft': exempt('no_owner_executor', 'external_write'),
  'ingestions.create_run': exempt('no_owner_executor', 'external_write'),
  'ingestions.create_schedule': exempt('no_owner_executor', 'external_write'),
  'ingestions.get_run_status': exempt('no_owner_executor', 'read'),
  'ingestions.pause_run': exempt('no_owner_executor', 'external_write'),
  'ingestions.resume_run': exempt('no_owner_executor', 'external_write'),
  'ingestions.cancel_run': exempt('no_owner_executor', 'external_write'),
  'ingestions.pause_schedule': exempt('no_owner_executor', 'external_write'),
  'ingestions.change_schedule_state': exempt('no_owner_executor', 'external_write'),
  'ingestions.create_source': exempt('no_owner_executor', 'external_write'),
  'ingestions.delete_source': exempt('no_owner_executor', 'external_write'),
  'ingestions.job_control': exempt('no_owner_executor', 'external_write'),
  'ingestions.start_agent_run': exempt('no_owner_executor', 'external_write'),
  'integrations.start_connect_session': exempt('no_owner_executor', 'external_write'),
  'integrations.disconnect': exempt('no_owner_executor', 'external_write'),
  'integrations.start_chatgpt_subscription': exempt('no_owner_executor', 'external_write'),
  'integrations.disconnect_chatgpt_subscription': exempt('no_owner_executor', 'external_write'),
  'integrations.trigger_sync': exempt('no_owner_executor', 'external_write'),
  'integrations.trigger_inbox_sync': exempt('no_owner_executor', 'external_write'),
  'integrations.extend_inbox_history': exempt('no_owner_executor', 'external_write'),
  'integrations.browse_connection': exempt('no_owner_executor', 'read'),
  'integrations.fetch_connection_content': exempt('no_owner_executor', 'read'),
  'integrations.list_connector_policies': exempt('agent_cannot_self_govern', 'read'),
  'integrations.set_connector_policy': exempt('agent_cannot_self_govern', 'external_write'),
  'org.soft_delete': exempt('human_identity_or_security', 'external_write'),
  'org.restore': exempt('human_identity_or_security', 'external_write'),
  'org.mark_exported': exempt('org_lifecycle_needs_control_review', 'external_write', { eligible: true }),
  'org.acknowledge_deletion': exempt('org_lifecycle_needs_control_review', 'external_write', { eligible: true }),
  'org.set_quota': exempt('human_identity_or_security', 'external_write'),
  'org.set_model_entitlement': exempt('human_identity_or_security', 'external_write'),
  'org.add_domain': exempt('human_identity_or_security', 'external_write'),
  'org.verify_domain': exempt('human_identity_or_security', 'external_write'),
  'org.revoke_domain': exempt('human_identity_or_security', 'external_write'),
  'org.update_instructions': exempt('human_identity_or_security', 'external_write'),
  'org.update_zdr': exempt('human_identity_or_security', 'external_write'),
  'org.update_sso_policy': exempt('human_identity_or_security', 'external_write'),
  'org.add_sso_provider': exempt('human_identity_or_security', 'external_write'),
  'org.remove_sso_provider': exempt('human_identity_or_security', 'external_write'),
  'org.create_scim_token': exempt('human_identity_or_security', 'external_write'),
  'org.revoke_scim_token': exempt('human_identity_or_security', 'external_write'),
  'org.update_support_ai_mode': exempt('human_identity_or_security', 'external_write'),
  'org.update_agent_learning_policy': exempt('human_identity_or_security', 'external_write'),
  'membership.invite_member': exempt('human_identity_or_security', 'external_write'),
  'membership.remove_member': exempt('human_identity_or_security', 'external_write'),
  'membership.update_member_role': exempt('human_identity_or_security', 'external_write'),
  'organization.switch_active': exempt('human_identity_or_security', 'external_write'),
  'chat.approve_plan': exempt('approval_authority', 'destructive'),
  'chat.execute_approved_plan': exempt('irreversible_destructive', 'destructive'),
  'chat.cancel_invocation': exempt('no_owner_executor', 'context_write'),
  'chat.queue_invocation_input': exempt('no_owner_executor', 'context_write'),
  'chat.run.continue': exempt('irreversible_destructive', 'destructive'),
  'chat.answer_question': exempt('no_owner_executor', 'context_write'),
  'approvals.decide': exempt('irreversible_destructive', 'destructive'),
  'approvals.preview': exempt('approval_authority', 'read'),
  'chat.clear_threads': exempt('irreversible_destructive', 'destructive'),
  'chat.delete_thread': exempt('no_owner_executor', 'context_write'),
  'chat.mark_thread_seen': exempt('no_owner_executor', 'context_write'),
  'chat.save_thread_snapshot': exempt('no_owner_executor', 'context_write', { eligible: true }),
  'chat.submit_feedback': exempt('no_owner_executor', 'context_write', { eligible: true }),
  'chat.upload_document': exempt('no_owner_executor', 'external_write'),
  'chat.extract_document': exempt('no_owner_executor', 'external_write'),
  'chat.set_connector_scope': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.set_agent_mode': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.set_transcript_density': exempt('ui_state_only', 'external_write'),
  'chat.inspect_run_request': exempt('no_owner_executor', 'read'),
  'chat.open_cited_passage': exempt('no_owner_executor', 'read'),
  'chat.list_artifact_revisions': exempt('no_owner_executor', 'read'),
  'chat.save_artifact_revision': exempt('no_owner_executor', 'context_write'),
  'chat.restore_artifact_revision': exempt('no_owner_executor', 'context_write'),
  'chat.edit_artifact_selection': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.save_artifact_to_knowledge': exempt('no_owner_executor', 'external_write'),
  'chat.wide_research_start': exempt('no_owner_executor', 'external_write'),
  'chat.wide_research_retry_item': exempt('no_owner_executor', 'external_write'),
  'chat.research_plan_start': exempt('no_owner_executor', 'external_write'),
  'chat.research_plan_cancel': exempt('ui_state_only', 'context_write'),
  'chat.set_context_refs': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.pick_context_ref': exempt('no_owner_executor', 'read'),
  'chat.list_deck_templates': exempt('no_owner_executor', 'read'),
  'chat.apply_deck_template': exempt('no_owner_executor', 'context_write'),
  'chat.open_deck_slide': exempt('ui_state_only', 'read'),
  'chat.set_goal': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.update_goal': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.clear_goal': exempt('agent_cannot_self_govern', 'context_write'),
  'chat.continue_goal': exempt('no_owner_executor', 'external_write'),
  'chat.goal_auto_toggle': exempt('agent_cannot_self_govern', 'context_write'),
  'audio.transcribe': exempt('no_owner_executor', 'external_write'),
  'audio.dictate': exempt('no_owner_executor', 'external_write'),
  'orchestration.decide_approval': exempt('approval_authority', 'destructive'),
  'orchestration.resume_run': exempt('no_owner_executor', 'external_write'),
  'orchestration.cancel_run': exempt('no_owner_executor', 'external_write'),
  'browser_run.start': exempt('irreversible_destructive', 'destructive'),
  'browser_run.control': exempt('no_owner_executor', 'external_write'),
  'browser_run.steer': exempt('no_owner_executor', 'context_write'),
  'browser_session.create': exempt('no_owner_executor', 'external_write'),
  'browser_session.close': exempt('no_owner_executor', 'external_write'),
  'browser_tab.create': exempt('no_owner_executor', 'external_write'),
  'browser_tab.select': exempt('no_owner_executor', 'external_write'),
  'browser_tab.close': exempt('no_owner_executor', 'external_write'),
  'browser_action.run': exempt('irreversible_destructive', 'destructive'),
  'browser_action.set_control_mode': exempt('agent_cannot_self_govern', 'external_write'),
  'browser_action.suggest': exempt('no_owner_executor', 'external_write'),
  'browser_profile.create': exempt('no_owner_executor', 'external_write'),
  'browser_profile.rename': exempt('no_owner_executor', 'external_write'),
  'browser_profile.delete': exempt('no_owner_executor', 'external_write'),
  'browser_profile.probe_restore': exempt('no_owner_executor', 'external_write'),
  'browser_omnibox.search': exempt('no_owner_executor', 'external_write'),
  'browser_omnibox.suggest': exempt('no_owner_executor', 'external_write'),
  'browser_evidence.compile_procedure': exempt('no_owner_executor', 'external_write'),
  'browser_session.keepalive': exempt('no_owner_executor', 'external_write'),
  'browser.blocked.try_other_source': exempt('no_owner_executor', 'external_write'),
  'browser_download.send_to_knowledge': exempt('no_owner_executor', 'external_write'),
  'router_policy.update': exempt('no_owner_executor', 'external_write'),
  'audit.verify_chain': exempt('no_owner_executor', 'external_write'),
  'audit.list_events': exempt('no_owner_executor', 'read'),
  'audit.export_log': exempt('no_owner_executor', 'external_write'),
  'audit.create_siem_destination': exempt('no_owner_executor', 'external_write'),
  'audit.update_siem_destination': exempt('no_owner_executor', 'external_write'),
  'audit.delete_siem_destination': exempt('no_owner_executor', 'external_write'),
  'schedule.list': exempt('no_owner_executor', 'read'),
  'schedule.pause': exempt('agent_cannot_self_govern', 'external_write'),
  'schedule.resume': exempt('agent_cannot_self_govern', 'external_write'),
  'schedule.update_time': exempt('agent_cannot_self_govern', 'external_write'),
  'schedule.delete': exempt('irreversible_destructive', 'destructive'),
  'schedule.fires.list': exempt('no_owner_executor', 'read'),
  'schedule.run_now': exempt('agent_cannot_self_govern', 'external_write'),
  'agents.installations.list': exempt('no_owner_executor', 'read'),
  'agents.draft.save': exempt('agent_cannot_self_govern', 'external_write'),
  'agents.publish': exempt('agent_cannot_self_govern', 'external_write'),
  'agents.versions.list': exempt('no_owner_executor', 'read'),
}

/**
 * Reads the Ask loop answers that have no registry row (gateway-only Verevon reads). They
 * are contract rows with `registry: false`; the registry never lists them.
 */
export type ModelReadSurfaceEntry = Readonly<{
  access: 'read'
  authority: 'delegated_user_bearer'
  executor: Readonly<{ ask: string; do?: string }>
  sideEffectClass: 'read'
}>

function readSurfaceOf(ask: string, doTool?: string): ModelReadSurfaceEntry {
  const executor = doTool === undefined ? { ask } : { ask, do: doTool }
  return { access: 'read', authority: 'delegated_user_bearer', executor, sideEffectClass: 'read' }
}

export const MODEL_READ_SURFACE: Readonly<Record<string, ModelReadSurfaceEntry>> = {
  'insights.overview': readSurfaceOf('insights_overview'),
  'inbox.search': readSurfaceOf('inbox_search'),
  'inbox.get_conversation': readSurfaceOf('inbox_get_conversation'),
  'social.list_accounts': readSurfaceOf('social_list_accounts'),
  'social.list_posts': readSurfaceOf('social_list_posts'),
  'social.list_campaigns': readSurfaceOf('social_list_campaigns'),
  'knowledge.list_documents': readSurfaceOf('knowledge_list_documents', 'knowledge_list_documents'),
  'knowledge.graph_search': readSurfaceOf('knowledge_graph_search'),
  'knowledge.wiki_search': readSurfaceOf('knowledge_wiki_search'),
  'knowledge.contradictions': readSurfaceOf('knowledge_contradictions'),
}
