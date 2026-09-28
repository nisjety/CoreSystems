import { describe, expect, it } from 'vitest'
import {
  escapeXmlAttr,
  escapeXmlText,
  formatExportDate,
  isSafeExportHref,
  plainTextFromRuns,
  sanitizeXmlText,
  stripInlineMarkdown,
  tokenizeInline,
} from './ooxml'

describe('escapeXmlText', () => {
  it('escapes & < > and leaves Norwegian æøå untouched', () => {
    expect(escapeXmlText('Salg & <profitt> økte, "æøå" > 10%')).toBe(
      'Salg &amp; &lt;profitt&gt; økte, "æøå" &gt; 10%',
    )
  })

  it('does not touch quotes in text content (only escapeXmlAttr does)', () => {
    expect(escapeXmlText(`"quoted" and 'apostrophe'`)).toBe(`"quoted" and 'apostrophe'`)
  })
})

describe('escapeXmlAttr', () => {
  it('additionally escapes double and single quotes', () => {
    expect(escapeXmlAttr(`He said "hi" & 'bye'`)).toBe('He said &quot;hi&quot; &amp; &apos;bye&apos;')
  })
})

describe('sanitizeXmlText', () => {
  it('strips C0 control characters other than tab, LF and CR', () => {
    const withControls = 'a\u0000b\u0001c\u0007\td\ne\rf'
    expect(sanitizeXmlText(withControls)).toBe('abc\td\ne\rf')
  })

  it('keeps a valid surrogate pair (an emoji) intact', () => {
    const rocket = '\u{1F680}' // encodes as a high+low surrogate pair
    expect(sanitizeXmlText(`before ${rocket} after`)).toBe(`before ${rocket} after`)
  })

  it('drops a lone high surrogate', () => {
    const lone = 'before \uD83D after' // high surrogate with no low surrogate following
    expect(sanitizeXmlText(lone)).toBe('before  after')
  })

  it('drops a lone low surrogate', () => {
    const lone = 'before \uDC00 after'
    expect(sanitizeXmlText(lone)).toBe('before  after')
  })
})

describe('tokenizeInline', () => {
  it('tokenizes a code span', () => {
    expect(tokenizeInline('use `npm install` now')).toEqual([
      { text: 'use ' },
      { text: 'npm install', code: true },
      { text: ' now' },
    ])
  })

  it('tokenizes bold and italic', () => {
    expect(tokenizeInline('**viktig** og *litt kursiv*')).toEqual([
      { text: 'viktig', bold: true },
      { text: ' og ' },
      { text: 'litt kursiv', italic: true },
    ])
  })

  it('tokenizes a markdown link', () => {
    expect(tokenizeInline('se [Verevon](https://verevon.example/om)')).toEqual([
      { text: 'se ' },
      { text: 'Verevon', href: 'https://verevon.example/om' },
    ])
  })

  it('keeps a citation marker as literal text (its own run, unstyled)', () => {
    expect(tokenizeInline('dette er dokumentert [1]')).toEqual([
      { text: 'dette er dokumentert ' },
      { text: '[1]' },
    ])
  })

  it('keeps a multi-index citation marker as literal text', () => {
    expect(tokenizeInline('flere kilder [2, 3]')).toEqual([
      { text: 'flere kilder ' },
      { text: '[2, 3]' },
    ])
  })

  it('keeps inline math as raw text, delimiters included', () => {
    expect(tokenizeInline('formelen er $E = mc^2$ her')).toEqual([
      { text: 'formelen er ' },
      { text: '$E = mc^2$' },
      { text: ' her' },
    ])
  })

  it('autolinks a bare URL and splits trailing punctuation', () => {
    expect(tokenizeInline('se https://example.com/side.')).toEqual([
      { text: 'se ' },
      { text: 'https://example.com/side', href: 'https://example.com/side' },
      { text: '.' },
    ])
  })

  it('unescapes a backslash-escaped character in plain text', () => {
    expect(tokenizeInline('\\#arbeidsplass')).toEqual([{ text: '#arbeidsplass' }])
  })

  it('does not treat a currency figure as inline math', () => {
    expect(tokenizeInline('kostet $5 og $10 totalt')).toEqual([{ text: 'kostet $5 og $10 totalt' }])
  })

  it('drops the href of a markdown link whose scheme is not http(s)/mailto, keeping only its visible text', () => {
    // The trailing stray ")" is the same "url stops at the first close-paren"
    // behaviour `parseInline` in chat-media-markdown.tsx already has for a
    // scheme (like `javascript:alert(1)`) that itself contains parentheses —
    // unrelated to the href-safety check this test is really after.
    expect(tokenizeInline('se [her](javascript:alert(1))')).toEqual([
      { text: 'se ' },
      { text: 'her' },
      { text: ')' },
    ])
    expect(tokenizeInline('se [fil](file:///etc/passwd)')).toEqual([
      { text: 'se ' },
      { text: 'fil' },
    ])
    expect(tokenizeInline('se [data](data:text/plain,hallo)')).toEqual([
      { text: 'se ' },
      { text: 'data' },
    ])
  })

  it('keeps an http(s) or mailto markdown link as a real href', () => {
    expect(tokenizeInline('[epost](mailto:ola@example.com)')).toEqual([
      { text: 'epost', href: 'mailto:ola@example.com' },
    ])
  })
})

describe('isSafeExportHref', () => {
  it('allows http, https and mailto', () => {
    expect(isSafeExportHref('https://example.com')).toBe(true)
    expect(isSafeExportHref('http://example.com')).toBe(true)
    expect(isSafeExportHref('mailto:ola@example.com')).toBe(true)
  })

  it('rejects javascript:, data: and file: schemes', () => {
    expect(isSafeExportHref('javascript:alert(1)')).toBe(false)
    expect(isSafeExportHref('data:text/html,<script>alert(1)</script>')).toBe(false)
    expect(isSafeExportHref('file:///etc/passwd')).toBe(false)
  })

  it('rejects a schemeless or fragment-only value', () => {
    expect(isSafeExportHref('#')).toBe(false)
    expect(isSafeExportHref('/relative/path')).toBe(false)
  })
})

describe('stripInlineMarkdown / plainTextFromRuns', () => {
  it('reduces formatted text to plain text', () => {
    expect(stripInlineMarkdown('**Total**: `1 234,50` kr — se [detaljer](https://x.test)')).toBe(
      'Total: 1 234,50 kr — se detaljer',
    )
  })

  it('plainTextFromRuns joins run text in order', () => {
    expect(plainTextFromRuns(tokenizeInline('a **b** c'))).toBe('a b c')
  })
})

describe('formatExportDate', () => {
  it('formats a Norwegian long date by default', () => {
    expect(formatExportDate('2026-09-28T10:00:00Z', 'no')).toContain('2026')
  })

  it('falls back to "now" for an unparseable date rather than throwing', () => {
    expect(() => formatExportDate('not-a-date', 'no')).not.toThrow()
  })
})
