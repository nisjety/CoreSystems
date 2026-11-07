import { describe, expect, it } from 'vitest'
import { assistantNavigationActionSchema } from '@/shared/actions/assistant-navigation'
import { actionRegistry } from '@/shared/actions/action-registry'

describe('local assistant navigation action', () => {
  it('accepts only the fixed internal destinations and modes', () => {
    expect(assistantNavigationActionSchema.safeParse({ type: 'assistant.navigation.guide', href: '/knowledge', mode: 'navigate' }).success).toBe(true)
    for (const href of ['https://example.com', '//example.com', '/knowledge?redirect=other', '/knowledge/other', '/settings']) {
      expect(assistantNavigationActionSchema.safeParse({ type: 'assistant.navigation.guide', href, mode: 'navigate' }).success).toBe(false)
    }
    expect(assistantNavigationActionSchema.safeParse({ type: 'assistant.navigation.guide', href: '/chat', mode: 'execute' }).success).toBe(false)
    expect(assistantNavigationActionSchema.safeParse({ type: 'assistant.navigation.guide', href: '/chat', mode: 'point', source: 'model' }).success).toBe(false)
  })

  it('does not expose this local interaction as a model-executable action', () => {
    expect(actionRegistry.some((action) => String(action.id) === 'assistant.navigation.guide')).toBe(false)
  })
})
