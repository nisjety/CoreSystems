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

describe('chat turn containment CSS', () => {
  it('contains non-latest chat turns only when the switch attribute is set', () => {
    expect(css).toMatch(
      /\.verevon-chat-thread\[data-chat-containment="on"\] > \.verevon-chat-message:not\(\[data-latest\]\) \{\s*content-visibility: auto;\s*contain-intrinsic-size: auto 480px;\s*\}/,
    )
  })

  it('declares content-visibility only in rules gated by the switch attribute', () => {
    const declaring = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].filter(([, , body]) =>
      /content-visibility\s*:/.test(body),
    )
    expect(declaring.length).toBeGreaterThan(0)
    for (const [, selector] of declaring) {
      expect(selector).toContain('[data-chat-containment="on"]')
    }
  })
})
