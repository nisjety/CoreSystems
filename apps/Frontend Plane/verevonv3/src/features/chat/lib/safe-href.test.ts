import { describe, expect, it } from 'vitest'
import { isSafeLinkHref } from './safe-href'

describe('isSafeLinkHref', () => {
  it('accepts http, https and mailto, in any letter case', () => {
    expect(isSafeLinkHref('https://example.com/a?b=1')).toBe(true)
    expect(isSafeLinkHref('http://example.com')).toBe(true)
    expect(isSafeLinkHref('mailto:ola@example.com')).toBe(true)
    expect(isSafeLinkHref('HTTPS://EXAMPLE.COM')).toBe(true)
  })

  it('rejects javascript: in any letter case', () => {
    expect(isSafeLinkHref('javascript:alert(1)')).toBe(false)
    expect(isSafeLinkHref('JaVaScRiPt:alert(1)')).toBe(false)
  })

  it('rejects a scheme split by a tab, newline or other control character', () => {
    expect(isSafeLinkHref('java\tscript:alert(1)')).toBe(false)
    expect(isSafeLinkHref('java\nscript:alert(1)')).toBe(false)
    expect(isSafeLinkHref('\u0000javascript:alert(1)')).toBe(false)
  })

  it('rejects control characters even inside an allowed scheme', () => {
    expect(isSafeLinkHref('https://exa\u0007mple.com')).toBe(false)
    expect(isSafeLinkHref('https://example.com/\u007f')).toBe(false)
  })

  it('rejects data:, file:, blob:, vbscript: and protocol-relative targets', () => {
    expect(isSafeLinkHref('data:text/html,<script>alert(1)</script>')).toBe(false)
    expect(isSafeLinkHref('file:///etc/passwd')).toBe(false)
    expect(isSafeLinkHref('blob:https://example.com/3f2a')).toBe(false)
    expect(isSafeLinkHref('vbscript:msgbox(1)')).toBe(false)
    expect(isSafeLinkHref('//evil.example.com/x')).toBe(false)
  })

  it('rejects relative, fragment-only, schemeless and empty targets', () => {
    expect(isSafeLinkHref('/innstillinger')).toBe(false)
    expect(isSafeLinkHref('#')).toBe(false)
    expect(isSafeLinkHref('evil.example.com')).toBe(false)
    expect(isSafeLinkHref('')).toBe(false)
  })

  it('trims surrounding whitespace before it decides', () => {
    expect(isSafeLinkHref('  https://example.com  ')).toBe(true)
    expect(isSafeLinkHref('  javascript:alert(1)')).toBe(false)
  })
})
