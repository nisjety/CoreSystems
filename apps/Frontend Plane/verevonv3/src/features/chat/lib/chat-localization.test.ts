/**
 * Accessible names switch language with everything else.
 *
 * UX spec acceptance criterion 9 says Norwegian and English labels switch
 * together and mixed-language chrome is not accepted. The chat feature passed
 * three live audits against that criterion and still failed it, because the
 * defect was invisible on screen: the VISIBLE chrome all went through
 * `i18n.tr`, while accessible names did not. `aria-label="Answer version"` sat
 * four lines from `title="Resultatet ble avkortet"` in the same component —
 * whichever locale you chose, a screen-reader user heard the other language.
 *
 * So the rule is enforced where the failure actually lives: any attribute that
 * produces a name a person reads or hears (`aria-label`, `title`,
 * `placeholder`, `alt`) must route its text through `i18n.tr(no, en)`.
 *
 * A test rather than an ESLint rule for the same reason as
 * `chat-route-ownership.test.ts`: `pnpm lint` cannot run against the pinned
 * TypeScript 7, and an unenforceable rule is worse than none.
 */

import { describe, expect, it } from 'vitest'

/** Attributes whose value a person reads on screen or hears read aloud. */
const NAMING_ATTRIBUTES = ['aria-label', 'aria-description', 'alt', 'placeholder', 'title']

/**
 * Strings that are the same word in both languages, so wrapping them in
 * `i18n.tr('Chat', 'Chat')` would add a hook call and no meaning. Keep this
 * list short and only for genuine loanwords, product names and file formats —
 * an entry here is a claim that translation is a no-op, not an excuse.
 */
const LOCALE_NEUTRAL = new Set([
  'Chat',
  'CSV',
  'HTML',
  'JSON',
  'PDF',
  'Verevon',
])

/**
 * A naming attribute and whatever opens its value: a quote (`title="…"`) or a
 * brace (`title={…}`). Only the value that follows is examined — reading the
 * whole LINE instead reports every `class="verevon-chat-…"` and
 * `loading="lazy"` that happens to share it, which is how the first draft of
 * this guard produced four false positives.
 */
const NAMING_ATTRIBUTE = new RegExp(
  `(${NAMING_ATTRIBUTES.join('|')})=(["'{])`,
  'g',
)

/** Quoted text inside an expression: 'x', "x", or a `x` template. */
const QUOTED_TEXT = /(['"`])((?:[^'"`\\]|\\.)*?)\1/g

/**
 * Text that has to be translated: at least one run of letters. This is what
 * separates prose from a value that happens to be a string — `'button'`,
 * `'none'`, a class name or a bare `${…}` interpolation carry no language.
 */
const HAS_WORDS = /[A-Za-zÆØÅæøå]{2,}/

function isTranslatable(text: string): boolean {
  // Strip `${…}` first: `Last ned ${name}` is prose, `${name}` alone is not.
  const withoutInterpolation = text.replace(/\$\{[^}]*\}/g, ' ').trim()
  if (!withoutInterpolation) return false
  if (LOCALE_NEUTRAL.has(withoutInterpolation)) return false
  return HAS_WORDS.test(withoutInterpolation)
}

async function chatSourceFiles(): Promise<Array<{ path: string; text: string }>> {
  const { readdirSync, readFileSync, statSync } = await import('node:fs')
  const { join, resolve } = await import('node:path')
  const root = resolve(process.cwd(), 'src', 'features', 'chat')
  const out: Array<{ path: string; text: string }> = []
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const full = join(dir, entry)
      if (statSync(full).isDirectory()) {
        walk(full)
        continue
      }
      if (!/\.tsx$/.test(entry) || /\.test\.tsx$/.test(entry)) continue
      // The separator is written by char code so no backslash literal appears
      // in this file (see the same note in chat-route-ownership.test.ts).
      const sep = String.fromCharCode(92)
      const relative = full.slice(root.length + 1).split(sep).join('/')
      out.push({ path: relative, text: readFileSync(full, 'utf8') })
    }
  }
  walk(root)
  return out
}

/**
 * The value that starts at `open` — either the rest of a quoted string or a
 * braced expression — returned with the offset just past its close. Quoted
 * text inside a brace is skipped while balancing, so a `}` inside a string
 * cannot end the expression early.
 */
function readAttributeValue(text: string, open: number): { value: string; end: number } {
  const opener = text[open] ?? ''
  if (opener !== '{') {
    const close = text.indexOf(opener, open + 1)
    if (close < 0) return { value: '', end: open + 1 }
    return { value: text.slice(open + 1, close), end: close + 1 }
  }
  let depth = 0
  let quote = ''
  for (let i = open; i < text.length; i += 1) {
    const char = text[i]
    if (quote) {
      if (char === '\\') i += 1
      else if (char === quote) quote = ''
      continue
    }
    if (char === '"' || char === "'" || char === '`') quote = char
    else if (char === '{') depth += 1
    else if (char === '}') {
      depth -= 1
      if (depth === 0) return { value: text.slice(open + 1, i), end: i + 1 }
    }
  }
  return { value: text.slice(open + 1), end: text.length }
}

/**
 * One finding per untranslated name, reported as `file:line text` so a failure
 * names the exact site instead of only a count.
 */
function findUntranslatedNames(files: Array<{ path: string; text: string }>): string[] {
  const findings: string[] = []
  for (const file of files) {
    const lineStarts = [0]
    for (let i = 0; i < file.text.length; i += 1) {
      if (file.text[i] === '\n') lineStarts.push(i + 1)
    }
    const lineOf = (offset: number) => {
      let low = 0
      let high = lineStarts.length - 1
      while (low < high) {
        const mid = Math.ceil((low + high) / 2)
        if ((lineStarts[mid] ?? 0) <= offset) low = mid
        else high = mid - 1
      }
      return low + 1
    }

    for (const match of file.text.matchAll(NAMING_ATTRIBUTE)) {
      const open = match.index + match[0].length - 1
      const { value } = readAttributeValue(file.text, open)
      const at = `${file.path}:${lineOf(match.index)}`

      if (file.text[open] !== '{') {
        if (isTranslatable(value)) findings.push(`${at} ${match[1]}="${value}"`)
        continue
      }
      // `i18n.tr(no, en)` is the fix; its two arguments are quoted text and
      // must not be read as findings.
      if (value.includes('i18n.tr(')) continue
      for (const quoted of value.matchAll(QUOTED_TEXT)) {
        const quotedText = quoted[2] ?? ''
        if (isTranslatable(quotedText)) findings.push(`${at} ${quotedText}`)
      }
    }
  }
  return findings
}

describe('acceptance criterion 9: accessible names follow the locale', () => {
  it('routes every naming attribute in features/chat through i18n.tr', async () => {
    const files = await chatSourceFiles()
    // A guard that scans nothing passes vacuously.
    expect(files.length).toBeGreaterThan(5)
    expect(findUntranslatedNames(files)).toEqual([])
  })

  it('reads a bare literal attribute as a finding', () => {
    const findings = findUntranslatedNames([
      { path: 'Fake.tsx', text: '  <button aria-label="Previous version" />' },
    ])
    expect(findings).toEqual(['Fake.tsx:1 aria-label="Previous version"'])
  })

  it('reads a literal inside an expression as a finding', () => {
    const findings = findUntranslatedNames([
      { path: 'Fake.tsx', text: "  <span title={open() ? 'Lukk' : undefined} />" },
    ])
    expect(findings).toEqual(['Fake.tsx:1 Lukk'])
  })

  it('accepts a translated attribute', () => {
    const findings = findUntranslatedNames([
      {
        path: 'Fake.tsx',
        text: "  <button aria-label={i18n.tr('Forrige versjon', 'Previous version')} />",
      },
    ])
    expect(findings).toEqual([])
  })

  it('ignores values that carry no language', () => {
    const findings = findUntranslatedNames([
      // A pure interpolation, a locale-neutral loanword, and an attribute that
      // is not a name at all.
      { path: 'Fake.tsx', text: '  <img alt={`${props.name}`} />' },
      { path: 'Fake.tsx', text: '  <div aria-label="Chat" role="tabpanel" />' },
      { path: 'Fake.tsx', text: '  <button type="button" class="verevon-chat" />' },
    ])
    expect(findings).toEqual([])
  })

  it('reads only the attribute value, not the rest of the line', () => {
    // The first draft scanned whole lines and reported the class name here.
    const findings = findUntranslatedNames([
      {
        path: 'Fake.tsx',
        text: '  <img class="verevon-chat-attachment__image" alt={caption()} loading="lazy" />',
      },
    ])
    expect(findings).toEqual([])
  })

  it('balances braces across lines and past quoted braces', () => {
    const findings = findUntranslatedNames([
      {
        path: 'Fake.tsx',
        text: [
          '  <span',
          '    title={open()',
          "      ? 'Lukk arbeidsflaten'",
          "      : '}'}",
          '    class="not-a-name"',
          '  />',
        ].join('\n'),
      },
    ])
    // The `'}'` string must not end the expression early, and the class name
    // that follows must stay out of it.
    expect(findings).toEqual(['Fake.tsx:2 Lukk arbeidsflaten'])
  })
})
