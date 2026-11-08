import { describe, expect, it } from 'vitest'
import { sandboxHtmlDocument, sandboxWidgetDocument } from './sandbox-html'

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

describe('sandboxWidgetDocument', () => {
  const theme = ':root{--vv-text:#111;--vv-accent:#0a5}'

  it('puts the CSP before the theme, and the theme before the widget', () => {
    const output = sandboxWidgetDocument('<style>body{color:red}</style><p>w</p>', theme)
    expectCspFirst(output)
    const head = parse(output).head
    expect(head.children[1]?.tagName).toBe('STYLE')
    expect(head.children[1]?.textContent?.startsWith(theme)).toBe(true)
    expect(output.indexOf(theme)).toBeLessThan(output.indexOf('<body>'))
    expect(parse(output).body.querySelector('p')?.textContent).toBe('w')
  })

  it('keeps the CSP first when the widget is a full document with its own head', () => {
    expectCspFirst(sandboxWidgetDocument('<html><head><meta http-equiv="refresh" content="0"></head><body>x</body></html>', theme))
  })

  it('writes no script without the bridge, the same document as before', () => {
    const plain = sandboxWidgetDocument('<p>w</p>', theme)
    expect(sandboxWidgetDocument('<p>w</p>', theme, {})).toBe(plain)
    expect(sandboxWidgetDocument('<p>w</p>', theme, { bridgeShim: '' })).toBe(plain)
    expect(parse(plain).head.querySelector('script')).toBeNull()
  })

  it('puts the bridge shim after the CSP and theme and before the widget', () => {
    const shim = 'window.verevon={}'
    const output = sandboxWidgetDocument('<script>verevon.resize(200)</script>', theme, { bridgeShim: shim })
    expectCspFirst(output)
    const head = parse(output).head
    expect(head.children[1]?.tagName).toBe('STYLE')
    expect(head.children[2]?.tagName).toBe('SCRIPT')
    expect(head.children[2]?.textContent).toBe(shim)
    expect(output.indexOf(shim)).toBeLessThan(output.indexOf('<body>'))
    expect(output.indexOf(shim)).toBeGreaterThan(output.indexOf(theme))
  })
})

describe('sandboxHtmlDocument is unchanged by the widget wrapper', () => {
  it('writes exactly the preview document it always did', () => {
    expect(sandboxHtmlDocument(' <p>x</p> ')).toBe(
      '<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="' +
        "default-src 'none'; base-uri 'none'; child-src 'none'; connect-src 'none'; font-src data:; " +
        "form-action 'none'; frame-ancestors 'none'; frame-src 'none'; img-src data: blob:; " +
        "media-src data: blob:; object-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; " +
        "worker-src 'none'" +
        '"></head><body><p>x</p></body></html>',
    )
  })
})
