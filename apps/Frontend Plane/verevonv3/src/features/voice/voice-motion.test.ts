/**
 * Voice mode under `prefers-reduced-motion: reduce`.
 *
 * The dialog panel carries the shared `verevon-panel-in` entrance (a slide up
 * and scale), and the listening mic pulses. Neither is listed in the global
 * reduced-motion guard, so the voice block itself has to switch them off.
 * Read as source text: jsdom neither evaluates media queries nor runs CSS
 * animations, so there is no rendered seam to observe this through.
 */
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const CSS = readFileSync(resolve(process.cwd(), 'src/styles/global.css'), 'utf8').replace(/\r\n/g, '\n')

/** The bodies of every `@media (prefers-reduced-motion: reduce)` block, braces balanced. */
function reducedMotionBlocks(css: string): string[] {
  const blocks: string[] = []
  const opener = '@media (prefers-reduced-motion: reduce)'
  let from = css.indexOf(opener)
  while (from !== -1) {
    const open = css.indexOf('{', from)
    let depth = 0
    let end = open
    for (; end < css.length; end += 1) {
      if (css[end] === '{') depth += 1
      else if (css[end] === '}' && --depth === 0) break
    }
    blocks.push(css.slice(open + 1, end))
    from = css.indexOf(opener, end)
  }
  return blocks
}

/** Whether some reduced-motion rule whose selector list names `selector` sets `animation: none`. */
function stopsAnimation(selector: string): boolean {
  return reducedMotionBlocks(CSS).some((block) =>
    [...block.matchAll(/([^{}]+)\{([^{}]*)\}/g)].some(([, selectors, body]) =>
      selectors!.split(',').some((candidate) => candidate.trim() === selector) && /animation:\s*none/.test(body!),
    ),
  )
}

describe('Voice mode honours reduced motion', () => {
  it('drops the dialog panel entrance', () => {
    expect(stopsAnimation('.realtime-voice-modal__panel.verevon-panel-in')).toBe(true)
  })

  it('stops the listening pulse', () => {
    expect(stopsAnimation('.verevon-voice-pulse')).toBe(true)
  })
})
