import { describe, expect, it } from 'vitest'
import { webAddress } from './web-address'

describe('Nett address detection', () => {
  it('opens absolute and ordinary domain addresses as web pages', () => {
    expect(webAddress('https://example.com/path?q=x')).toBe('https://example.com/path?q=x')
    expect(webAddress('example.com/path')).toBe('https://example.com/path')
  })

  it('keeps searches and unsafe addresses out of the browser action', () => {
    for (const text of ['how to use ai', 'javascript:alert(1)', 'file:///secret', 'https://alice:secret@example.com', '']) {
      expect(webAddress(text)).toBeNull()
    }
  })
})
