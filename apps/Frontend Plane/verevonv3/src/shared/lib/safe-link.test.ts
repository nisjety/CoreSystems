import { describe, expect, it } from 'vitest'
import { linkHost, looksSpoofed, safeHref } from './safe-link'

describe('safeHref', () => {
  it('accepts https, http and mailto targets and trims them', () => {
    expect(safeHref('https://example.com/a')).toBe('https://example.com/a')
    expect(safeHref('http://example.com')).toBe('http://example.com')
    expect(safeHref('mailto:ola@example.com')).toBe('mailto:ola@example.com')
    expect(safeHref('  https://example.com/a  ')).toBe('https://example.com/a')
    expect(safeHref('HTTPS://EXAMPLE.COM')).toBe('HTTPS://EXAMPLE.COM')
  })

  it.each([
    'javascript:alert(1)',
    'JaVaScRiPt:alert(1)',
    'java\tscript:alert(1)',
    'data:text/html,hello',
    'vbscript:msgbox',
    'file:///etc/passwd',
    'blob:https://example.com/x',
    'example.com/path',
    'www.example.com',
    '//example.com',
    '/settings',
    './relative',
    '',
    '   ',
  ])('refuses %j', (raw) => {
    expect(safeHref(raw)).toBeNull()
  })

  it('refuses a non-string without throwing', () => {
    expect(safeHref(undefined as unknown as string)).toBeNull()
    expect(safeHref(42 as unknown as string)).toBeNull()
  })
})

describe('linkHost', () => {
  it('returns the lower-cased host of a web link', () => {
    expect(linkHost('https://Docs.Example.com/a?b=1')).toBe('docs.example.com')
    expect(linkHost('http://example.com:8080/x')).toBe('example.com')
  })

  it('returns the domain of a mailto address', () => {
    expect(linkHost('mailto:Ola@Example.com')).toBe('example.com')
    expect(linkHost('mailto:ola@example.com?subject=hi')).toBe('example.com')
  })

  it('returns null for an unsafe target or an address without a domain', () => {
    expect(linkHost('javascript:alert(1)')).toBeNull()
    expect(linkHost('mailto:ola')).toBeNull()
    expect(linkHost('mailto:ola@')).toBeNull()
    expect(linkHost('/relative')).toBeNull()
  })
})

describe('looksSpoofed', () => {
  it('flags a domain-like label that points to another host', () => {
    expect(looksSpoofed('example.com', 'https://other.test')).toBe(true)
    expect(looksSpoofed('https://example.com/login', 'https://other.test/login')).toBe(true)
  })

  it('does not flag a label whose host matches, including a www prefix', () => {
    expect(looksSpoofed('example.com', 'https://example.com/path')).toBe(false)
    expect(looksSpoofed('www.example.com', 'https://example.com')).toBe(false)
    expect(looksSpoofed('example.com', 'https://www.example.com')).toBe(false)
    expect(looksSpoofed('https://example.com/a', 'https://EXAMPLE.com/b')).toBe(false)
  })

  it('does not flag plain words or sentences', () => {
    expect(looksSpoofed('the report', 'https://other.test')).toBe(false)
    expect(looksSpoofed('click here', 'https://other.test')).toBe(false)
    expect(looksSpoofed('Source', 'https://other.test')).toBe(false)
  })

  it('does not flag when the target itself is unsafe', () => {
    expect(looksSpoofed('example.com', 'javascript:alert(1)')).toBe(false)
  })
})
