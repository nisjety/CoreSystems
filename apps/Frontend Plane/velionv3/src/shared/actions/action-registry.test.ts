import { describe, expect, it } from 'vitest'
import { actionRegistry, getActionDescriptor } from '@/shared/actions/action-registry'
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

  it('includes actions in compact model context packs', () => {
    const pack = buildModelContextPack({
      route: '/knowledge',
      visibleItems: [{ type: 'source', id: 'src_website', label: 'Website crawl', status: 'stale' }],
    })

    expect(pack.currentView).toBe('knowledge')
    expect(pack.availableActions).toContain('knowledge.recrawl_source')
    expect(pack.redactionPolicy).toBe('ids-and-summaries-only')
  })

  it('keeps external URL security actions approval-gated', () => {
    const reputation = getActionDescriptor('security.check_url_reputation')
    const investigation = getActionDescriptor('security.investigate_url')

    expect(reputation).toBeDefined()
    expect(investigation).toBeDefined()
    if (!reputation || !investigation) throw new Error('Expected security actions to be registered')

    expect(reputation.requiresApproval).toBe(true)
    expect(reputation.reversible).toBe(false)
    expect(investigation.requiresApproval).toBe(true)
    expect(investigation.reversible).toBe(false)

    expect(reputation.inputSchema.safeParse({
      url: 'https://example.com/docs',
      allowExternalLookup: true,
    }).success).toBe(true)

    expect(investigation.inputSchema.safeParse({
      url: 'https://example.com/suspicious',
      allowExternalSubmission: true,
      dataClass: 'customer',
      visibility: 'unlisted',
    }).success).toBe(false)
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
})
