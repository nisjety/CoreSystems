import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const css = readFileSync(`${process.cwd()}/src/styles/global.css`, 'utf8')

describe('onboarding shared UI CSS', () => {
  it('does not override shared buttons from onboarding-specific selectors', () => {
    expect(css).not.toMatch(/\.onboarding[^{,]*\.button/)
  })

  it('uses Verevon primitives instead of old step-specific button-like classes', () => {
    expect(css).not.toMatch(
      /onboarding-(skip-large|skip-link|result-row|connector-row|size-chip|paywall__toggle)/,
    )
    expect(css).toContain('.verevon-choice-chip')
    expect(css).toContain('.verevon-selectable-row')
    expect(css).toContain('.verevon-switch__control')
  })
})
