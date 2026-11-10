import { describe, expect, it } from 'vitest'
import {
  actionRegistry,
  getActionDescriptor,
  integrationsBrowseConnectionInput,
  integrationsConnectorPoliciesOutput,
  integrationsFetchConnectionContentInput,
  integrationsSetConnectorPolicyInput,
} from '@/shared/actions/action-registry'
import { spaceActions } from '@/shared/actions/domains/spaces'
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

  it('takes an edit with an approval only, within the mail limits (EP-10-S6)', () => {
    const fingerprint = 'a'.repeat(64)
    const edit = { patch: { subject: 'New subject' }, baseFingerprint: fingerprint }
    for (const id of ['approvals.decide', 'orchestration.decide_approval'] as const) {
      const schema = getActionDescriptor(id)?.inputSchema
      const base = { approvalId: 'appr_1', decision: 'approve' as const }

      expect(schema?.safeParse({ ...base, edit }).success).toBe(true)
      expect(
        schema?.safeParse({
          ...base,
          edit: { patch: { to: ['a@example.com'], bodyText: 'Hei' }, baseFingerprint: fingerprint },
        }).success,
      ).toBe(true)

      // A rejection sends nothing, so it carries no edit.
      expect(schema?.safeParse({ ...base, decision: 'reject', edit }).success).toBe(false)
      // The draft the person saw is named by its 64-hex fingerprint.
      expect(schema?.safeParse({ ...base, edit: { ...edit, baseFingerprint: 'abc' } }).success).toBe(false)
      // Only recipients, subject and body: the connection, operation and threading are frozen.
      for (const forged of [{ connectionId: 'conn-2' }, { operation: 'gmail.delete' }, { threadId: 't' }, { bodyHtml: '<b>x</b>' }]) {
        expect(schema?.safeParse({ ...base, edit: { ...edit, patch: { ...edit.patch, ...forged } } }).success).toBe(false)
      }
      expect(schema?.safeParse({ ...base, edit: { ...edit, extra: true } }).success).toBe(false)
      // An empty patch, a header-breaking subject, no recipients and too many are refused.
      for (const patch of [
        {},
        { subject: 'a\r\nBcc: x@example.com' },
        { to: [] },
        { to: Array.from({ length: 51 }, (_, i) => `r${i}@example.com`) },
        { bodyText: 'x'.repeat(48 * 1024 + 1) },
      ]) {
        expect(schema?.safeParse({ ...base, edit: { patch, baseFingerprint: fingerprint } }).success).toBe(false)
      }
    }
  })

  it('registers the approval preview as a low-risk read the model cannot execute (EP-10-S1)', () => {
    const preview = getActionDescriptor('approvals.preview')

    expect(preview?.ownerPlane).toBe('model')
    expect(preview?.risk).toBe('low')
    expect(preview?.requiresApproval).toBe(false)
    expect(isModelExecutableAction('approvals.preview')).toBe(false)
    expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain(
      'approvals.preview',
    )
    const description = preview?.description ?? ''
    expect(description).toContain('approval_not_found')
    expect(description).toContain('preview_unavailable')
    expect(description).toContain('Never model-executable')

    // The approval id is the one input: the org and the owner are the credential's.
    expect(preview?.inputSchema.safeParse({ approvalId: 'appr_1' }).success).toBe(true)
    expect(preview?.inputSchema.safeParse({ approvalId: '' }).success).toBe(false)
    expect(preview?.inputSchema.safeParse({ approvalId: 'appr_1', orgId: 'org_x' }).success).toBe(false)

    const shaped = {
      kind: 'mail_send',
      provider: 'google',
      operation: 'gmail.send',
      connectionId: 'conn-1',
      to: ['a@example.com'],
      subject: 'S',
      bodyText: 'Hei',
      hasHtml: false,
      isReply: false,
      rawOnly: false,
      editableFields: ['to', 'subject', 'body_text'],
      actionFingerprint: 'f'.repeat(64),
      revision: 0,
    }
    expect(preview?.outputSchema.safeParse(shaped).success).toBe(true)
    expect(preview?.outputSchema.safeParse({ ...shaped, kind: 'calendar_event' }).success).toBe(false)
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

  it('keeps the retention window and legal holds approval-gated, high risk and out of reach of the model', () => {
    for (const id of ['org.set_retention', 'org.place_legal_hold', 'org.release_legal_hold'] as const) {
      const action = getActionDescriptor(id)
      expect(action?.ownerPlane).toBe('control')
      expect(action?.risk).toBe('high')
      expect(action?.requiresApproval).toBe(true)
      // A model must not shorten what its org keeps, nor lift a hold on evidence.
      expect(isModelExecutableAction(id)).toBe(false)
    }
    expect(getActionDescriptor('org.release_legal_hold')?.reversible).toBe(false)

    const setRetention = getActionDescriptor('org.set_retention')
    const base = { orgId: 'org_1', expected_revision: 0, thread_days: 90 }
    expect(setRetention?.inputSchema.safeParse(base).success).toBe(true)
    // Null is keep; the bounds are org-core's 30..3650 days.
    expect(setRetention?.inputSchema.safeParse({ ...base, thread_days: null }).success).toBe(true)
    expect(setRetention?.inputSchema.safeParse({ ...base, thread_days: 29 }).success).toBe(false)
    expect(setRetention?.inputSchema.safeParse({ ...base, thread_days: 3651 }).success).toBe(false)
    expect(setRetention?.inputSchema.safeParse({ ...base, thread_days: 90.5 }).success).toBe(false)
    expect(setRetention?.inputSchema.safeParse({ orgId: 'org_1', thread_days: 90 }).success).toBe(false)
    expect(setRetention?.inputSchema.safeParse({ ...base, days: 90 }).success).toBe(false)

    const place = getActionDescriptor('org.place_legal_hold')
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'org', hold_ref: 'Matter 17' }).success).toBe(true)
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'thread', thread_id: 'thr_1', hold_ref: 'Matter 17' }).success).toBe(true)
    // An org hold names no thread; a thread hold must name one; the reference is required.
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'org', thread_id: 'thr_1', hold_ref: 'M' }).success).toBe(false)
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'thread', hold_ref: 'M' }).success).toBe(false)
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'org', hold_ref: '  ' }).success).toBe(false)
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'org', hold_ref: 'r'.repeat(201) }).success).toBe(false)
    expect(place?.inputSchema.safeParse({ orgId: 'org_1', scope: 'space', hold_ref: 'M' }).success).toBe(false)

    const release = getActionDescriptor('org.release_legal_hold')
    expect(release?.inputSchema.safeParse({ orgId: 'org_1', holdId: `lh_${'a'.repeat(32)}` }).success).toBe(true)
    expect(release?.inputSchema.safeParse({ orgId: 'org_1', holdId: 'lh_short' }).success).toBe(false)
    expect(release?.inputSchema.safeParse({ orgId: 'org_1', holdId: `lh_${'A'.repeat(32)}` }).success).toBe(false)
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

  it('registers the connected-app levels as low-risk model-plane contracts the model cannot execute (EP-10 S7)', () => {
    for (const id of ['integrations.set_connector_policy', 'integrations.list_connector_policies'] as const) {
      const action = getActionDescriptor(id)

      expect(action).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
      expect(isModelExecutableAction(id)).toBe(false)
      expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain(id)
      expect(action?.description ?? '').toContain('Never model-executable')
    }
  })

  it('takes only tightening levels, never an allow, and nothing a client does not own', () => {
    const set = integrationsSetConnectorPolicyInput
    for (const level of ['use_without_asking', 'ask_first', 'off']) {
      expect(set.safeParse({ kind: 'read', level }).success).toBe(true)
    }
    for (const level of ['always_ask', 'off']) {
      expect(set.safeParse({ kind: 'write', level }).success).toBe(true)
    }
    // A write is never released from asking, and an ask is not a separate level for it.
    expect(set.safeParse({ kind: 'write', level: 'use_without_asking' }).success).toBe(false)
    expect(set.safeParse({ kind: 'write', level: 'ask_first' }).success).toBe(false)
    expect(set.safeParse({ kind: 'read', level: 'always_ask' }).success).toBe(false)
    expect(set.safeParse({ kind: 'read', level: 'allow' }).success).toBe(false)
    expect(set.safeParse({ kind: 'delete', level: 'off' }).success).toBe(false)
    for (const forged of [{ scope: 'org' }, { orgId: 'org_x' }, { userId: 'user_x' }, { capabilityId: 'cap.x' }]) {
      expect(set.safeParse({ kind: 'read', level: 'off', ...forged }).success).toBe(false)
    }

    expect(
      integrationsConnectorPoliciesOutput.safeParse({
        enforced: false,
        read: 'ask_first',
        write: 'always_ask',
        orgRead: 'use_without_asking',
        orgWrite: 'off',
      }).success,
    ).toBe(true)
    expect(
      integrationsConnectorPoliciesOutput.safeParse({
        enforced: true,
        read: 'ask_first',
        write: 'ask_first',
        orgRead: 'use_without_asking',
        orgWrite: 'always_ask',
      }).success,
    ).toBe(false)
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

  it('registers the connected-app suggestion card as a low-risk model-plane action the model cannot execute', () => {
    const accept = getActionDescriptor('chat.accept_connector_suggestion')

    expect(accept).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.accept_connector_suggestion')).toBe(false)
    expect(accept?.description ?? '').toContain('Never model-executable')

    const schema = accept?.inputSchema
    expect(schema?.safeParse({ path: 'connect', suggestionId: 'sug_01', provider: 'google' }).success).toBe(true)
    expect(
      schema?.safeParse({ path: 'use', suggestionId: 'sug_01', provider: 'microsoft', connectionId: 'conn_1' }).success,
    ).toBe(true)
    // The provider list is closed, as the gateway's is.
    expect(schema?.safeParse({ path: 'connect', suggestionId: 'sug_01', provider: 'slack' }).success).toBe(false)
    expect(schema?.safeParse({ path: 'connect', suggestionId: 'sug_01', provider: 'Google' }).success).toBe(false)
    // Using an account names it; connecting does not.
    expect(schema?.safeParse({ path: 'use', suggestionId: 'sug_01', provider: 'google' }).success).toBe(false)
    expect(
      schema?.safeParse({ path: 'connect', suggestionId: 'sug_01', provider: 'google', connectionId: 'conn_1' }).success,
    ).toBe(false)
    expect(
      schema?.safeParse({ path: 'use', suggestionId: 'sug_01', provider: 'google', connectionId: 'conn 1' }).success,
    ).toBe(false)
    expect(schema?.safeParse({ path: 'rank', suggestionId: 'sug_01', provider: 'google' }).success).toBe(false)
    expect(schema?.safeParse({ path: 'connect', suggestionId: '', provider: 'google' }).success).toBe(false)
    // The org and the person come from the session.
    expect(
      schema?.safeParse({ path: 'connect', suggestionId: 'sug_01', provider: 'google', orgId: 'org_x' }).success,
    ).toBe(false)
  })

  it('registers the context chips as model-plane actions the model cannot execute, and bounds them like the gateway', () => {
    const set = getActionDescriptor('chat.set_context_refs')
    expect(set).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.set_context_refs')).toBe(false)
    expect(set?.description ?? '').toContain('Never model-executable')

    const schema = set?.inputSchema
    const doc = { kind: 'document', id: 'doc-1', label: 'Budsjett' }
    expect(schema?.safeParse({ contextRefs: [] }).success).toBe(true)
    expect(schema?.safeParse({ contextRefs: [doc, { kind: 'skill', id: 'skill.a', label: 'Skriv' }] }).success).toBe(true)
    expect(schema?.safeParse({ contextRefs: Array.from({ length: 20 }, (_, i) => ({ ...doc, id: `d${i}` })) }).success).toBe(true)
    expect(schema?.safeParse({ contextRefs: Array.from({ length: 21 }, (_, i) => ({ ...doc, id: `d${i}` })) }).success).toBe(false)
    expect(schema?.safeParse({ contextRefs: [doc, doc] }).success).toBe(false)
    expect(schema?.safeParse({ contextRefs: [{ ...doc, id: 'a b' }] }).success).toBe(false)
    expect(schema?.safeParse({ contextRefs: [{ ...doc, label: 'x'.repeat(121) }] }).success).toBe(false)
    // A conversation and a selection are not chips yet (EP-3 S6/S7), and a reference carries no content.
    expect(schema?.safeParse({ contextRefs: [{ ...doc, kind: 'thread' }] }).success).toBe(false)
    expect(schema?.safeParse({ contextRefs: [{ ...doc, kind: 'artifact_selection' }] }).success).toBe(false)
    expect(schema?.safeParse({ contextRefs: [{ ...doc, content: 'text' }] }).success).toBe(false)

    const pick = getActionDescriptor('chat.pick_context_ref')
    expect(pick).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false })
    expect(isModelExecutableAction('chat.pick_context_ref')).toBe(false)
    expect(pick?.inputSchema.safeParse({ tab: 'documents' }).success).toBe(true)
    expect(pick?.inputSchema.safeParse({ tab: 'threads' }).success).toBe(false)
  })

  it('registers the selection edit as a model-plane action the model cannot execute, and bounds its range like the gateway', () => {
    const edit = getActionDescriptor('chat.edit_artifact_selection')
    expect(edit).toMatchObject({ ownerPlane: 'model', risk: 'medium', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.edit_artifact_selection')).toBe(false)
    expect(edit?.description ?? '').toContain('Never model-executable')

    const schema = edit?.inputSchema
    const valid = { artifactId: 'art-1', version: 2, start: 0, end: 5, quoteSha256: 'a'.repeat(64), instruction: 'Gjør formelt' }
    expect(schema?.safeParse(valid).success).toBe(true)
    expect(schema?.safeParse({ ...valid, quoteSha256: 'A'.repeat(64) }).success).toBe(false)
    expect(schema?.safeParse({ ...valid, quoteSha256: 'a'.repeat(63) }).success).toBe(false)
    expect(schema?.safeParse({ ...valid, start: -1 }).success).toBe(false)
    expect(schema?.safeParse({ ...valid, version: 0 }).success).toBe(false)
    expect(schema?.safeParse({ ...valid, instruction: '' }).success).toBe(false)
    // The text of the selection is never an input: only its hash rides in the request.
    expect(schema?.safeParse({ ...valid, text: 'Første avsnitt' }).success).toBe(false)
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

  it('registers the request inspector as a low-risk, read-only model action the model cannot execute (EP-14-S7)', () => {
    const inspect = getActionDescriptor('chat.inspect_run_request')

    expect(inspect).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.inspect_run_request')).toBe(false)
    expect(MODEL_ACCESS['chat.inspect_run_request']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = inspect?.inputSchema
    expect(schema?.safeParse({ runId: 'run_01H', callSeq: 1 }).success).toBe(true)
    for (const input of [
      { runId: 'run.1', callSeq: 1 },
      { runId: '', callSeq: 1 },
      { runId: 'run_1', callSeq: 0 },
      { runId: 'run_1', callSeq: 1.5 },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
    // The owner is read from the session, never named by a client.
    expect(schema?.safeParse({ runId: 'run_1', callSeq: 1, userId: 'user_x' }).success).toBe(false)
  })

  it('registers the thread export as a low-risk, owner-only model-plane read the model cannot execute (EP-12-S11)', () => {
    const exportThread = getActionDescriptor('thread.export')

    expect(exportThread).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('thread.export')).toBe(false)
    expect(buildModelContextPack({ route: '/chat', visibleItems: [] }).availableActions).not.toContain('thread.export')
    expect(MODEL_ACCESS['thread.export']).toMatchObject({ access: 'none', eligible: false, sideEffectClass: 'read' })
    expect(exportThread?.description ?? '').toContain('Never model-executable')
    expect(exportThread?.description ?? '').toContain('redacted')

    // Both choices are named, the thread is named, and nothing else: the owner is the session's.
    const schema = exportThread?.inputSchema
    expect(schema?.safeParse({ threadId: 'thread-1', format: 'json', profile: 'full' }).success).toBe(true)
    expect(schema?.safeParse({ threadId: 'thread-1', format: 'markdown', profile: 'redacted' }).success).toBe(true)
    expect(schema?.safeParse({ threadId: 'thread-1', format: 'json' }).success).toBe(false)
    expect(schema?.safeParse({ threadId: 'thread-1', format: 'csv', profile: 'full' }).success).toBe(false)
    expect(schema?.safeParse({ threadId: '', format: 'json', profile: 'full' }).success).toBe(false)
    for (const forged of [{ userId: 'user_x' }, { orgId: 'org_x' }]) {
      expect(schema?.safeParse({ threadId: 'thread-1', format: 'json', profile: 'full', ...forged }).success).toBe(false)
    }

    // The answer is the file: its media type and download name, not a JSON envelope.
    expect(exportThread?.outputSchema.safeParse({
      contentType: 'text/markdown; charset=utf-8',
      filename: 'verevon-thread-thread-1.md',
    }).success).toBe(true)
    expect(exportThread?.outputSchema.safeParse({ status: 'ok' }).success).toBe(false)
  })

  it('registers the thread goal actions as person-only writes with the server bounds (EP-7-S1)', () => {
    for (const id of ['chat.set_goal', 'chat.update_goal', 'chat.clear_goal'] as const) {
      expect(getActionDescriptor(id)).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false })
      expect(isModelExecutableAction(id)).toBe(false)
    }
    expect(getActionDescriptor('chat.continue_goal')).toMatchObject({
      ownerPlane: 'model',
      risk: 'medium',
      requiresApproval: false,
    })
    expect(isModelExecutableAction('chat.continue_goal')).toBe(false)

    const set = getActionDescriptor('chat.set_goal')?.inputSchema
    expect(set?.safeParse({ threadId: 't1', objective: 'Ship the report', successCriteria: ['Reconciled'] }).success).toBe(true)
    for (const input of [
      { threadId: 't1', objective: '' },
      { threadId: 't1', objective: 'x'.repeat(2001) },
      { threadId: 't1', objective: 'aim', successCriteria: Array.from({ length: 9 }, (_, i) => `c${i}`) },
      { threadId: 't1', objective: 'aim', successCriteria: ['x'.repeat(501)] },
      { threadId: 't1', objective: 'aim', maxTurns: 51 },
      // The owner is read from the session, never named by a client.
      { threadId: 't1', objective: 'aim', userId: 'user_x' },
    ]) {
      expect(set?.safeParse(input).success).toBe(false)
    }

    const update = getActionDescriptor('chat.update_goal')?.inputSchema
    expect(update?.safeParse({ threadId: 't1', goalId: 'goal_1', expectedRevision: 2, status: 'paused' }).success).toBe(true)
    // A person never writes `met` or a stop state; the server does.
    expect(update?.safeParse({ threadId: 't1', goalId: 'goal_1', expectedRevision: 2, status: 'met' }).success).toBe(false)
    expect(update?.safeParse({ threadId: 't1', goalId: 'goal_1', status: 'paused' }).success).toBe(false)

    const go = getActionDescriptor('chat.continue_goal')?.inputSchema
    expect(go?.safeParse({ threadId: 't1', goalId: 'goal_1' }).success).toBe(true)
    expect(go?.safeParse({ threadId: 't1' }).success).toBe(false)
  })

  it('registers Keep going as a person-only switch that grants nothing (EP-7-S4)', () => {
    expect(getActionDescriptor('chat.goal_auto_toggle')).toMatchObject({
      ownerPlane: 'model',
      risk: 'medium',
      requiresApproval: false,
      reversible: true,
    })
    // A goal never continues itself: the model can neither run the pump nor turn it on.
    expect(isModelExecutableAction('chat.goal_auto_toggle')).toBe(false)

    const toggle = getActionDescriptor('chat.goal_auto_toggle')?.inputSchema
    expect(toggle?.safeParse({ threadId: 't1', goalId: 'goal_1', on: true }).success).toBe(true)
    expect(toggle?.safeParse({ threadId: 't1', goalId: 'goal_1', on: false }).success).toBe(true)
    for (const input of [
      { threadId: 't1', goalId: 'goal_1' },
      { threadId: 't1', on: true },
      { threadId: 't1', goalId: 'goal_1', on: 'true' },
      // A switch carries no approval: the pump never decides anything for the person.
      { threadId: 't1', goalId: 'goal_1', on: true, approve: true },
    ]) {
      expect(toggle?.safeParse(input).success).toBe(false)
    }
  })

  it('registers artifact edit and restore as reversible person-only writes (EP-3-S4)', () => {
    const save = getActionDescriptor('chat.save_artifact_revision')
    const restore = getActionDescriptor('chat.restore_artifact_revision')

    expect(save).toMatchObject({ ownerPlane: 'model', risk: 'medium', requiresApproval: false, reversible: true })
    expect(restore).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.save_artifact_revision')).toBe(false)
    expect(isModelExecutableAction('chat.restore_artifact_revision')).toBe(false)
    expect(MODEL_ACCESS['chat.save_artifact_revision']).toMatchObject({ access: 'none' })
    expect(MODEL_ACCESS['chat.restore_artifact_revision']).toMatchObject({ access: 'none' })

    const saveSchema = save?.inputSchema
    expect(saveSchema?.safeParse({ threadId: 't1', artifactId: 'report', content: 'text', expectedVersion: 2 }).success).toBe(true)
    for (const input of [
      { threadId: 't1', artifactId: 'report', content: '', expectedVersion: 2 },
      { threadId: 't1', artifactId: 'report', content: 'x'.repeat(200_001), expectedVersion: 2 },
      { threadId: 't1', artifactId: 'report', content: 'text', expectedVersion: 0 },
      { threadId: 't1', artifactId: 'report', content: 'text' },
      { threadId: '', artifactId: 'report', content: 'text', expectedVersion: 2 },
      // The owner is read from the session, never named by a client.
      { threadId: 't1', artifactId: 'report', content: 'text', expectedVersion: 2, userId: 'user_x' },
    ]) {
      expect(saveSchema?.safeParse(input).success).toBe(false)
    }

    const restoreSchema = restore?.inputSchema
    expect(restoreSchema?.safeParse({ threadId: 't1', artifactId: 'report', version: 1, expectedVersion: 3 }).success).toBe(true)
    for (const input of [
      { threadId: 't1', artifactId: 'report', version: 0, expectedVersion: 3 },
      { threadId: 't1', artifactId: 'report', version: 1 },
      { threadId: 't1', artifactId: 'report', version: 1, expectedVersion: 3, content: 'x' },
    ]) {
      expect(restoreSchema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers saving an artifact version to knowledge as an irreversible person-only Data write (EP-3-S8)', () => {
    const promote = getActionDescriptor('chat.save_artifact_to_knowledge')

    expect(promote).toMatchObject({ ownerPlane: 'data', risk: 'medium', requiresApproval: false, reversible: false })
    expect(isModelExecutableAction('chat.save_artifact_to_knowledge')).toBe(false)
    expect(MODEL_ACCESS['chat.save_artifact_to_knowledge']).toMatchObject({ access: 'none' })

    const schema = promote?.inputSchema
    const valid = { threadId: 't1', artifactId: 'report', version: 2, title: 'Rapport', content: 'tekst' }
    expect(schema?.safeParse(valid).success).toBe(true)
    for (const input of [
      { ...valid, version: 0 },
      { ...valid, version: 1.5 },
      { ...valid, threadId: '' },
      { ...valid, artifactId: 'x'.repeat(257) },
      { ...valid, content: '   ' },
      { ...valid, content: 'x'.repeat(200_001) },
      { threadId: 't1', artifactId: 'report', title: 'Rapport', content: 'tekst' },
      // The owner is read from the person's token, never named by a client.
      { ...valid, orgId: 'org_x' },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers the cited-passage viewer as a low-risk read the model cannot execute (EP-2-S4)', () => {
    const open = getActionDescriptor('chat.open_cited_passage')

    expect(open).toMatchObject({ ownerPlane: 'data', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.open_cited_passage')).toBe(false)
    expect(MODEL_ACCESS['chat.open_cited_passage']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = open?.inputSchema
    expect(schema?.safeParse({ citationId: 'dr-1' }).success).toBe(true)
    expect(schema?.safeParse({ citationId: 'doc-1', documentId: 'doc-1', chunkIndex: 2 }).success).toBe(true)
    for (const input of [
      { citationId: '' },
      { citationId: 'doc-1', chunkIndex: -1 },
      { citationId: 'doc-1', chunkIndex: 1.5 },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
    // Data Plane reads the viewer from the session; a client never names whose grants apply.
    expect(schema?.safeParse({ citationId: 'doc-1', orgId: 'org_x' }).success).toBe(false)
  })

  it('registers the artifact revision history as a low-risk read the model cannot execute (EP-3-S3)', () => {
    const history = getActionDescriptor('chat.list_artifact_revisions')

    expect(history).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.list_artifact_revisions')).toBe(false)
    expect(MODEL_ACCESS['chat.list_artifact_revisions']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = history?.inputSchema
    expect(schema?.safeParse({ threadId: 'thread-1', artifactId: 'report' }).success).toBe(true)
    expect(schema?.safeParse({ threadId: 'thread-1', artifactId: 'report', beforeVersion: 3, limit: 20 }).success).toBe(true)
    for (const input of [
      { threadId: '', artifactId: 'report' },
      { threadId: 'thread-1', artifactId: '' },
      { threadId: 'thread-1', artifactId: 'report', beforeVersion: 0 },
      { threadId: 'thread-1', artifactId: 'report', limit: 21 },
      { threadId: 'thread-1', artifactId: 'report', limit: 1.5 },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
    // The owner is read from the session, never named by a client.
    expect(schema?.safeParse({ threadId: 'thread-1', artifactId: 'report', userId: 'user_x' }).success).toBe(false)
  })

  it('registers the deck template picker as low-risk actions the model cannot execute (EP-5-S7)', () => {
    const list = getActionDescriptor('chat.list_deck_templates')
    const apply = getActionDescriptor('chat.apply_deck_template')

    expect(list).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(apply).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.list_deck_templates')).toBe(false)
    expect(isModelExecutableAction('chat.apply_deck_template')).toBe(false)
    expect(MODEL_ACCESS['chat.list_deck_templates']).toMatchObject({ access: 'none', sideEffectClass: 'read' })
    expect(MODEL_ACCESS['chat.apply_deck_template']).toMatchObject({ access: 'none' })

    // The org comes from the session: the list takes nothing a client could name.
    const listSchema = list?.inputSchema
    expect(listSchema?.safeParse({}).success).toBe(true)
    expect(listSchema?.safeParse({ orgId: 'org_x' }).success).toBe(false)

    const schema = apply?.inputSchema
    expect(schema?.safeParse({ skillId: 'skill-brand' }).success).toBe(true)
    expect(schema?.safeParse({ skillId: 'org:brand.v2_1', threadId: 'thread-1' }).success).toBe(true)
    for (const input of [
      {},
      { skillId: '' },
      { skillId: '../blob' },
      { skillId: 'a b' },
      { skillId: 'x'.repeat(129) },
      { skillId: 'skill-brand', name: 'Aquatiq brand' },
      { skillId: 'skill-brand', orgId: 'org_x' },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers the deck slide open as a low-risk read the model cannot execute (EP-5-S4)', () => {
    const open = getActionDescriptor('chat.open_deck_slide')

    expect(open).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.open_deck_slide')).toBe(false)
    expect(MODEL_ACCESS['chat.open_deck_slide']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = open?.inputSchema
    expect(schema?.safeParse({ artifactId: 'artifact-deck', slideId: 's3' }).success).toBe(true)
    for (const input of [
      { artifactId: '', slideId: 's3' },
      { artifactId: 'artifact-deck', slideId: '' },
      { artifactId: 'artifact-deck', slideId: 's 3' },
      { artifactId: 'artifact-deck', slideId: '../s3' },
      { artifactId: 'artifact-deck', slideId: 'x'.repeat(65) },
      // The deck is read from the panel's manifest: a client never names a thread, an owner or an index.
      { artifactId: 'artifact-deck', slideId: 's3', threadId: 'thread-1' },
      { artifactId: 'artifact-deck', slideId: 's3', index: 2 },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers the computer panel open as a low-risk read the model cannot execute (P1)', () => {
    const open = getActionDescriptor('chat.open_computer_viewer')

    expect(open).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.open_computer_viewer')).toBe(false)
    expect(MODEL_ACCESS['chat.open_computer_viewer']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = open?.inputSchema
    expect(schema?.safeParse({ threadId: 'thr_1', callId: 'call_1' }).success).toBe(true)
    for (const input of [
      { threadId: '', callId: 'call_1' },
      { threadId: 'thr_1', callId: '' },
      { threadId: 'thr_1', callId: 'x'.repeat(257) },
      // The viewer is picked from the call's own intent: a client never names a viewer, a kind or a path.
      { threadId: 'thr_1', callId: 'call_1', viewer: 'terminal' },
      { threadId: 'thr_1', callId: 'call_1', path: 'notes.txt' },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers the deck presentation as a low-risk read the model cannot execute (EP-5-S5)', () => {
    const present = getActionDescriptor('chat.present_deck')

    expect(present).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.present_deck')).toBe(false)
    expect(MODEL_ACCESS['chat.present_deck']).toMatchObject({ access: 'none', sideEffectClass: 'read' })

    const schema = present?.inputSchema
    expect(schema?.safeParse({ artifactId: 'artifact-deck' }).success).toBe(true)
    for (const input of [
      { artifactId: '' },
      // The slides, the pages and the gate come from the panel's manifest: a client never names a slide, an index or a thread.
      { artifactId: 'artifact-deck', slideId: 's3' },
      { artifactId: 'artifact-deck', index: 2 },
      { artifactId: 'artifact-deck', threadId: 'thread-1' },
    ]) {
      expect(schema?.safeParse(input).success).toBe(false)
    }
  })

  it('registers the widget bridge as low-risk actions the model cannot execute (EP-4-S3)', () => {
    const stage = getActionDescriptor('chat.stage_widget_followup')
    expect(stage).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.stage_widget_followup')).toBe(false)
    expect(stage?.description ?? '').toContain('Never model-executable')
    expect(stage?.inputSchema.safeParse({ text: 'Vis for 25 år' }).success).toBe(true)
    expect(stage?.inputSchema.safeParse({ text: 'x'.repeat(500) }).success).toBe(true)
    for (const input of [
      { text: '' },
      { text: 'x'.repeat(501) },
      // The widget, the answer and the thread are the host's: a caller names only the text.
      { text: 'Neste', threadId: 'thread-1' },
      { text: 'Neste', send: true },
    ]) {
      expect(stage?.inputSchema.safeParse(input).success).toBe(false)
    }

    const link = getActionDescriptor('chat.open_widget_link')
    expect(link).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.open_widget_link')).toBe(false)
    expect(link?.description ?? '').toContain('Never model-executable')
    for (const url of ['https://example.com/rente', 'http://example.com', 'mailto:ola@example.com']) {
      expect(link?.inputSchema.safeParse({ url }).success).toBe(true)
    }
    for (const url of ['javascript:alert(1)', 'data:text/html,x', '/api/v1/me', '//evil.example', 'https://a.example/\nx', `https://a.example/${'x'.repeat(8 * 1024)}`]) {
      expect(link?.inputSchema.safeParse({ url }).success).toBe(false)
    }
  })

  it('registers the MCP App host as low-risk actions the model cannot execute (EP-4-S9)', () => {
    const open = getActionDescriptor('chat.open_mcp_app')
    expect(open).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.open_mcp_app')).toBe(false)
    expect(open?.description ?? '').toContain('Never model-executable')
    expect(open?.inputSchema.safeParse({ serverId: 'srv-1', tool: 'show_chart' }).success).toBe(true)
    for (const input of [
      { serverId: '', tool: 'show_chart' },
      { serverId: 'srv-1', tool: '' },
      // The org and viewer are the session's: a caller names only the server and the tool.
      { serverId: 'srv-1', tool: 'show_chart', orgId: 'org-2' },
      { serverId: 'srv-1', tool: 'show_chart', uri: 'ui://other/app' },
    ]) {
      expect(open?.inputSchema.safeParse(input).success).toBe(false)
    }

    const stage = getActionDescriptor('chat.stage_mcp_app_message')
    expect(stage).toMatchObject({ ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true })
    expect(isModelExecutableAction('chat.stage_mcp_app_message')).toBe(false)
    expect(stage?.description ?? '').toContain('Never model-executable')
    expect(stage?.inputSchema.safeParse({ text: 'Vis som tabell' }).success).toBe(true)
    for (const input of [{ text: '' }, { text: 'x'.repeat(501) }, { text: 'Neste', send: true }]) {
      expect(stage?.inputSchema.safeParse(input).success).toBe(false)
    }
  })

  it('registers the app-initiated MCP tool call as a medium-risk action the model cannot execute (EP-4-S10)', () => {
    const call = getActionDescriptor('chat.mcp_app_tool_call')
    expect(call).toMatchObject({ ownerPlane: 'model', risk: 'medium', requiresApproval: false, reversible: false })
    expect(isModelExecutableAction('chat.mcp_app_tool_call')).toBe(false)
    expect(call?.description ?? '').toContain('Never model-executable')
    expect(call?.description ?? '').toContain('app_call_requires_chat')
    const valid = { runId: 'run-1', serverId: 'srv-1', tool: 'echo_read', arguments: { q: 'x' }, frameNonce: 'frame-0123456789abcdef' }
    expect(call?.inputSchema.safeParse(valid).success).toBe(true)
    for (const input of [
      { ...valid, runId: '' },
      { ...valid, tool: '' },
      // A server id with `__` would name another server's tool.
      { ...valid, serverId: 'srv__other' },
      { ...valid, arguments: [1] },
      { ...valid, frameNonce: 'short' },
      // The org, the viewer and the permission posture are never the caller's to name.
      { ...valid, orgId: 'org-2' },
      { ...valid, permissionMode: 'auto' },
    ]) {
      expect(call?.inputSchema.safeParse(input).success).toBe(false)
    }
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
    for (const id of ['org.set_agent_policy', 'org.set_web_policy', 'org.create_hook', 'org.update_hook', 'org.delete_hook']) {
      expect(registered(id), id).toMatchObject({ risk: 'high' })
    }
    expect(registered('org.preview_effective_policy')).toMatchObject({ risk: 'low' })
  })

  it('registers the organization web access switch as a model-plane action the model cannot execute', () => {
    expect(registered('org.set_web_policy'), 'org.set_web_policy').toMatchObject({
      ownerPlane: 'model',
      requiresApproval: false,
      reversible: true,
    })
    expect(isModelExecutableAction('org.set_web_policy')).toBe(false)
    expect(registered('org.set_web_policy')?.description ?? '').toContain('model-executable')
  })

  it('takes the two web switches from the client, never cached_only, and no organization', () => {
    const setWeb = registered('org.set_web_policy')?.inputSchema

    expect(setWeb?.safeParse({ revision: 0, webMode: 'off', paidProviders: 'on' }).success).toBe(true)
    expect(setWeb?.safeParse({ revision: 4, webMode: 'live', paidProviders: 'off' }).success).toBe(true)
    // cached_only is reserved on the server until Quarry ships it, so the client does not offer it.
    expect(setWeb?.safeParse({ revision: 0, webMode: 'cached_only', paidProviders: 'on' }).success).toBe(false)
    // The revision is required, and the organization is the session's, never the body's.
    expect(setWeb?.safeParse({ webMode: 'off', paidProviders: 'on' }).success).toBe(false)
    expect(setWeb?.safeParse({ revision: 0, webMode: 'off', paidProviders: 'on', orgId: 'org_1' }).success).toBe(false)
  })

  it('registers the effective-policy read as a model-plane action the model cannot execute, taking only a thread', () => {
    expect(registered('policy.read_effective'), 'policy.read_effective').toMatchObject({
      ownerPlane: 'model',
      risk: 'low',
      requiresApproval: false,
      reversible: true,
    })
    expect(isModelExecutableAction('policy.read_effective')).toBe(false)
    const input = registered('policy.read_effective')?.inputSchema

    expect(input?.safeParse({ threadId: 'thread_1' }).success).toBe(true)
    // The organization is the session's: a body that names one, or no thread at all, is refused.
    expect(input?.safeParse({ threadId: 'thread_1', orgId: 'org_1' }).success).toBe(false)
    expect(input?.safeParse({}).success).toBe(false)
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

  it('registers spaces.update_sidebar as a low-risk, ungated, reversible application action the model cannot execute (WS16-S8)', () => {
    const action = getActionDescriptor('spaces.update_sidebar')
    expect(action).toBeDefined()
    expect(action?.ownerPlane).toBe('application')
    expect(action?.risk).toBe('low')
    expect(action?.requiresApproval).toBe(false)
    expect(action?.reversible).toBe(true)
    expect(isModelExecutableAction('spaces.update_sidebar')).toBe(false)
    expect(action?.inputSchema.safeParse({ baseRevision: 0, doc: { starred: [] } }).success).toBe(true)
    expect(action?.inputSchema.safeParse({ baseRevision: -1, doc: {} }).success).toBe(false)
    expect(action?.inputSchema.safeParse({ baseRevision: 1.5, doc: {} }).success).toBe(false)
    expect(action?.inputSchema.safeParse({ baseRevision: 0 }).success).toBe(false)
    expect(action?.inputSchema.safeParse({ baseRevision: 0, doc: [] }).success).toBe(false)
  })

  it('registers the direct message actions as low-risk application actions the model cannot execute (WS12-S3)', () => {
    for (const id of ['spaces.open_dm', 'spaces.hide_space'] as const) {
      const action = getActionDescriptor(id)
      expect(action, id).toBeDefined()
      expect(action?.ownerPlane).toBe('application')
      expect(action?.risk).toBe('low')
      expect(action?.requiresApproval).toBe(false)
      expect(isModelExecutableAction(id)).toBe(false)
    }
    const open = getActionDescriptor('spaces.open_dm')
    const ids = (n: number) => Array.from({ length: n }, (_, i) => `u-${i}`)
    expect(open?.reversible).toBe(false)
    expect(open?.inputSchema.safeParse({ participants: ids(1), clientRequestId: 'r' }).success).toBe(true)
    expect(open?.inputSchema.safeParse({ participants: ids(8), clientRequestId: 'r' }).success).toBe(true)
    expect(open?.inputSchema.safeParse({ participants: ids(9), clientRequestId: 'r' }).success).toBe(false)
    expect(open?.inputSchema.safeParse({ participants: [], clientRequestId: 'r' }).success).toBe(false)
    expect(open?.inputSchema.safeParse({ participants: ids(1) }).success).toBe(false)
    expect(open?.inputSchema.safeParse({ participants: [' '], clientRequestId: 'r' }).success).toBe(false)
    expect(getActionDescriptor('spaces.hide_space')?.reversible).toBe(true)
  })

  it('registers the WS10-S4 approval actions, in fragment order, and neither is model-executable', () => {
    const ids = actionRegistry.map((action) => action.id)
    const decide = ids.indexOf('spaces.decide_approval')
    expect(decide).toBeGreaterThan(-1)
    // Contiguous and in the order ws10.ts declares them.
    expect(ids.slice(decide, decide + 2)).toEqual(['spaces.decide_approval', 'spaces.revoke_approval_grant'])
    expect(decide).toBeGreaterThan(ids.indexOf('spaces.leave_room'))

    const decideAction = getActionDescriptor('spaces.decide_approval')
    expect(decideAction?.ownerPlane).toBe('model')
    expect(decideAction?.risk).toBe('high')
    const revokeAction = getActionDescriptor('spaces.revoke_approval_grant')
    expect(revokeAction?.ownerPlane).toBe('control')
    expect(revokeAction?.risk).toBe('medium')

    // An agent must never decide its own approval nor revoke the rule that limits it.
    expect(isModelExecutableAction('spaces.decide_approval')).toBe(false)
    expect(isModelExecutableAction('spaces.revoke_approval_grant')).toBe(false)

    const input = decideAction?.inputSchema
    const valid = { spaceRef: 'space-1', approvalId: 'apr-1', decision: 'approve' }
    expect(input?.safeParse(valid).success).toBe(true)
    expect(input?.safeParse({ ...valid, scope: 'always', idempotencyKey: 'key-aaaaaaaa' }).success).toBe(true)
    expect(input?.safeParse({ ...valid, scope: 'forever' }).success).toBe(false)
    expect(input?.safeParse({ ...valid, decision: 'maybe' }).success).toBe(false)
    expect(input?.safeParse({ ...valid, idempotencyKey: 'short' }).success).toBe(false)
    expect(input?.safeParse({ ...valid, reason: 'r'.repeat(501) }).success).toBe(false)
    expect(input?.safeParse({ approvalId: 'apr-1', decision: 'approve' }).success).toBe(false)
    expect(revokeAction?.inputSchema.safeParse({ spaceRef: 'space-1', grantId: 'grant-1' }).success).toBe(true)
    expect(revokeAction?.inputSchema.safeParse({ spaceRef: 'space-1' }).success).toBe(false)
    expect(revokeAction?.inputSchema.safeParse({ spaceRef: 'space-1', grantId: 'g'.repeat(129) }).success).toBe(false)
  })

  it('registers the WS14-S7 room file actions and none of them is model-executable', () => {
    const roomFileIds = ['space.files.attach', 'space.files.list', 'space.files.read_text', 'space.files.delete']
    const ids: string[] = actionRegistry.map((action) => action.id)
    const first = ids.indexOf(roomFileIds[0] as string)
    expect(first).toBeGreaterThan(-1)
    expect(ids.slice(first, first + roomFileIds.length)).toEqual(roomFileIds)
    for (const id of roomFileIds) {
      expect(isModelExecutableAction(id as Parameters<typeof isModelExecutableAction>[0]), id).toBe(false)
    }
    expect(getActionDescriptor('space.files.attach')?.ownerPlane).toBe('ingestion')
    expect(getActionDescriptor('space.files.list')?.ownerPlane).toBe('data')
    expect(getActionDescriptor('space.files.delete')?.risk).toBe('medium')
    expect(
      getActionDescriptor('space.files.list')?.inputSchema.safeParse({ spaceRef: 'space-1' }).success,
    ).toBe(true)
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

  it('registers Run now as a high-risk model-plane action the model can never press (EP-6-S2)', () => {
    const action = getActionDescriptor('schedule.run_now')
    expect(action).toMatchObject({ ownerPlane: 'model', risk: 'high', requiresApproval: false, reversible: false })
    expect(isModelExecutableAction('schedule.run_now')).toBe(false)
    // A model that could fire its own schedule on demand would spend the creator's stored authority.
    expect(MODEL_ACCESS['schedule.run_now']).toMatchObject({ access: 'none', exemptReason: 'agent_cannot_self_govern' })

    // The schedule and a client request id, nothing else: the creator and the org come from the session.
    const requestId = '6f1c2a9e-0b7d-4c55-9a1e-3d2f8b7c6a10'
    expect(action?.inputSchema.safeParse({ scheduleId: 'cron_1', requestId }).success).toBe(true)
    expect(action?.inputSchema.safeParse({ scheduleId: 'cron_1' }).success).toBe(false)
    for (const bad of ['short', 'has/slash-0123456789', 'x'.repeat(65)]) {
      expect(action?.inputSchema.safeParse({ scheduleId: 'cron_1', requestId: bad }).success, bad).toBe(false)
    }
    for (const extra of ['creator_subject_id', 'orgId', 'task_template']) {
      expect(action?.inputSchema.safeParse({ scheduleId: 'cron_1', requestId, [extra]: 'x' }).success, extra).toBe(false)
    }
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

  it('registers templates and presets as person-only writes whose preset settings are the closed schema (EP-11-S6)', () => {
    for (const id of ['templates.create', 'templates.update', 'presets.create', 'presets.update'] as const) {
      const descriptor = getActionDescriptor(id)
      expect(descriptor?.ownerPlane).toBe('model')
      expect(descriptor?.risk).toBe('low')
      expect(MODEL_ACCESS[id].eligible).toBe(false)
    }
    const create = getActionDescriptor('presets.create')
    const preset = { name: 'Research', content: 'Research {{input}}', settings: { modeId: 'research', browseWeb: true, planMode: true } }
    expect(create?.inputSchema.safeParse(preset).success).toBe(true)
    expect(create?.inputSchema.safeParse({ ...preset, settings: { planMode: false } }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ ...preset, settings: { persona: 'pirate' } }).success).toBe(false)
    expect(create?.inputSchema.safeParse({ ...preset, settings: { skillIds: ['a', 'b', 'c', 'd', 'e'] } }).success).toBe(false)
    const template = getActionDescriptor('templates.create')
    expect(template?.inputSchema.safeParse({ name: 'Ukesrapport', content: 'Rapport for {{input}}' }).success).toBe(true)
    expect(template?.inputSchema.safeParse({ name: '', content: 'x' }).success).toBe(false)
  })

  it('registers the skill draft lifecycle as person-only writes the model cannot execute (EP-11-S5)', () => {
    for (const id of ['skills.draft.save', 'skills.publish', 'skills.draft.discard', 'skills.version.restore'] as const) {
      const action = getActionDescriptor(id)
      expect(action?.ownerPlane).toBe('model')
      expect(action?.requiresApproval).toBe(false)
      expect(MODEL_ACCESS[id].access).toBe('none')
    }
    const restore = getActionDescriptor('skills.version.restore')
    expect(restore?.inputSchema.safeParse({ skillId: 'sk-1', version: 2 }).success).toBe(true)
    expect(restore?.inputSchema.safeParse({ skillId: 'sk-1', version: 0 }).success).toBe(false)
    expect(restore?.inputSchema.safeParse({ skillId: 'sk-1', version: 1.5 }).success).toBe(false)
    const save = getActionDescriptor('skills.draft.save')
    expect(save?.inputSchema.safeParse({ skillId: 'sk-1', content: 'Nye steg' }).success).toBe(true)
    expect(save?.inputSchema.safeParse({ skillId: 'sk-1', content: 'x'.repeat(32_001) }).success).toBe(false)
    expect(getActionDescriptor('skills.publish')?.outputSchema.safeParse({ id: 'sk-1', version: 3, published: true }).success).toBe(true)
  })

  it('registers skill import as a person-only write bounded by the bundle cap (EP-11-S8)', () => {
    const importAction = getActionDescriptor('skills.import')
    expect(importAction?.ownerPlane).toBe('model')
    expect(importAction?.requiresApproval).toBe(false)
    expect(MODEL_ACCESS['skills.import'].access).toBe('none')
    expect(importAction?.inputSchema.safeParse({ fileName: 'SKILL.md', sizeBytes: 120 }).success).toBe(true)
    expect(importAction?.inputSchema.safeParse({ fileName: 'x.zip', sizeBytes: 9 * 1024 * 1024 }).success).toBe(false)
    expect(importAction?.inputSchema.safeParse({ fileName: 'x.zip', sizeBytes: 10, orgId: 'org-2' }).success).toBe(false)
    expect(importAction?.outputSchema.safeParse({ id: 'skill_1', draft: true, enabled: false }).success).toBe(true)
  })

  it('registers save-as-skill as a low-risk person-only write the model cannot execute, bounded like the server (EP-11-S1)', () => {
    const save = getActionDescriptor('skills.save_from_message')
    expect(save?.ownerPlane).toBe('model')
    expect(save?.risk).toBe('low')
    expect(save?.requiresApproval).toBe(false)
    expect(MODEL_ACCESS['skills.save_from_message'].access).toBe('none')
    expect(MODEL_ACCESS['skills.save_from_message'].eligible).toBe(false)

    const valid = {
      threadId: 'thread-1',
      messageId: 'msg-1',
      name: 'Utrulling',
      description: 'Kort',
      content: '1. Kjør testene',
      enabled: true,
    }
    expect(save?.inputSchema.safeParse(valid).success).toBe(true)
    expect(save?.inputSchema.safeParse({ ...valid, name: ' Utrulling ' }).success).toBe(true)
    expect(save?.inputSchema.safeParse({ ...valid, name: 'x'.repeat(65) }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ ...valid, content: '   ' }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ ...valid, content: 'x'.repeat(32_001) }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ ...valid, description: 'x'.repeat(301) }).success).toBe(false)
    expect(save?.inputSchema.safeParse({ ...valid, messageId: '' }).success).toBe(false)
    // The owner and the organisation come from the session, never from the body.
    const parsed = save?.inputSchema.parse({ ...valid, orgId: 'org-forged', owner_user_id: 'user-2' }) as Record<string, unknown>
    expect(parsed).not.toHaveProperty('orgId')
    expect(parsed).not.toHaveProperty('owner_user_id')
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
    // 272 after W3-E3.2-g1 registered browser.get_org_policy and browser.set_org_policy (Settings > Browser).
    // 320 after main EP-2/EP-3/EP-4/EP-6/EP-7/EP-9/EP-10/EP-11/EP-12/EP-14 (chat/skills/templates/policy/widget/thread actions).
    // 270 after EP-14-S7 registered chat.inspect_run_request, the request inspector read.
    // 271 after EP-6-S2 registered schedule.run_now.
    // 273 after EP-10-S7 registered integrations.list_connector_policies and integrations.set_connector_policy.
    // 274 after main's W3-E3.4-7 registered browser_download.send_to_knowledge (downloads drawer).
    // 275 after EP-2-S4 registered chat.open_cited_passage, the cited-passage viewer read.
    // +1 after EP-2-S8 registered chat.wide_research_start, the composer's wide-research entry.
    // +1 after EP-10-S1 registered approvals.preview, the owner's write-approval preview.
    // +1 after EP-3-S3 registered chat.list_artifact_revisions, the Output panel's stored history read.
    // +2 after EP-3-S4 registered chat.save_artifact_revision and chat.restore_artifact_revision.
    // +2 after EP-3-S5 registered chat.set_context_refs and chat.pick_context_ref, the composer's context chips.
    // +2 after EP-2-S5 registered chat.research_plan_start and chat.research_plan_cancel, the deep-research plan card.
    // +1 after EP-2-S9 registered chat.wide_research_retry_item, the wide-research grid's per-item retry.
    // +1 after EP-3-S8 registered chat.save_artifact_to_knowledge, the Output panel's copy to knowledge.
    // +2 after EP-5-S7 registered chat.list_deck_templates and chat.apply_deck_template, the template picker.
    // +4 after EP-7-S1 registered chat.set_goal, chat.update_goal, chat.clear_goal and chat.continue_goal.
    // +1 after EP-3-S6 registered chat.edit_artifact_selection, the Output panel's ask about a selection.
    // +1 after EP-5-S4 registered chat.open_deck_slide, the slide strip's open-a-slide action.
    // +1 after EP-7-S4 registered chat.goal_auto_toggle, the goal bar's Keep going switch.
    // +1 after EP-5-S5 registered chat.present_deck, the deck's present mode.
    // +1 after EP-9-W2 registered org.set_web_policy, the organization's web access switch.
    // +3 after EP-9-C1 registered org.set_retention, org.place_legal_hold and org.release_legal_hold.
    // +1 after EP-11-S1 registered skills.save_from_message, the Save-as-skill message action.
    // +1 after EP-9-T2 registered policy.read_effective, the thread's effective-policy read.
    // +2 after EP-4-S3 registered chat.stage_widget_followup and chat.open_widget_link, the widget bridge.
    // +1 after EP-11-S2 registered skills.versions.list, the Skills page's history read.
    // +1 after EP-11-S7 registered learning.learn_from_thread, the /learn command.
    // +4 after EP-11-S6 registered templates.create, templates.update, presets.create and presets.update.
    // +4 after EP-11-S5 registered skills.draft.save, skills.publish, skills.draft.discard and skills.version.restore.
    // +2 after EP-4-S9 registered chat.open_mcp_app and chat.stage_mcp_app_message, the MCP App host.
    // +1 after EP-4-S10 registered chat.mcp_app_tool_call, an app's read tool call on its own server.
    // +1 after EP-12-S11 registered thread.export, the owner's download of one conversation.
    // +1 after EP-11-S8 registered skills.import, the Skills page's import from a file.
    // +1 after EP-10-S8 registered chat.accept_connector_suggestion, the connected-app suggestion card.
    // Union (W-sweep): HEAD (W3-E3.2-g1 browser.get/set_org_policy: +2) and
    // model-plane (P1 chat.open_computer_viewer: +1): 320 + 3 = 323.
    // +1 after EP-6-S9 registered spaces.agent_binding.pin, the Space agent version pin.
    // +1 after EP-6-S11 registered agents.admitted_actions.list (action-registry.ts, the same tree): 323 + 2 = 325.
    // +1 after EP-6-S8 registered agents.preview, the builder's draft preview.
    // +1 after W3-EP-7-S6-c registered chat.goal_away_start, the keep-working-while-away start.
    // +1 after W4-CC-2 registered mcp.probe_server, the MCP add flow's probe of a server before it is saved.
    // +3 after W5-077-c registered chat.thread.archive, chat.thread.star and chat.thread.mark_unread, the palette's thread flags.
    // +5 after W4-PL-4 registered plugins.list_catalog, plugins.install, plugins.uninstall, plugins.approve_catalog and plugins.kill_digest.
    // +3 after W5-074-c registered org.list_permission_rules, org.create_permission_rule and org.delete_permission_rule, the organization's permission rules.
    expect(ids).toHaveLength(339)

    // The fragments also carry ids outside the `spaces.` prefix (`space.files.*`), so the Space
    // entries are the ones the fragments declare.
    const fragmentIds = new Set<string>(spaceActions.map((action) => action.id))
    const spaceIds = ids.filter((id) => fragmentIds.has(id))
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
    // The later workstream fragments follow them in fragment order (ws01, ws03, ws04, ...).
    expect(spaceIds.slice(14, 21)).toEqual([
      'spaces.create_post',
      'spaces.reply_in_thread',
      'spaces.set_effect_entitlements',
      'spaces.apply_effect_preset',
      'spaces.update_room_details',
      'spaces.change_member_role',
      'spaces.leave_room',
    ])
    expect(spaceIds.indexOf('spaces.set_agent_wake_policy')).toBeGreaterThan(spaceIds.indexOf('spaces.leave_room'))
    // Every fragment entry is spread once, in the order domains/spaces/index.ts lists them.
    expect(spaceIds).toEqual(spaceActions.map((action) => action.id))
  })
})

describe('settings.update_preferences contract v2 (WS16-S2)', () => {
  const schema = () => {
    const action = getActionDescriptor('settings.update_preferences')
    if (!action) throw new Error('settings.update_preferences is not registered')
    return action.inputSchema
  }

  it('accepts dateFormat, timeFormat (including auto) and compactMode', () => {
    for (const dateFormat of ['MM/DD/YYYY', 'DD/MM/YYYY', 'YYYY-MM-DD', 'auto']) {
      expect(schema().safeParse({ dateFormat }).success).toBe(true)
    }
    for (const timeFormat of ['12h', '24h', 'auto']) {
      expect(schema().safeParse({ timeFormat }).success).toBe(true)
    }
    expect(schema().safeParse({ compactMode: true }).success).toBe(true)
    expect(schema().safeParse({ compactMode: false, dateFormat: 'auto', timeFormat: '24h', timezone: 'Europe/Oslo' }).success).toBe(true)
  })

  it('rejects unknown enum values and a non-boolean compactMode', () => {
    expect(schema().safeParse({ dateFormat: 'YYYY/MM/DD' }).success).toBe(false)
    expect(schema().safeParse({ dateFormat: '' }).success).toBe(false)
    expect(schema().safeParse({ timeFormat: '13h' }).success).toBe(false)
    expect(schema().safeParse({ timeFormat: '24H' }).success).toBe(false)
    expect(schema().safeParse({ compactMode: 'yes' }).success).toBe(false)
  })
})
