import { describe, expect, it } from 'vitest'
import { buildReplay, type ReplayPlan } from './replay-run'

const plan: ReplayPlan = {
  steps: [
    { kind: 'navigate', value: 'https://example.com/search?q={{q}}' },
    { kind: 'click', selector: '#submit' },
    { kind: 'type', selector: '#box', value: 'hello {{name}}' },
    { kind: 'read' },
  ],
}

describe('buildReplay', () => {
  it('substitutes variables into the recorded steps', () => {
    const result = buildReplay(plan, { q: 'vere', name: 'ida' })
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.steps[0]?.value).toBe('https://example.com/search?q=vere')
    expect(result.steps[2]?.value).toBe('hello ida')
  })

  it('fails closed on a missing variable', () => {
    const result = buildReplay(plan, { q: 'vere' })
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.stepIndex).toBe(2)
    expect(result.reason).toBe('missing_variable:name')
  })

  it('fails closed on an unsafe substituted URL and a malformed step', () => {
    expect(buildReplay({ steps: [{ kind: 'navigate', value: 'javascript:alert(1)' }] }, {}).ok).toBe(false)
    expect(buildReplay({ steps: [{ kind: 'click' }] }, {}).ok).toBe(false)
  })
})
