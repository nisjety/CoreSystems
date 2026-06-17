import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const css = readFileSync(`${process.cwd()}/src/styles/global.css`, 'utf8')

describe('onboarding shared UI CSS', () => {
  it('does not override shared buttons from onboarding-specific selectors', () => {
    expect(css).not.toMatch(/\.onboarding[^{,]*\.button/)
  })

  it('uses Velion primitives instead of old step-specific button-like classes', () => {
    expect(css).not.toMatch(
      /onboarding-(skip-large|skip-link|result-row|connector-row|size-chip|paywall__toggle)/,
    )
    expect(css).toContain('.velion-choice-chip')
    expect(css).toContain('.velion-selectable-row')
    expect(css).toContain('.velion-switch__control')
  })
})
