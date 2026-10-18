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
})
