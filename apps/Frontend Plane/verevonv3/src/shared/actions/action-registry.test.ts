import { describe, expect, it } from 'vitest'
import {
  actionRegistry,
  getActionDescriptor,
  integrationsBrowseConnectionInput,
  integrationsFetchConnectionContentInput,
} from '@/shared/actions/action-registry'
import { chatProjectSchema } from '@/shared/actions/chat-project'
import { MODEL_ACCESS } from '@/shared/actions/model-access'
import { isModelExecutableAction } from '@/shared/actions/model-eligibility'
import { buildModelContextPack } from '@/shared/context-packs/context-pack'

describe('action registry', () => {
  it('gives every registered action an AI-visible contract', () => {
    expect(actionRegistry.length).toBeGreaterThan(0)

    for (const action of actionRegistry) {
      expect(action.id).toContain('.')
      expect(action.inputSchema).toBeDefined()
      expect(action.outputSchema).toBeDefined()
      expect(typeof action.requiresApproval).toBe('boolean')
    }
  })

  it('reports view and redaction policy in compact model context packs', () => {
    const pack = buildModelContextPack({
      route: '/knowledge',
      visibleItems: [{ type: 'source', id: 'src_website', label: 'Website crawl', status: 'stale' }],
    })

    expect(pack.currentView).toBe('knowledge')
    // Model eligibility is earned per action (see model-eligibility.ts): each
    // of these is exposed because its owner issues a durable, queryable
    // record of the action, collapses duplicate submissions, and denies a
    // forged actor on both sides. Asserted as an exact set, so a further
    // action cannot be admitted without this test being revisited.
    expect(pack.availableActions).toEqual([
      'inbox.follow_conversation',
      'inbox.set_csat_preference',
      'inbox.review_ai_action',
      'tickets.create',
      'org.mark_exported',
      'org.acknowledge_deletion',
      'chat.save_thread_snapshot',
      'chat.submit_feedback',
    ])
    // Kept explicit so loosening the allowlist cannot silently expose either a
    // side-effecting crawl or an irreversible, approval-gated publish.
    expect(pack.availableActions).not.toContain('knowledge.recrawl_source')
    expect(pack.availableActions).not.toContain('social.publish_post')
    expect(pack.redactionPolicy).toBe('ids-and-summaries-only')
  })

  it('does not expose actions that have no reachable backend', () => {
    // security.check_url_reputation / security.investigate_url described Web Risk
    // and urlscan.io connectors that do not exist (absent from the frozen
    // actions-surface contract, integration-corev2, and the gateway dispatcher),
    // and agents.deploy_channel targeted the Channel Plane, which is docs-only
    // today. An action contract without a backend is a misleading surface for
    // humans and agents alike, so these ids must stay unregistered.
    const ids = actionRegistry.map((action) => action.id)
    expect(ids).not.toContain('security.check_url_reputation')
    expect(ids).not.toContain('security.investigate_url')
    expect(ids).not.toContain('agents.deploy_channel')
  })

  it('keeps social publishing approval-gated', () => {
    const draft = getActionDescriptor('social.create_draft')
    const publish = getActionDescriptor('social.publish_post')

    expect(draft?.requiresApproval).toBe(false)
    expect(publish?.requiresApproval).toBe(true)
    expect(publish?.reversible).toBe(false)

    expect(publish?.inputSchema.safeParse({
      postId: 'social_post_1',
      platforms: ['linkedin', 'x'],
      approvalId: 'approval_1',
    }).success).toBe(true)
  })

  it('registers the run continuation as a model-plane action the model cannot execute', () => {
    const continueRun = getActionDescriptor('chat.run.continue')

    expect(continueRun?.ownerPlane).toBe('model')
    expect(continueRun?.risk).toBe('high')
    expect(continueRun?.requiresApproval).toBe(false)
    expect(isModelExecutableAction('chat.run.continue')).toBe(false)

    // A blank identifier is refused at the contract, before any upstream call.
    expect(continueRun?.inputSchema.safeParse({
      runId: 'run_1',
      approvalId: '',
      claimToken: 'claim_1',
    }).success).toBe(false)
    expect(continueRun?.inputSchema.safeParse({
      runId: 'run_1',
      approvalId: 'appr_1',
      claimToken: 'claim_1',
    }).success).toBe(true)
  })

  it('pins the continuation input keys and says in its description what a refusal or a spent frame means', () => {
    const continueRun = getActionDescriptor('chat.run.continue')

    // The wire carries only the three identifiers; nothing else widens a continuation.
    expect(Object.keys((continueRun?.inputSchema as unknown as { shape: Record<string, unknown> }).shape).sort())
      .toEqual(['approvalId', 'claimToken', 'runId'])
    expect(continueRun?.risk).toBe('high')
    expect(continueRun?.requiresApproval).toBe(false)
    expect(continueRun?.reversible).toBe(false)
    expect(isModelExecutableAction('chat.run.continue')).toBe(false)

    const description = continueRun?.description ?? ''
    // Single use, and where the closed refusals live.
    expect(description).toContain('single use')
    expect(description).toContain('chat-run-continue-contract.ts')
    // A spent frame is final: follow the run, never retry with a new token.
    expect(description).toContain('approval_spent true')
    expect(description).toContain('never retry with a new token')
    // A refusal frame without the flag leaves the approval state unknown.
    expect(description).toContain('without approval_spent')
    // No instruction a client cannot follow: no read the SPA has shows whether an
    // approval was spent, so the description never says to re-read one. It says what
    // a client can do: do not assume either way, follow the run by its id, and a
    // retry with the same token answers 409 if the click was spent.
    expect(description).not.toMatch(/re-?read\s+the\s+approval/i)
    expect(description).toContain('do not assume either way')
    expect(description).toContain('follow the run by its id with the existing run read')
    expect(description).toContain('A retry with the same token is answered either with the continuation or with a refusal')
    expect(description).toContain('only 409 continuation_already_consumed means the click was spent')
    expect(description).toContain('not_resumable, approval_not_found and continue_disabled leave the spend unknown')
    expect(description).toContain('the run read is the arbiter')
    expect(description).not.toContain('answers 409 if the click was spent')
    expect(description).toContain('every consumed approval is refused with 409 continuation_already_consumed')
    // Following a run is not a promise that it is live.
    expect(description).toContain('does not promise that the run is live')
    expect(description).toContain('about 15 minutes from the consume')
    // A failed request that is not a listed refusal says nothing about the spend.
    expect(description).toContain('upstream_unavailable')
    // The thread's approved plan and its grant are not consumed.
    expect(description).toContain('approved plan')
    expect(description).toContain('not consumed')
    // The original sentence is kept.
    expect(description).toContain('the approved call runs once under the authority the pause recorded')
  })

  it('lets a parked run be continued with no approval: approvalId is optional, never blank, and the description says what the park form is', () => {
    const continueRun = getActionDescriptor('chat.run.continue')
    const schema = continueRun?.inputSchema

    // The park form: a run id and the claim token, no approval.
    expect(schema?.safeParse({ runId: 'run_1', claimToken: 'claim_1' }).success).toBe(true)
    // The approval form is unchanged.
    expect(schema?.safeParse({ runId: 'run_1', approvalId: 'appr_1', claimToken: 'claim_1' }).success).toBe(true)
    // An approval id that is present is never blank: the wire's empty approval id is what the
    // client writes for the park form, never something a caller supplies.
    expect(schema?.safeParse({ runId: 'run_1', approvalId: '', claimToken: 'claim_1' }).success).toBe(false)
    expect(schema?.safeParse({ runId: 'run_1', approvalId: undefined, claimToken: 'claim_1' }).success).toBe(true)
    // The run and the claim token are still required, and never blank.
    expect(schema?.safeParse({ runId: 'run_1' }).success).toBe(false)
    expect(schema?.safeParse({ claimToken: 'claim_1' }).success).toBe(false)
    expect(schema?.safeParse({ runId: '', claimToken: 'claim_1' }).success).toBe(false)
    expect(schema?.safeParse({ runId: 'run_1', claimToken: '' }).success).toBe(false)
    expect(schema?.safeParse({ runId: 'run_1', approvalId: 'appr_1' }).success).toBe(false)

    // The wire still names only the three identifiers, and the action keeps its standing.
    expect(Object.keys((schema as unknown as { shape: Record<string, unknown> }).shape).sort())
      .toEqual(['approvalId', 'claimToken', 'runId'])
    expect(continueRun?.risk).toBe('high')
    expect(continueRun?.requiresApproval).toBe(false)
    expect(isModelExecutableAction('chat.run.continue')).toBe(false)

    const description = continueRun?.description ?? ''
    expect(description).toContain('park form')
    expect(description).toContain('omit approvalId')
    expect(description).toContain('send only runId and claimToken')
    expect(description).toContain('nothing is spent')
    // A park failure frame is marked, and never says an approval was spent.
    expect(description).toContain('park_continue true never says approval_spent')
    for (const code of ['park_not_restorable', 'park_route_unavailable', 'park_not_due']) {
      expect(description).toContain(code)
    }
    // The client follows the run; the run read says whether it stays parked.
    expect(description).toContain('the client follows the run by its id with the existing run read')
    expect(description).toContain('not_resumable and park_not_restorable say it does not')
    expect(description).toContain('a bare agent_dispatch_unavailable on a park stream leave that unknown')
    expect(description).not.toMatch(/re-?read\s+the\s+approval/i)
    expect(description).not.toMatch(/reread/i)
  })

  it('registers chat.execute_approved_plan as a high-risk model-plane action the model cannot execute', () => {
    const execute = getActionDescriptor('chat.execute_approved_plan')

    expect(execute?.ownerPlane).toBe('model')
    expect(execute?.risk).toBe('high')
    // The approval click and the server-held grant are the human decision.
    expect(execute?.requiresApproval).toBe(false)
    expect(execute?.reversible).toBe(false)
    expect(isModelExecutableAction('chat.execute_approved_plan')).toBe(false)
    // Approving a plan is not model-executable either: a model never grants itself authority.
    expect(isModelExecutableAction('chat.approve_plan')).toBe(false)

    expect(execute?.inputSchema.safeParse({ threadId: 'thread_1' }).success).toBe(true)
    expect(execute?.inputSchema.safeParse({ threadId: 'thread_1', planId: 'plan_1' }).success).toBe(true)
    // The rung lives on the server; a client-supplied rung is refused at the contract.
    expect(execute?.inputSchema.safeParse({
      threadId: 'thread_1',
      grantedRung: 'danger_full_access',
    }).success).toBe(false)
    expect(execute?.inputSchema.safeParse({ threadId: '' }).success).toBe(false)
    expect(execute?.inputSchema.safeParse({ threadId: 'thread_1', planId: '' }).success).toBe(false)

    const description = execute?.description ?? ''
    expect(description).toContain('NEW')
    expect(description).toContain('rung the server holds')
    expect(description).toContain('never model-executable')
  })

  it('lets chat.approve_plan name the structured plan and carry the person\'s step edits', () => {
    const approve = getActionDescriptor('chat.approve_plan')
    const base = { runId: 'run_1', grantedRung: 'workspace_write', justification: 'Looks right to me' }

    // The legacy rung-only input still parses.
    expect(approve?.inputSchema.safeParse(base).success).toBe(true)
    expect(approve?.inputSchema.safeParse({
      ...base,
      planId: 'plan_1',
      editedSteps: [
        { stepId: 's1', title: 'Rename the file', removed: false },
        { title: 'A step the person added', operation: 'write file', removed: false },
        { stepId: 's3', title: 'Drop me', removed: true },
      ],
    }).success).toBe(true)
    expect(approve?.inputSchema.safeParse({ ...base, planId: '' }).success).toBe(false)
    expect(approve?.inputSchema.safeParse({
      ...base,
      editedSteps: [{ stepId: 's1', removed: true }],
    }).success).toBe(false)
  })

  it('registers the question answer as a low-risk, owner-only action that never grants authority', () => {
    const answer = getActionDescriptor('chat.answer_question')

    expect(answer?.ownerPlane).toBe('model')
    expect(answer?.risk).toBe('low')
    expect(answer?.requiresApproval).toBe(false)
    expect(isModelExecutableAction('chat.answer_question')).toBe(false)

    const description = answer?.description ?? ''
    expect(description).toContain('not an approval')
    expect(description).toContain('never an authority grant')
    expect(description).toContain('question_already_answered')
    expect(description).toContain('question_expired')
    expect(description).toContain('Never model-executable')
  })

  it('takes an answer for the question, or a skip, and nothing a client does not own', () => {
    const answer = getActionDescriptor('chat.answer_question')
    const one = { itemId: 'scope', optionIds: ['main'] }

    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', answers: [one] }).success).toBe(true)
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', answers: [{ itemId: 'notes', text: 'n/a' }] }).success).toBe(true)
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', skip: true }).success).toBe(true)

    // Neither nothing, nor a skip that also answers, nor an answer that skips nothing.
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1' }).success).toBe(false)
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', skip: true, answers: [one] }).success).toBe(false)
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', skip: false, answers: [] }).success).toBe(false)
    expect(answer?.inputSchema.safeParse({ questionId: '', skip: true }).success).toBe(false)

    // The org, the owner and the carrying message are the server's, never a client input.
    for (const forged of [{ orgId: 'org_x' }, { userId: 'user_x' }, { answeredMessageId: 'msg_x' }]) {
      expect(answer?.inputSchema.safeParse({ questionId: 'q_1', skip: true, ...forged }).success).toBe(false)
    }
    expect(answer?.inputSchema.safeParse({
      questionId: 'q_1',
      answers: [{ itemId: 'scope', optionIds: ['main'], userId: 'user_x' }],
    }).success).toBe(false)
  })

  it('holds the answer to the shared question bounds', () => {
    const answer = getActionDescriptor('chat.answer_question')
    const item = (n: number) => ({ itemId: `i${n}`, optionIds: ['a'] })

    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', answers: [1, 2, 3, 4].map(item) }).success).toBe(true)
    expect(answer?.inputSchema.safeParse({ questionId: 'q_1', answers: [1, 2, 3, 4, 5].map(item) }).success).toBe(false)
    expect(answer?.inputSchema.safeParse({
      questionId: 'q_1',
      answers: [{ itemId: 'i', optionIds: ['a', 'b', 'c', 'd', 'e', 'f', 'g'] }],
    }).success).toBe(false)
    expect(answer?.inputSchema.safeParse({
      questionId: 'q_1',
      answers: [{ itemId: 'notes', text: 'x'.repeat(2000) }],
    }).success).toBe(true)
    expect(answer?.inputSchema.safeParse({
      questionId: 'q_1',
      answers: [{ itemId: 'notes', text: 'x'.repeat(2001) }],
    }).success).toBe(false)
  })

  it('registers the approval decision as a high-risk, owner-only action the model cannot execute', () => {
    const decide = getActionDescriptor('approvals.decide')

    expect(decide?.ownerPlane).toBe('model')
    expect(decide?.risk).toBe('high')
    expect(decide?.reversible).toBe(false)
    expect(isModelExecutableAction('approvals.decide')).toBe(false)
    expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain(
      'approvals.decide',
    )

    const description = decide?.description ?? ''
    expect(description).toContain('scope_not_enabled')
    expect(description).toContain('Never model-executable')
  })

  it('takes a decision for one call and nothing a client does not own', () => {
    const decide = getActionDescriptor('approvals.decide')
    const base = { approvalId: 'appr_1', decision: 'approve' as const }

    expect(decide?.inputSchema.safeParse(base).success).toBe(true)
    expect(decide?.inputSchema.safeParse({ ...base, decision: 'reject', reason: 'not this one' }).success).toBe(true)
    expect(decide?.inputSchema.safeParse({ ...base, scope: 'once' }).success).toBe(true)

    // A standing scope does not exist yet, and a decision is one of two words.
    for (const scope of ['thread', 'run', 'forever']) {
      expect(decide?.inputSchema.safeParse({ ...base, scope }).success).toBe(false)
    }
    expect(decide?.inputSchema.safeParse({ ...base, decision: 'maybe' }).success).toBe(false)
    expect(decide?.inputSchema.safeParse({ ...base, approvalId: '' }).success).toBe(false)
    expect(decide?.inputSchema.safeParse({ ...base, reason: 'x'.repeat(501) }).success).toBe(false)

    // The decider, the source and a grant are the server's, never a client input.
    for (const forged of [{ decidedBy: 'user_x' }, { decisionSource: 'POSTURE_AUTO' }, { grantId: 'apg_1' }, { orgId: 'org_x' }]) {
      expect(decide?.inputSchema.safeParse({ ...base, ...forged }).success).toBe(false)
    }
  })

  it('registers the tool policy and the tool approval as high-risk, owner-only actions the model cannot execute', () => {
    for (const id of ['tools.set_policy', 'mcp.approve_tool'] as const) {
      const action = getActionDescriptor(id)

      expect(action?.ownerPlane).toBe('model')
      expect(action?.risk).toBe('high')
      expect(isModelExecutableAction(id)).toBe(false)
      expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain(id)
      expect(action?.description ?? '').toContain('Never model-executable')
      expect(action?.description ?? '').toContain('zero data retention')
    }
  })

  it('lets only an allow carry the reviewed digest, and takes no org from the client', () => {
    const set = getActionDescriptor('tools.set_policy')
    const digest = 'a'.repeat(64)
    const base = { capabilityId: 'cap.mcp_tool.x' }

    expect(set?.inputSchema.safeParse({ ...base, decision: 'allow', pinnedDigest: digest }).success).toBe(true)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'allow', pinnedDigest: digest, scope: 'org' }).success).toBe(true)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'ask' }).success).toBe(true)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'never', scope: 'user', reason: 'no' }).success).toBe(true)

    // An allow without the reviewed digest, and a tightening policy with one.
    expect(set?.inputSchema.safeParse({ ...base, decision: 'allow' }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'ask', pinnedDigest: digest }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'never', pinnedDigest: digest }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'allow', pinnedDigest: 'abc' }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'allow', pinnedDigest: digest.toUpperCase() }).success).toBe(false)

    expect(set?.inputSchema.safeParse({ ...base, decision: 'deny' }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'ask', scope: 'thread' }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, capabilityId: '', decision: 'ask' }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, decision: 'ask', reason: 'x'.repeat(501) }).success).toBe(false)
    for (const forged of [{ orgId: 'org_x' }, { userId: 'user_x' }, { setBy: 'user_x' }]) {
      expect(set?.inputSchema.safeParse({ ...base, decision: 'ask', ...forged }).success).toBe(false)
    }
  })

  it('approves a tool by the digest that was reviewed and nothing else', () => {
    const approve = getActionDescriptor('mcp.approve_tool')
    const base = { serverId: 'srv', tool: 'echo_read', approvedDigest: 'b'.repeat(64) }

    expect(approve?.inputSchema.safeParse(base).success).toBe(true)
    expect(approve?.inputSchema.safeParse({ ...base, approvedDigest: '' }).success).toBe(false)
    expect(approve?.inputSchema.safeParse({ ...base, approvedDigest: 'nope' }).success).toBe(false)
    expect(approve?.inputSchema.safeParse({ ...base, tool: '' }).success).toBe(false)
    expect(approve?.inputSchema.safeParse({ ...base, orgId: 'org_x' }).success).toBe(false)
  })

  it('keeps org model access approval-gated, high risk and out of reach of the model', () => {
    const action = getActionDescriptor('org.set_model_entitlement')
    const base = {
      orgId: 'org_1',
      expected_revision: 0,
      roles: { member: { allow_mode: 'listed', allow: ['gpt-5'] } },
    }

    expect(action?.ownerPlane).toBe('control')
    expect(action?.risk).toBe('high')
    expect(action?.requiresApproval).toBe(true)
    // A model must not edit which models it, or its org, may use.
    expect(isModelExecutableAction('org.set_model_entitlement')).toBe(false)

    expect(action?.inputSchema.safeParse(base).success).toBe(true)
    expect(action?.inputSchema.safeParse({ ...base, org_default: null }).success).toBe(true)
    // The revision it was edited from is required: a write is never a blind overwrite.
    expect(action?.inputSchema.safeParse({ orgId: 'org_1' }).success).toBe(false)
    expect(action?.inputSchema.safeParse({ ...base, expected_revision: -1 }).success).toBe(false)
    // Unknown fields are refused, at the top level and inside a policy.
    expect(action?.inputSchema.safeParse({ ...base, denied: ['x'] }).success).toBe(false)
    expect(action?.inputSchema.safeParse({
      ...base,
      roles: { member: { allow_mode: 'listed', denied: ['x'] } },
    }).success).toBe(false)
    expect(action?.inputSchema.safeParse({
      ...base,
      roles: { member: { allow_mode: 'some' } },
    }).success).toBe(false)
  })

  it('takes the whole router policy, the ladder and the role table included, so a save through the action resets neither', () => {
    const action = getActionDescriptor('router_policy.update')
    const row = { simple: 'gpt-4o-mini', moderate: 'gpt-4o-mini', complex: 'claude-sonnet-5-5' }
    const policy = {
      enabled: true,
      budget_cap_usd: 50,
      constrained_fraction: 0.8,
      cheap_fallback: 'gpt-4o-mini',
      complexity: {
        large_total_chars: 4000,
        large_total_chars_score: 2,
        medium_total_chars: 1200,
        medium_total_chars_score: 1,
        long_user_turn_chars: 800,
        long_user_turn_score: 1,
        code_fence_score: 1,
        keyword_score: 1,
        tool_use_score: 2,
        deep_conversation_turns: 12,
        deep_conversation_score: 1,
        moderate_threshold: 1,
        complex_threshold: 3,
        keywords: ['analyze'],
        tool_use_floors_complex: false,
      },
      table: { budget: row, balance: row, genius: row },
      tool_fallback_ladder: ['claude-sonnet-4-6', 'gpt-4o-mini'],
      roles: {
        title: { candidates: ['claude-sonnet-5-5'], effort: 'minimal' },
        research_plan: { candidates: ['claude-opus-4-8'] },
      },
    }

    const parsed = action?.inputSchema.safeParse(policy)
    expect(parsed?.success).toBe(true)
    expect(parsed?.success && parsed.data).toMatchObject({
      tool_fallback_ladder: ['claude-sonnet-4-6', 'gpt-4o-mini'],
      roles: policy.roles,
      complexity: { tool_use_floors_complex: false },
    })
    // A save that leaves the ladder or the complexity flag out would reset it on the server, so it is refused.
    expect(action?.inputSchema.safeParse({ ...policy, tool_fallback_ladder: undefined }).success).toBe(false)
    expect(action?.inputSchema.safeParse({
      ...policy,
      complexity: { ...policy.complexity, tool_use_floors_complex: undefined },
    }).success).toBe(false)
    // The role table is optional, and a role names one to four models and only the four purposes.
    expect(action?.inputSchema.safeParse({ ...policy, roles: undefined }).success).toBe(true)
    expect(action?.inputSchema.safeParse({ ...policy, roles: { title: { candidates: [] } } }).success).toBe(false)
    expect(action?.inputSchema.safeParse({
      ...policy,
      roles: { title: { candidates: ['a', 'b', 'c', 'd', 'e'] } },
    }).success).toBe(false)
    expect(action?.inputSchema.safeParse({ ...policy, roles: { summarise: { candidates: ['a'] } } }).success).toBe(false)
  })

  it('registers the memory controls with their risk: pausing is light, resetting is gated and final', () => {
    const setState = getActionDescriptor('memory.set_state')
    const reset = getActionDescriptor('memory.reset')
    const remember = getActionDescriptor('memory.remember')

    expect(setState).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(remember).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false })
    expect(reset).toMatchObject({ ownerPlane: 'model', risk: 'high', requiresApproval: true, reversible: false })
    // A model cannot pause, erase or write the user's memory on their behalf.
    for (const id of ['memory.set_state', 'memory.reset', 'memory.remember'] as const) {
      expect(isModelExecutableAction(id)).toBe(false)
    }

    // Only the three literal states; a typo must not silently mean "on".
    expect(setState?.inputSchema.safeParse({ state: 'paused' }).success).toBe(true)
    expect(setState?.inputSchema.safeParse({ state: 'ON' }).success).toBe(false)
    expect(setState?.inputSchema.safeParse({}).success).toBe(false)
    // The reset needs the literal confirmation, never a truthy stand-in.
    expect(reset?.inputSchema.safeParse({ confirm: true }).success).toBe(true)
    expect(reset?.inputSchema.safeParse({ confirm: false }).success).toBe(false)
    expect(reset?.inputSchema.safeParse({ confirm: 'yes' }).success).toBe(false)
    expect(reset?.inputSchema.safeParse({}).success).toBe(false)
    expect(remember?.inputSchema.safeParse({ threadId: 't1', content: 'Jeg heter Ima' }).success).toBe(true)
    expect(remember?.inputSchema.safeParse({ threadId: 't1', content: '   ' }).success).toBe(false)
  })

  it('registers the profile edit and rebuild as human-only and bounded like the server (EP-8 S4)', () => {
    const save = getActionDescriptor('memory.profile.save')
    const rebuild = getActionDescriptor('memory.profile.rebuild')

    expect(save).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(rebuild).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: false })
    // A model never writes the profile it is later shown.
    for (const id of ['memory.profile.save', 'memory.profile.rebuild'] as const) {
      expect(isModelExecutableAction(id)).toBe(false)
    }

    expect(save?.inputSchema.safeParse({ content: '- Prefers tables', expectedRevision: 2 }).success).toBe(true)
    expect(save?.inputSchema.safeParse({ content: 'x'.repeat(1401), expectedRevision: 2 }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ content: '- Prefers tables' }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ content: '- Prefers tables', expectedRevision: -1 }).success).toBe(false)
    expect(rebuild?.inputSchema.safeParse({ force: true }).success).toBe(true)
    expect(rebuild?.inputSchema.safeParse({}).success).toBe(false)
  })

  it('registers the connected-app reads as low-risk, ungated owner contracts the model cannot execute', () => {
    for (const id of ['integrations.browse_connection', 'integrations.fetch_connection_content'] as const) {
      const action = getActionDescriptor(id)

      expect(action).toMatchObject({ ownerPlane: 'ingestion', risk: 'low', requiresApproval: false, reversible: true })
      expect(isModelExecutableAction(id)).toBe(false)
      expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain(id)
    }
  })

  it('takes only the four connected-app listings and the closed content reads, with no body and no write', () => {
    // The enums mirror ConnectorBrowseOperation and ConnectorContentOperation in integrations-client.ts.
    expect(integrationsBrowseConnectionInput.shape.operation.options).toEqual([
      'mail.messages',
      'gmail.messages',
      'drive.files',
      'calendar.events',
    ])
    expect(integrationsFetchConnectionContentInput.shape.operation.options).toEqual([
      'drive.file.content',
      'microsoft.drive.file.content',
      'google.drive.file.content',
      'mail.messages.get',
      'microsoft.mail.messages.get',
      'google.gmail.messages.get',
      'gmail.messages.get',
    ])

    const browse = getActionDescriptor('integrations.browse_connection')?.inputSchema
    expect(browse?.safeParse({ connectionId: 'conn-1', operation: 'drive.files', params: { query: 'rapport' } }).success).toBe(true)
    expect(browse?.safeParse({ connectionId: 'conn-1', operation: 'mail.messages.send' }).success).toBe(false)
    expect(browse?.safeParse({ connectionId: '', operation: 'drive.files' }).success).toBe(false)
    expect(browse?.safeParse({ connectionId: 'conn-1', operation: 'drive.files', body: 'x' }).success).toBe(false)

    const content = getActionDescriptor('integrations.fetch_connection_content')?.inputSchema
    expect(content?.safeParse({ connectionId: 'conn-1', operation: 'gmail.messages.get', params: { id: 'm1' } }).success).toBe(true)
    expect(content?.safeParse({ connectionId: 'conn-1', operation: 'gmail.messages.get' }).success).toBe(false)
    expect(content?.safeParse({ connectionId: 'conn-1', operation: 'mail.send', params: {} }).success).toBe(false)
  })

  it('registers the connected-app choice as a model-plane action the model cannot execute, and bounds it like the gateway', () => {
    const scope = getActionDescriptor('chat.set_connector_scope')

    expect(scope).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false })
    expect(isModelExecutableAction('chat.set_connector_scope')).toBe(false)
    expect(scope?.description ?? '').toContain('Never model-executable')

    const schema = scope?.inputSchema
    expect(schema?.safeParse({ connectionIds: [] }).success).toBe(true)
    expect(schema?.safeParse({ connectionIds: ['conn-1', 'conn_2'] }).success).toBe(true)
    expect(schema?.safeParse({ connectionIds: Array.from({ length: 8 }, (_, i) => `c${i}`) }).success).toBe(true)
    expect(schema?.safeParse({ connectionIds: Array.from({ length: 9 }, (_, i) => `c${i}`) }).success).toBe(false)
    expect(schema?.safeParse({ connectionIds: ['conn-1', 'conn-1'] }).success).toBe(false)
    expect(schema?.safeParse({ connectionIds: ['conn 1'] }).success).toBe(false)
    expect(schema?.safeParse({ connectionIds: ['x'.repeat(129)] }).success).toBe(false)
    // The user, the org and the turn are the server's, never a client input.
    expect(schema?.safeParse({ connectionIds: [], userId: 'user_x' }).success).toBe(false)
  })

  it('registers the agent-mode choice as a model-plane action the model cannot execute, and accepts only the four mode ids', () => {
    const mode = getActionDescriptor('chat.set_agent_mode')

    expect(mode).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.set_agent_mode')).toBe(false)
    expect(mode?.description ?? '').toContain('Never model-executable')

    const schema = mode?.inputSchema
    for (const id of ['lockdown', 'read_only', 'ask_first', 'auto']) {
      expect(schema?.safeParse({ mode: id }).success).toBe(true)
    }
    for (const id of ['full_access', 'Auto', 'read-only', '', 'danger_full_access']) {
      expect(schema?.safeParse({ mode: id }).success).toBe(false)
    }
    // A mode is the person's choice for one turn, never a field a caller can widen with a second key.
    expect(schema?.safeParse({ mode: 'auto', rung: 'danger_full_access' }).success).toBe(false)
  })

  it('registers the transcript level as a control-plane action the model cannot execute, and accepts only the four level ids', () => {
    const density = getActionDescriptor('chat.set_transcript_density')

    expect(density).toMatchObject({ ownerPlane: 'control', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.set_transcript_density')).toBe(false)
    expect(density?.description ?? '').toContain('Never model-executable')

    const schema = density?.inputSchema
    for (const level of ['detailed', 'standard', 'compact', 'minimal']) {
      expect(schema?.safeParse({ transcriptDensity: level }).success).toBe(true)
    }
    for (const level of ['full', 'Compact', '', 'none']) {
      expect(schema?.safeParse({ transcriptDensity: level }).success).toBe(false)
    }
    // The person is the caller the server reads, never a field a client names.
    expect(schema?.safeParse({ transcriptDensity: 'minimal', userId: 'user_x' }).success).toBe(false)
  })

  function registered(id: string) {
    return actionRegistry.find((action) => action.id === id)
  }

  it('registers the org agent policy and its hook rules as model-plane actions the model cannot execute, the writes high-risk', () => {
    for (const id of [
      'org.set_agent_policy',
      'org.preview_effective_policy',
      'org.create_hook',
      'org.update_hook',
      'org.delete_hook',
    ]) {
      expect(registered(id), id).toMatchObject({ ownerPlane: 'model', requiresApproval: false, reversible: true })
      expect(isModelExecutableAction(id), id).toBe(false)
      expect(registered(id)?.description ?? '', id).toContain('model-executable')
    }
    for (const id of ['org.set_agent_policy', 'org.create_hook', 'org.update_hook', 'org.delete_hook']) {
      expect(registered(id), id).toMatchObject({ risk: 'high' })
    }
    expect(registered('org.preview_effective_policy')).toMatchObject({ risk: 'low' })
  })

  it('takes the closed policy settings and no organization from the client, and never a reserved key', () => {
    const setPolicy = registered('org.set_agent_policy')?.inputSchema

    expect(setPolicy?.safeParse({ revision: 3, settings: {} }).success).toBe(true)
    expect(
      setPolicy?.safeParse({
        revision: 3,
        settings: { mode_cap: 'read_only', rung_cap: 'workspace_write', lockdown: false, mcp_tool_approval: 'approved_only' },
      }).success,
    ).toBe(true)
    expect(
      setPolicy?.safeParse({ revision: 3, settings: { 'ext.mcp_servers': { mode: 'approved_only', ids: ['srv-a', 'srv-b'] } } })
        .success,
    ).toBe(true)
    // The revision is required and a whole number; the organization is the session's, never the body's.
    expect(setPolicy?.safeParse({ settings: {} }).success).toBe(false)
    expect(setPolicy?.safeParse({ revision: -1, settings: {} }).success).toBe(false)
    expect(setPolicy?.safeParse({ revision: 3, orgId: 'org-1', settings: {} }).success).toBe(false)
    expect(setPolicy?.safeParse({ revision: 3, settings: { orgId: 'org-1' } }).success).toBe(false)
    // Only the enforced closed values are offered: a value outside the schema, a reserved key and ids on
    // a mode that takes none are all refused.
    expect(setPolicy?.safeParse({ revision: 3, settings: { mode_cap: 'full_access' } }).success).toBe(false)
    expect(setPolicy?.safeParse({ revision: 3, settings: { 'ext.skills': { mode: 'any', ids: [] } } }).success).toBe(false)
    expect(setPolicy?.safeParse({ revision: 3, settings: { 'ext.mcp_servers': { mode: 'any', ids: ['srv-a'] } } }).success).toBe(
      false,
    )
  })

  it('keeps a post-tool rule to a denial, takes no organization, and bounds the matcher like the engine', () => {
    const create = registered('org.create_hook')?.inputSchema

    expect(create?.safeParse({ event: 'pre_tool', toolMatcher: 'shell', decision: 'deny', reason: 'no shell' }).success).toBe(true)
    expect(
      create?.safeParse({ event: 'pre_tool', toolMatcher: 'mcp__crm_*', decision: 'ask', reason: 'check first', priority: 5 })
        .success,
    ).toBe(true)
    expect(create?.safeParse({ event: 'post_tool', decision: 'deny', reason: 'withhold the result' }).success).toBe(true)
    expect(create?.safeParse({ event: 'post_tool', decision: 'ask', reason: 'check first' }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', decision: 'allow', reason: 'open' }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', decision: 'deny', reason: '   ' }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', toolMatcher: '../shell', decision: 'deny', reason: 'no' }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', toolMatcher: 'shell*x', decision: 'deny', reason: 'no' }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', decision: 'deny', reason: 'no', priority: 1.5 }).success).toBe(false)
    expect(create?.safeParse({ event: 'pre_tool', decision: 'deny', reason: 'no', orgId: 'org-1' }).success).toBe(false)

    const update = registered('org.update_hook')?.inputSchema
    expect(update?.safeParse({ hookId: 'ohr_1', event: 'pre_tool', decision: 'deny', reason: 'no shell' }).success).toBe(true)
    expect(update?.safeParse({ event: 'pre_tool', decision: 'deny', reason: 'no shell' }).success).toBe(false)
    expect(update?.safeParse({ hookId: 'ohr_1', event: 'post_tool', decision: 'ask', reason: 'no' }).success).toBe(false)

    const retire = registered('org.delete_hook')?.inputSchema
    expect(retire?.safeParse({ hookId: 'ohr_1' }).success).toBe(true)
    expect(retire?.safeParse({ hookId: 'ohr_1', orgId: 'org-1' }).success).toBe(false)
  })

  it('lets a chat project reuse the registered Space actions, and its contract grants nothing', () => {
    // A project is a Space, so creating one and editing its instructions are the Space
    // actions the room and Space pages already use: no second route and no new authority.
    const createRoomAction = getActionDescriptor('spaces.create_room')
    const instructionsAction = getActionDescriptor('spaces.update_space_instructions')

    expect(createRoomAction?.ownerPlane).toBe('application')
    expect(createRoomAction?.requiresApproval).toBe(false)
    expect(createRoomAction?.inputSchema.safeParse({ kind: 'room', name: 'Launch' }).success).toBe(true)
    // A project is a named room: a personal Space is not created from the project list.
    expect(createRoomAction?.inputSchema.safeParse({ kind: 'personal', name: 'Mine' }).success).toBe(false)
    expect(createRoomAction?.inputSchema.safeParse({ kind: 'room', name: '   ' }).success).toBe(false)

    expect(instructionsAction?.ownerPlane).toBe('application')
    expect(instructionsAction?.inputSchema.safeParse({ spaceRef: 'space_1', instructions: 'Answer in numbered steps.' }).success).toBe(true)
    expect(instructionsAction?.inputSchema.safeParse({ spaceRef: '', instructions: 'Answer in numbered steps.' }).success).toBe(false)
    // Writing the instructions is the gateway's editor, manager or owner floor; the descriptor says so.
    expect(instructionsAction?.description).toContain('editor, manager, or owner')

    // The project shape describes where a chat goes. A viewer is not a role a project chat can start from.
    expect(chatProjectSchema.safeParse({ spaceRef: 'space_1', name: 'Launch', kind: 'room', role: 'editor' }).success).toBe(true)
    expect(chatProjectSchema.safeParse({ spaceRef: 'space_1', name: 'Launch', kind: 'room', role: 'viewer' }).success).toBe(false)
    expect(chatProjectSchema.safeParse({ spaceRef: 'space_1', name: 'Launch', kind: 'organization', role: 'owner' }).success).toBe(false)
  })

  it('registers the human post actions as low-risk, ungated, model-plane actions the model cannot execute (WS01-S5)', () => {
    for (const id of ['spaces.create_post', 'spaces.reply_in_thread'] as const) {
      const action = getActionDescriptor(id)
      expect(action, id).toBeDefined()
      expect(action?.ownerPlane).toBe('model')
      expect(action?.risk).toBe('low')
      expect(action?.requiresApproval).toBe(false)
      expect(action?.reversible).toBe(false)
      // A post is a person speaking in a room, never an agent proposal.
      expect(isModelExecutableAction(id)).toBe(false)
    }
    const send = 'send_id_0123456789ab'
    const create = getActionDescriptor('spaces.create_post')
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'hello', clientSendId: send }).success).toBe(true)
    // The send id is required and shaped like the gateway's ^[A-Za-z0-9_-]{16,64}$.
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'hello' }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'hello', clientSendId: 'short' }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'hello', clientSendId: 'has space 0123456789' }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: '   ', clientSendId: send }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'x'.repeat(8001), clientSendId: send }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'x'.repeat(8000), clientSendId: send }).success).toBe(true)
    const reply = getActionDescriptor('spaces.reply_in_thread')
    expect(reply?.inputSchema.safeParse({ spaceRef: 'space-room', threadId: 'thread-1', content: 'hi', clientSendId: send }).success).toBe(true)
    expect(reply?.inputSchema.safeParse({ spaceRef: 'space-room', content: 'hi', clientSendId: send }).success).toBe(false)
  })

  it('registers the Space effect-policy actions as high-risk, approval-gated, control-plane actions the model cannot execute (WS03-S4)', () => {
    for (const id of ['spaces.set_effect_entitlements', 'spaces.apply_effect_preset'] as const) {
      const action = getActionDescriptor(id)
      expect(action, id).toBeDefined()
      expect(action?.ownerPlane).toBe('control')
      expect(action?.risk).toBe('high')
      expect(action?.requiresApproval).toBe(true)
      expect(action?.reversible).toBe(true)
      // An agent must never widen the entitlement floor.
      expect(isModelExecutableAction(id)).toBe(false)
    }
    const set = getActionDescriptor('spaces.set_effect_entitlements')
    const base = { orgId: 'org_1', expectedEntitlementRevision: 2, entitlements: { effects_enabled: true } }
    expect(set?.inputSchema.safeParse(base).success).toBe(true)
    // The revision the edit started from is required: never a blind overwrite.
    expect(set?.inputSchema.safeParse({ orgId: 'org_1', entitlements: {} }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, expectedEntitlementRevision: -1 }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, entitlements: { 'Bad-Key': true } }).success).toBe(false)
    expect(set?.inputSchema.safeParse({ ...base, entitlements: { effects_enabled: 'yes' } }).success).toBe(false)
    const preset = getActionDescriptor('spaces.apply_effect_preset')
    expect(preset?.inputSchema.safeParse({ orgId: 'org_1', expectedEntitlementRevision: 0, presetId: 'balanced' }).success).toBe(true)
    expect(preset?.inputSchema.safeParse({ orgId: 'org_1', expectedEntitlementRevision: 0 }).success).toBe(false)
    expect(preset?.inputSchema.safeParse({ orgId: 'org_1', expectedEntitlementRevision: 0, presetId: 'p'.repeat(49) }).success).toBe(false)
  })

  // P-SPA1 moved the spaces.* entries into per-workstream fragments under
  // domains/spaces/*.ts and spread them once. The move must not add, drop, rename
  // or reorder an id: the order is what the contract probes and the context pack read.
  it('registers the six schedule contracts the automations hub calls, as model-plane actions the model cannot execute', () => {
    const scheduleIds = [
      'schedule.list',
      'schedule.pause',
      'schedule.resume',
      'schedule.update_time',
      'schedule.delete',
      'schedule.fires.list',
    ] as const

    for (const id of scheduleIds) {
      const action = getActionDescriptor(id)
      expect(action?.ownerPlane, id).toBe('model')
      expect(action?.requiresApproval, id).toBe(false)
      expect(isModelExecutableAction(id), id).toBe(false)
    }
    expect(getActionDescriptor('schedule.delete')).toMatchObject({ risk: 'high', reversible: false })
  })

  it('takes the schedule id, the cron expression and the fires limit, and nothing else the hub never sends', () => {
    const pause = getActionDescriptor('schedule.pause')
    expect(pause?.inputSchema.safeParse({ scheduleId: 'cron_1' }).success).toBe(true)
    expect(pause?.inputSchema.safeParse({ scheduleId: '' }).success).toBe(false)
    expect(pause?.inputSchema.safeParse({ scheduleId: 'cron_1', enabled: false }).success).toBe(false)

    // The time change is a PATCH of the cadence alone: a goal, an approval or a creator is refused.
    const updateTime = getActionDescriptor('schedule.update_time')
    expect(updateTime?.inputSchema.safeParse({ scheduleId: 'cron_1', scheduleExpr: '0 8 * * 1-5' }).success).toBe(true)
    for (const extra of ['description', 'enabled', 'task_template', 'creator_subject_id']) {
      expect(
        updateTime?.inputSchema.safeParse({ scheduleId: 'cron_1', scheduleExpr: '0 8 * * 1-5', [extra]: 'x' }).success,
        extra,
      ).toBe(false)
    }

    const firesList = getActionDescriptor('schedule.fires.list')
    expect(firesList?.inputSchema.safeParse({ scheduleId: 'cron_1', limit: 5 }).success).toBe(true)
    expect(firesList?.inputSchema.safeParse({ scheduleId: 'cron_1', limit: 0 }).success).toBe(false)
    expect(firesList?.inputSchema.safeParse({ scheduleId: 'cron_1', limit: 101 }).success).toBe(false)

    // The list takes no filter from the caller: the creator scope is applied by the hub, not by a parameter.
    const list = getActionDescriptor('schedule.list')
    expect(list?.inputSchema.safeParse({}).success).toBe(true)
    expect(list?.inputSchema.safeParse({ creator: 'someone-else' }).success).toBe(false)
  })

  it('closes each schedule contract in the side map with the reason that matches what it does', () => {
    expect(MODEL_ACCESS['schedule.list']).toMatchObject({
      access: 'none',
      exemptReason: 'no_owner_executor',
      sideEffectClass: 'read',
    })
    expect(MODEL_ACCESS['schedule.fires.list']).toMatchObject({
      access: 'none',
      exemptReason: 'no_owner_executor',
      sideEffectClass: 'read',
    })
    // A model that could pause, resume or retime its own schedule would govern its own autonomy.
    for (const id of ['schedule.pause', 'schedule.resume', 'schedule.update_time'] as const) {
      expect(MODEL_ACCESS[id], id).toMatchObject({ access: 'none', exemptReason: 'agent_cannot_self_govern' })
    }
    expect(MODEL_ACCESS['schedule.delete']).toMatchObject({
      access: 'none',
      exemptReason: 'irreversible_destructive',
      sideEffectClass: 'destructive',
    })
  })

  it('registers the agent installations read as a low-risk, ungated application contract the model cannot execute', () => {
    const action = getActionDescriptor('agents.installations.list')
    expect(action).toMatchObject({ ownerPlane: 'application', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('agents.installations.list')).toBe(false)
    expect(MODEL_ACCESS['agents.installations.list']).toMatchObject({
      access: 'none',
      exemptReason: 'no_owner_executor',
      sideEffectClass: 'read',
    })

    // The organisation comes from the session, so the read takes no parameter a caller could widen.
    expect(action?.inputSchema.safeParse({}).success).toBe(true)
    expect(action?.inputSchema.safeParse({ orgId: 'org_other' }).success).toBe(false)
  })

  it('registers the agent builder contracts as application actions the model cannot execute (EP-6-S8)', () => {
    for (const id of ['agents.draft.save', 'agents.publish', 'agents.versions.list'] as const) {
      const action = getActionDescriptor(id)
      expect(action?.ownerPlane, id).toBe('application')
      expect(action?.requiresApproval, id).toBe(false)
      expect(isModelExecutableAction(id), id).toBe(false)
    }
    expect(getActionDescriptor('agents.versions.list')).toMatchObject({ risk: 'low', reversible: true })
    // A model that could rewrite or publish an agent's instructions would govern its own behaviour.
    for (const id of ['agents.draft.save', 'agents.publish'] as const) {
      expect(MODEL_ACCESS[id], id).toMatchObject({ access: 'none', exemptReason: 'agent_cannot_self_govern' })
    }
    expect(MODEL_ACCESS['agents.versions.list']).toMatchObject({
      access: 'none',
      exemptReason: 'no_owner_executor',
      sideEffectClass: 'read',
    })
  })

  it('takes a draft of capability ids and a bounded autonomy cap, and nothing the builder never sends', () => {
    const save = getActionDescriptor('agents.draft.save')
    const draft = {
      name: 'Driftsassistent',
      system_prompt: 'Svar kort.',
      model: 'Verevon Balance',
      tool_capability_ids: ['cap.tool.ticket.create'],
      autonomy_cap: 'ask_first',
    }
    expect(save?.inputSchema.safeParse({ agentRef: 'agent_1', draft }).success).toBe(true)
    // The cap never widens to lockdown, and a free tool name is not a capability id.
    expect(save?.inputSchema.safeParse({ agentRef: 'agent_1', draft: { ...draft, autonomy_cap: 'lockdown' } }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ agentRef: 'agent_1', draft: { ...draft, tool_capability_ids: ['Send Email'] } }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ agentRef: 'agent_1', draft: { ...draft, system_prompt: 'x'.repeat(4001) } }).success).toBe(false)
    for (const extra of ['version', 'digest', 'published_by']) {
      expect(save?.inputSchema.safeParse({ agentRef: 'agent_1', draft: { ...draft, [extra]: 'x' } }).success, extra).toBe(false)
    }

    // Publishing takes the agent alone: the version number and digest are the server's.
    const publish = getActionDescriptor('agents.publish')
    expect(publish?.inputSchema.safeParse({ agentRef: 'agent_1' }).success).toBe(true)
    expect(publish?.inputSchema.safeParse({ agentRef: 'agent_1', version: 3 }).success).toBe(false)
    expect(publish?.inputSchema.safeParse({ agentRef: '' }).success).toBe(false)
  })

  it('keeps every action id unique and the spaces.* entries contiguous and in their pre-fragment order (P-SPA1)', () => {
    const ids: string[] = actionRegistry.map((action) => action.id)
    expect(new Set(ids).size).toBe(ids.length)
    // 187 before the move, plus the seven W1 additions (create_post, reply_in_thread, the two
    // effect-policy actions, update_room_details, change_member_role, leave_room), plus the
    // eight actions main added on its own side (195 before the Space merge).
    // 243 after main was merged into model-plane (2026-11-02): main's 202 plus model-plane's own actions.
    // 249 after EP-6-S1 registered the six schedule contracts the automations hub calls.
    // 250 after EP-6-S5 registered agents.installations.list, the read the agents landing shows.
    // 251 after EP-8-S3 registered settings.set_tone_preset, the caller's own reply tone.
    // 253 after EP-8-S4 registered memory.profile.save and memory.profile.rebuild; that slice did not move this count.
    // 255 after EP-8-S8 registered settings.complete_personal_setup and settings.skip_personal_setup, the first-run step.
    // 256 after EP-14-S2 registered chat.set_transcript_density, the person's transcript level.
    // 259 after EP-6-S8 registered agents.draft.save, agents.publish and agents.versions.list.
    // 269 after w23/integ (Quarry W2/W3) was merged with model-plane: the four Innhenting actions of the
    // ingestions and monitoring domains (job_control, start_agent_run, create_monitor, delete_monitor),
    // the six ingestions run and schedule controls (get_run_status, pause_run, resume_run, cancel_run,
    // pause_schedule, change_schedule_state) that replace run_schedule_action, and browser_run.steer.
    expect(ids).toHaveLength(269)

    const spaceIds = ids.filter((id) => id.startsWith('spaces.'))
    const first = ids.indexOf(spaceIds[0] as string)
    expect(ids.slice(first, first + spaceIds.length)).toEqual(spaceIds)

    // The fourteen entries the fragment move carried, exactly as they stood in action-registry.ts.
    expect(spaceIds.slice(0, 14)).toEqual([
      'spaces.create_personal_space',
      'spaces.ensure_organization_room',
      'spaces.update_space_instructions',
      'spaces.create_space_agent',
      'spaces.bind_space_agent',
      'spaces.request_personal_space_deletion',
      'spaces.update_thread_presentation',
      'spaces.mark_read',
      'spaces.record_presence',
      'spaces.set_agent_state',
      'spaces.revoke_agent',
      'spaces.create_room',
      'spaces.add_member',
      'spaces.remove_member',
    ])
    // The later workstream fragments follow them in fragment order (ws01, ws03, ws04).
    expect(spaceIds.slice(14)).toEqual([
      'spaces.create_post',
      'spaces.reply_in_thread',
      'spaces.set_effect_entitlements',
      'spaces.apply_effect_preset',
      'spaces.update_room_details',
      'spaces.change_member_role',
      'spaces.leave_room',
    ])
  })
})
