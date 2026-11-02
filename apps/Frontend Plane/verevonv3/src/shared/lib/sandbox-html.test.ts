import { describe, expect, it } from 'vitest'
import { sandboxHtmlDocument } from './sandbox-html'

function parse(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html')
}

function expectCspFirst(output: string) {
  expect(output.startsWith('<!doctype html><html><head>')).toBe(true)
  const head = parse(output).head
  const first = head.firstElementChild
  expect(first?.tagName).toBe('META')
  expect(first?.getAttribute('http-equiv')).toBe('Content-Security-Policy')
  expect(first?.getAttribute('content')).toContain("default-src 'none'")
  expect(first?.getAttribute('content')).toContain("connect-src 'none'")
  expect(head.querySelectorAll('meta[http-equiv="Content-Security-Policy"]')).toHaveLength(1)
}

describe('sandboxHtmlDocument', () => {
  it('wraps a fragment with the CSP meta as the first head child', () => {
    const output = sandboxHtmlDocument('<p>hello</p>')
    expectCspFirst(output)
    expect(parse(output).body.textContent).toBe('hello')
  })

  it('keeps the CSP first when the artifact is a full document with its own head', () => {
    expectCspFirst(sandboxHtmlDocument('<!doctype html><html><head><title>t</title></head><body>x</body></html>'))
  })

  it('is not fooled by a commented-out head', () => {
    expectCspFirst(sandboxHtmlDocument('<!-- <head> --><p>x</p>'))
  })

  it('is not fooled by a head string inside a script', () => {
    expectCspFirst(sandboxHtmlDocument('<script>var s = "<head>"</script><p>x</p>'))
  })

  it('trims surrounding whitespace and handles empty input', () => {
    expectCspFirst(sandboxHtmlDocument('   \n<p>x</p>\n  '))
    expectCspFirst(sandboxHtmlDocument(''))
  })
})
