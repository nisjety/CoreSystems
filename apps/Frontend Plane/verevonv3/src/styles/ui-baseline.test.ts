import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

/**
 * WS16-S5 guards for the Space UI baseline (ui-baseline.css) and for the motion rules in Space CSS.
 *
 * These are string-level checks over the stylesheets, in the pattern of global.test.js. They run no browser. The
 * emulated forced-colours and more-contrast screenshots are tests/e2e/space-ui-baseline.spec.ts and need a live stack.
 */

const here = dirname(fileURLToPath(import.meta.url))
const srcRoot = join(here, '..')
const uiBaselinePath = join(here, 'ui-baseline.css')
const globalPath = join(here, 'global.css')

// ---- reading the sheets ----------------------------------------------------------------------------------------
// Not a full CSS parser, and it does not resolve the cascade. It skips comments, reads strings and nested blocks,
// and reports each style rule with its selector list, its declarations and the at-rules that enclose it.

interface Declaration {
  prop: string
  value: string
}

interface CssRule {
  selectors: string[]
  /** The rule's selector text, prefixed by its parent rule's selector for nested CSS. */
  selectorText: string
  /** The prelude of every enclosing at-rule, outermost first. */
  atRules: string[]
  declarations: Declaration[]
}

type CssNode = { kind: 'stmt'; text: string } | { kind: 'block'; prelude: string; children: CssNode[] }

function parseCss(source: string): CssNode[] {
  const css = source.replace(/\/\*[\s\S]*?\*\//g, (comment) => comment.replace(/[^\n]/g, ' '))
  let at = 0
  const readBlock = (depth: number): CssNode[] => {
    const nodes: CssNode[] = []
    let buffer = ''
    const flush = (): void => {
      const text = buffer.trim()
      if (text) nodes.push({ kind: 'stmt', text })
      buffer = ''
    }
    while (at < css.length) {
      const ch = css.charAt(at)
      if (ch === '"' || ch === "'") {
        let end = at + 1
        while (end < css.length && css.charAt(end) !== ch) end += css.charAt(end) === '\\' ? 2 : 1
        buffer += css.slice(at, end + 1)
        at = end + 1
      } else if (ch === '{') {
        const prelude = buffer.trim()
        buffer = ''
        at += 1
        nodes.push({ kind: 'block', prelude, children: readBlock(depth + 1) })
      } else if (ch === ';') {
        flush()
        at += 1
      } else if (ch === '}') {
        flush()
        at += 1
        if (depth > 0) return nodes
      } else {
        buffer += ch
        at += 1
      }
    }
    flush()
    return nodes
  }
  return readBlock(0)
}

/** Splits a selector list at its top-level commas, so `:where(a, b)` stays whole. */
function splitSelectorList(list: string): string[] {
  const parts: string[] = []
  let depth = 0
  let quote = ''
  let current = ''
  for (const ch of list) {
    if (quote) {
      if (ch === quote) quote = ''
    } else if (ch === '"' || ch === "'") {
      quote = ch
    } else if (ch === '(' || ch === '[') {
      depth += 1
    } else if (ch === ')' || ch === ']') {
      depth -= 1
    } else if (ch === ',' && depth === 0) {
      parts.push(current.trim())
      current = ''
      continue
    }
    current += ch
  }
  if (current.trim()) parts.push(current.trim())
  return parts
}

function cssRules(nodes: CssNode[], atRules: string[] = [], parentSelector = ''): CssRule[] {
  const rules: CssRule[] = []
  for (const node of nodes) {
    if (node.kind !== 'block') continue
    if (node.prelude.startsWith('@')) {
      rules.push(...cssRules(node.children, [...atRules, node.prelude], parentSelector))
      continue
    }
    const selectorText = [parentSelector, node.prelude].filter(Boolean).join(' ')
    const declarations = node.children.flatMap((child): Declaration[] => {
      if (child.kind !== 'stmt') return []
      const colon = child.text.indexOf(':')
      if (colon < 0) return []
      return [{ prop: child.text.slice(0, colon).trim().toLowerCase(), value: child.text.slice(colon + 1).trim() }]
    })
    rules.push({ selectors: splitSelectorList(node.prelude), selectorText, atRules, declarations })
    rules.push(...cssRules(node.children, atRules, selectorText))
  }
  return rules
}

const readRules = (path: string): CssRule[] => cssRules(parseCss(readFileSync(path, 'utf8')))
const underAtRule = (rules: CssRule[], prelude: string): CssRule[] => rules.filter((rule) => rule.atRules.includes(prelude))
/** The last value a top-level rule (outside any at-rule) gives `prop` for exactly this selector. */
const topLevelValue = (rules: CssRule[], selector: string, prop: string): string | undefined =>
  rules
    .filter((rule) => rule.atRules.length === 0 && rule.selectors.includes(selector))
    .flatMap((rule) => rule.declarations)
    .filter((declaration) => declaration.prop === prop)
    .at(-1)?.value

function filesUnder(dir: string, keep: (name: string) => boolean): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return filesUnder(path, keep)
    return entry.isFile() && keep(entry.name) ? [path] : []
  })
}

const toPosix = (path: string): string => path.replace(/\\/g, '/')
/** Space's own sheets: the feature folder, and the sidebar's space-* sheets. */
const isSpaceSheet = (path: string): boolean => {
  const posix = toPosix(path)
  return posix.includes('/features/spaces/') || /\/features\/core\/components\/sidebar\/space-[^/]*\.css$/.test(posix)
}

const baseline = readRules(uiBaselinePath)
const globalRules = readRules(globalPath)

const COMPACT = 'html:root[data-density="compact"]'
const DOTS = ['.core-sidebar-working-dot', '.core-sidebar-unread-dot', '.verevon-space-presence__dot']

describe('ui-baseline.css selectors', () => {
  it('prefixes every rule with html:root, so the result does not depend on import order', () => {
    const selectors = baseline.flatMap((rule) => rule.selectors)
    expect(selectors.length).toBeGreaterThan(0)
    expect(selectors.filter((selector) => !/^html:root(?![\w-])/.test(selector))).toEqual([])
  })
})

describe('forced colours', () => {
  const forced = underAtRule(baseline, '@media (forced-colors: active)')

  it('keeps the status dots visible: a system background and a system border on each', () => {
    const dotRule = forced.find((rule) => DOTS.every((dot) => rule.selectors.includes(`html:root ${dot}`)))
    expect(dotRule, 'one forced-colors rule names all three dots').toBeDefined()
    const declared = (prop: string): string | undefined => dotRule?.declarations.find((d) => d.prop === prop)?.value
    expect(declared('forced-color-adjust')).toBe('none')
    expect(declared('background')).toBe('Highlight')
    expect(declared('border')).toBe('1px solid CanvasText')
  })

  it('names dots that a component renders, so the rule has something to restore', () => {
    const features = filesUnder(join(srcRoot, 'features'), (name) => name.endsWith('.tsx') && !name.endsWith('.test.tsx'))
    const markup = features.map((file) => readFileSync(file, 'utf8')).join('\n')
    for (const dot of DOTS) expect(markup.includes(dot.slice(1)), `${dot} is rendered`).toBe(true)
  })

  it('keeps the critical activity row outlined and the muted rows at full opacity', () => {
    const critical = forced.find((rule) => rule.selectors.includes('html:root .verevon-activity-row.verevon-activity-row--critical'))
    expect(critical?.declarations).toContainEqual({ prop: 'border', value: '2px solid CanvasText' })
    expect(forced.flatMap((rule) => rule.selectors)).toContain('html:root .verevon-activity-row.verevon-activity-row--muted')
  })

  it('draws focus rings in a system colour', () => {
    const focus = forced.find((rule) => rule.selectors.some((selector) => selector.endsWith(':focus-visible')))
    expect(focus?.declarations).toContainEqual({ prop: 'outline-color', value: 'Highlight' })
  })
})

describe('prefers-contrast: more', () => {
  const more = underAtRule(baseline, '@media (prefers-contrast: more)')
  const LIGHT_TOKENS = [
    '--verevon-text-muted',
    '--verevon-e-muted',
    '--verevon-text-subtle',
    '--verevon-border',
    '--verevon-border-soft',
    '--verevon-border-strong',
    '--verevon-sidebar-text',
    '--verevon-sidebar-muted',
  ]
  const DARK_TOKENS = [
    '--verevon-text-muted',
    '--verevon-e-muted',
    '--verevon-text-subtle',
    '--verevon-border',
    '--verevon-border-soft',
    '--verevon-d-border',
    '--verevon-sidebar-text',
    '--verevon-sidebar-muted',
  ]

  it('moves the muted text, border and sidebar tokens to high-contrast values on :root', () => {
    const light = more.find((rule) => rule.selectors.length === 1 && rule.selectors[0] === 'html:root')
    expect(light?.declarations.map((d) => d.prop)).toEqual(expect.arrayContaining(LIGHT_TOKENS))
  })

  it('repeats them under html:root.dark, which outranks html:root', () => {
    const dark = more.find((rule) => rule.selectors.includes('html:root.dark'))
    expect(dark?.declarations.map((d) => d.prop)).toEqual(expect.arrayContaining(DARK_TOKENS))
  })
})

describe('compact density', () => {
  it('is keyed on data-density on <html>, and nothing else', () => {
    const densitySelectors = baseline.flatMap((rule) => rule.selectors).filter((selector) => selector.includes('data-density'))
    expect(densitySelectors.length).toBeGreaterThan(0)
    expect(densitySelectors.filter((selector) => !selector.startsWith(`${COMPACT} `))).toEqual([])
  })

  it('is tighter than the comfortable value it overrides in global.css', () => {
    const px = (value: string | undefined): number => Number.parseFloat(value ?? '')
    expect(px(topLevelValue(globalRules, '.core-sidebar-panel-link', 'height'))).toBeGreaterThan(
      px(topLevelValue(baseline, `${COMPACT} .core-sidebar-panel-link`, 'height')),
    )
    expect(px(topLevelValue(globalRules, '.verevon-activity-row', 'padding'))).toBeGreaterThan(
      px(topLevelValue(baseline, `${COMPACT} .verevon-activity-row`, 'padding-top')),
    )
    expect(px(topLevelValue(globalRules, '.verevon-room-turn', 'gap'))).toBeGreaterThan(
      px(topLevelValue(baseline, `${COMPACT} .verevon-room-turn`, 'gap')),
    )
  })

  it('is set on <html> by the preferences store, and the provider loads this sheet', () => {
    const read = (relativePath: string): string => readFileSync(join(srcRoot, relativePath), 'utf8')
    expect(read('shared/preferences/preferences-store.ts')).toContain('document.documentElement.dataset.density = density')
    expect(read('shared/preferences/UserPreferencesProvider.tsx')).toContain("import '@/styles/ui-baseline.css'")
  })
})

describe('reduced motion over Space CSS', () => {
  const SPACE_SELECTOR =
    /\.(?:verevon-space|verevon-room|verevon-activity|verevon-attention|core-sidebar-working-dot|core-sidebar-unread-dot)/
  const NO_PREFERENCE = /\(\s*prefers-reduced-motion\s*:\s*no-preference\s*\)/
  const isMotion = (declaration: Declaration): boolean =>
    declaration.prop.startsWith('animation') && declaration.value.replace(/!important/gi, '').trim() !== 'none'

  it('every Space animation sits inside prefers-reduced-motion: no-preference', () => {
    const motion: string[] = []
    const unguarded: string[] = []
    for (const file of filesUnder(srcRoot, (name) => name.endsWith('.css'))) {
      const dedicated = isSpaceSheet(file)
      for (const rule of readRules(file)) {
        if (rule.atRules.some((at) => at.startsWith('@keyframes'))) continue
        if (!dedicated && !SPACE_SELECTOR.test(rule.selectorText)) continue
        if (!rule.declarations.some(isMotion)) continue
        const label = `${toPosix(relative(srcRoot, file))}: ${rule.selectorText.replace(/\s+/g, ' ')}`
        motion.push(label)
        if (!rule.atRules.some((at) => NO_PREFERENCE.test(at))) unguarded.push(label)
      }
    }
    // The scan has to see the Space animations it guards, or a parser slip would pass with nothing checked.
    for (const selector of [
      '.verevon-room-turn__working-dots',
      '.core-sidebar-working-dot',
      '.verevon-space-room-composer__typing-dots span',
      '.verevon-run-card__dots',
    ]) {
      expect(motion.some((label) => label.includes(selector)), selector).toBe(true)
    }
    expect(unguarded).toEqual([])
  })

  it('ui-baseline.css carries no motion of its own', () => {
    expect(baseline.flatMap((rule) => rule.declarations).filter(isMotion)).toEqual([])
  })
})
