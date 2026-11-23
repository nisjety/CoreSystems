import { existsSync, readdirSync, readFileSync } from 'node:fs'
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
const TEXT_DOTS = ['.verevon-space-room-composer__typing-dots span', '.verevon-room-turn__working-dots']

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
    // Class tokens are read from string literals only, so a class named in a comment does not count as rendered.
    const rendered = classTokens(filesUnder(join(srcRoot, 'features'), (name) => name.endsWith('.tsx') && !name.endsWith('.test.tsx')))
    for (const dot of [...DOTS, ...TEXT_DOTS]) {
      const root = dot.slice(1).split(' ')[0] ?? ''
      expect(rendered.has(root), `${dot} is rendered`).toBe(true)
    }
  })

  it('keeps the typing and working dots visible in the text colour, all three working dots included', () => {
    for (const dot of TEXT_DOTS) {
      const rule = forced.find((candidate) => candidate.selectors.includes(`html:root ${dot}`))
      expect(rule, dot).toBeDefined()
      const declared = (prop: string): string | undefined => rule?.declarations.find((d) => d.prop === prop)?.value
      expect(declared('forced-color-adjust'), dot).toBe('none')
      expect(declared('background'), dot).toBe('CanvasText')
    }
    // The second and third working dots are box-shadows of the first, so they need a system colour too.
    const working = forced.find((rule) => rule.selectors.includes('html:root .verevon-room-turn__working-dots'))
    const shadow = working?.declarations.find((d) => d.prop === 'box-shadow')?.value ?? ''
    expect(shadow.match(/CanvasText/g)).toHaveLength(2)
    expect(topLevelValue(globalRules, '.verevon-room-turn__working-dots', 'box-shadow')?.split(',')).toHaveLength(2)
  })

  it('keeps the critical activity row outlined and the muted rows at full opacity', () => {
    const critical = forced.find((rule) => rule.selectors.includes('html:root .verevon-activity-row.verevon-activity-row--critical'))
    expect(critical?.declarations).toContainEqual({ prop: 'border', value: '2px solid CanvasText' })
    const muted = forced.find((rule) => rule.selectors.includes('html:root .verevon-activity-row.verevon-activity-row--muted'))
    expect(muted?.declarations).toContainEqual({ prop: 'opacity', value: '1' })
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

  // WCAG 2 contrast. Text needs 4.5:1 (1.4.3) and borders 3:1 (1.4.11), on white for light and on the dark surface
  // #171717 for dark. Translucent colours are composited over that surface first.
  type Rgb = [number, number, number]
  const parseColour = (value: string, surface: Rgb): Rgb => {
    const hex = /^#([0-9a-f]{6})$/i.exec(value)
    if (hex) return [0, 2, 4].map((at) => Number.parseInt((hex[1] ?? '').slice(at, at + 2), 16)) as Rgb
    const rgb = /^rgb\(\s*(\d+)\s+(\d+)\s+(\d+)\s*(?:\/\s*(\d+(?:\.\d+)?)%)?\s*\)$/i.exec(value)
    if (!rgb) throw new Error(`unparsed colour ${value}`)
    const alpha = rgb[4] === undefined ? 1 : Number(rgb[4]) / 100
    return [1, 2, 3].map((at, index) => Number(rgb[at]) * alpha + surface[index]! * (1 - alpha)) as Rgb
  }
  const luminance = ([r, g, b]: Rgb): number => {
    const channel = (c: number): number => (c / 255 <= 0.03928 ? c / 255 / 12.92 : ((c / 255 + 0.055) / 1.055) ** 2.4)
    return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
  }
  const contrast = (value: string, surface: Rgb): number => {
    const [a, b] = [luminance(parseColour(value, surface)), luminance(surface)].sort((x, y) => y - x)
    return (a! + 0.05) / (b! + 0.05)
  }
  const tokenValue = (rule: CssRule | undefined, token: string): string =>
    rule?.declarations.find((d) => d.prop === token)?.value ?? ''
  const WHITE: Rgb = [255, 255, 255]
  const DARK_SURFACE: Rgb = [0x17, 0x17, 0x17]
  const TEXT = ['--verevon-text-muted', '--verevon-e-muted', '--verevon-text-subtle']

  it('moves the muted text, border and sidebar tokens to high-contrast values on :root', () => {
    const light = more.find((rule) => rule.selectors.length === 1 && rule.selectors[0] === 'html:root')
    expect(light?.declarations.map((d) => d.prop)).toEqual(expect.arrayContaining(LIGHT_TOKENS))
    for (const token of TEXT) expect(contrast(tokenValue(light, token), WHITE), token).toBeGreaterThanOrEqual(4.5)
    for (const token of ['--verevon-border', '--verevon-border-soft', '--verevon-border-strong']) {
      expect(contrast(tokenValue(light, token), WHITE), token).toBeGreaterThanOrEqual(3)
    }
  })

  it('repeats them under html:root.dark, which outranks html:root', () => {
    const dark = more.find((rule) => rule.selectors.includes('html:root.dark'))
    expect(dark?.declarations.map((d) => d.prop)).toEqual(expect.arrayContaining(DARK_TOKENS))
    for (const token of TEXT) expect(contrast(tokenValue(dark, token), DARK_SURFACE), token).toBeGreaterThanOrEqual(4.5)
    for (const token of ['--verevon-border', '--verevon-border-soft', '--verevon-d-border']) {
      expect(contrast(tokenValue(dark, token), DARK_SURFACE), token).toBeGreaterThanOrEqual(3)
    }
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

// ---- what Space renders ----------------------------------------------------------------------------------------
// Space markup is the feature folder, the sidebar's Space components, and every component file those import
// directly (one hop: the model picker, the chat message list, ...). A class token is any hyphenated word inside a
// string literal of those files, so `class="onboarding-phase-spinner"` and classList keys both count.

const isSource = (name: string): boolean => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name)

function spaceSourceFiles(): string[] {
  return [
    ...filesUnder(join(srcRoot, 'features', 'spaces'), isSource),
    ...filesUnder(join(srcRoot, 'features', 'core', 'components', 'sidebar'), (name) =>
      isSource(name) && /^(?:Space|CoreSidebarSpacesPanel)/.test(name),
    ),
  ]
}

function resolveImport(fromFile: string, specifier: string): string | undefined {
  const base = specifier.startsWith('@/') ? join(srcRoot, specifier.slice(2)) : specifier.startsWith('.') ? join(dirname(fromFile), specifier) : undefined
  if (!base) return undefined
  return [`${base}.tsx`, join(base, 'index.tsx')].find((candidate) => existsSync(candidate))
}

/** The Space source files plus the component files they import directly. */
function spaceRenderedFiles(): string[] {
  const own = spaceSourceFiles()
  const files = new Set(own)
  for (const file of own) {
    for (const match of readFileSync(file, 'utf8').matchAll(/\bfrom\s+['"]([^'"]+)['"]/g)) {
      const resolved = resolveImport(file, match[1] ?? '')
      if (resolved) files.add(resolved)
    }
  }
  return [...files]
}

function classTokens(files: string[]): Set<string> {
  const tokens = new Set<string>()
  for (const file of files) {
    for (const literal of readFileSync(file, 'utf8').matchAll(/(['"`])((?:(?!\1)[^\\\n]|\\.)*)\1/g)) {
      for (const word of (literal[2] ?? '').split(/\s+/)) {
        if (/^[a-z][a-z0-9]*(?:[-_]+[a-z0-9]+)+$/.test(word)) tokens.add(word)
      }
    }
  }
  return tokens
}

/** The hyphenated classes a selector names. Bare state words such as `.dark` or `.active` are left out. */
const selectorClasses = (selectorText: string): string[] =>
  [...selectorText.matchAll(/\.([a-zA-Z_][\w-]*)/g)].map((match) => match[1] ?? '').filter((name) => /[-_]/.test(name))

/** True when Space markup carries every hyphenated class the selector needs, so the rule can match in a Space view. */
const matchesSpaceMarkup = (selectorText: string, rendered: Set<string>): boolean => {
  const classes = selectorClasses(selectorText)
  return classes.length > 0 && classes.every((name) => rendered.has(name))
}

describe('reduced motion over Space CSS', () => {
  const SPACE_SELECTOR =
    /\.(?:verevon-space|verevon-room|verevon-activity|verevon-attention|core-sidebar-working-dot|core-sidebar-unread-dot)/
  const NO_PREFERENCE = /\(\s*prefers-reduced-motion\s*:\s*no-preference\s*\)/
  const isMotion = (declaration: Declaration): boolean =>
    declaration.prop.startsWith('animation') && declaration.value.replace(/!important/gi, '').trim() !== 'none'
  const rendered = classTokens(spaceRenderedFiles())

  it('reads the class tokens of the shared components Space mounts', () => {
    // The model picker menu, the streaming cursor on the Space answer, and the /spaces provisioning spinner are
    // classes from outside the feature folder. If these go missing, the scan below has gone blind to them.
    for (const token of ['verevon-popover', 'verevon-chat-streaming', 'onboarding-phase-spinner']) {
      expect(rendered.has(token), token).toBe(true)
    }
  })

  it('every animation on a class Space renders sits inside prefers-reduced-motion: no-preference', () => {
    const motion: string[] = []
    const unguarded: string[] = []
    for (const file of filesUnder(srcRoot, (name) => name.endsWith('.css'))) {
      // src/demo is the film-only /scene route; its sheets are scoped to a scene attribute the product never sets.
      if (toPosix(relative(srcRoot, file)).startsWith('demo/')) continue
      const dedicated = isSpaceSheet(file)
      for (const rule of readRules(file)) {
        if (rule.atRules.some((at) => at.startsWith('@keyframes'))) continue
        const spaceRule =
          dedicated ||
          SPACE_SELECTOR.test(rule.selectorText) ||
          matchesSpaceMarkup(rule.selectorText, rendered)
        if (!spaceRule) continue
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
      '.verevon-popover',
      '.verevon-thinking-dots span',
      '.verevon-chat-streaming::after',
      '.onboarding-phase-spinner',
    ]) {
      expect(motion.some((label) => label.includes(selector)), selector).toBe(true)
    }
    expect(unguarded).toEqual([])
  })

  it('ui-baseline.css carries no motion of its own', () => {
    expect(baseline.flatMap((rule) => rule.declarations).filter(isMotion)).toEqual([])
  })
})
