import { describe, expect, it } from 'vitest'
import { actionRegistry, getActionDescriptor } from '@/shared/actions/action-registry'
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
})
