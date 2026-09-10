import { describe, expect, it } from 'vitest'

async function readCss(): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), 'src/styles/global.css'), 'utf8')
}

function ruleBody(css: string, selector: string): string {
  const start = css.indexOf(`${selector} {`)
  expect(start, `selector not found: ${selector}`).toBeGreaterThan(-1)
  const end = css.indexOf('}', start)
  return css.slice(start, end)
}

describe('Inbox viewport layout', () => {
  it('keeps all three desktop panes inside the available workspace', async () => {
    const css = await readCss()
    const workspace = ruleBody(css, '.verevon-inbox-workspace')
    const detail = ruleBody(css, '.verevon-inbox-detail-grid')

    expect(workspace).toContain('height: 100%')
    expect(workspace).toContain('overflow: hidden')
    expect(workspace).toContain('min(var(--inbox-list-w, 340px), 34%)')
    expect(detail).toContain('height: 100%')
    expect(detail).toContain('overflow: hidden')
    expect(detail).toContain('--inbox-aside-track: clamp(220px, var(--inbox-aside-w, 280px), calc(100% - 228px))')
    expect(detail).toContain('grid-template-columns: minmax(220px, 1fr) var(--inbox-aside-track)')
  })

  it('does not move the context pane below the conversation at laptop widths', async () => {
    const css = await readCss()
    expect(css).not.toContain('@media (max-width: 1279px)')
    expect(ruleBody(css, '.verevon-inbox-page')).toContain('overflow: hidden')
  })

  it('scrolls inside each pane instead of the page shell', async () => {
    const css = await readCss()
    expect(ruleBody(css, '.verevon-inbox-ticket-list')).toContain('overflow-y: auto')
    expect(ruleBody(css, '.verevon-inbox-transcript')).toContain('overflow-y: auto')
    expect(ruleBody(css, '.verevon-inbox-aside-scroll')).toContain('overflow-y: auto')
    expect(ruleBody(css, '.verevon-inbox-aside-scroll')).toContain('overflow-x: hidden')
    const tabpanel = ruleBody(css, ".verevon-inbox-aside > [role='tabpanel']")
    expect(tabpanel).toContain('min-height: 0')
    expect(tabpanel).toContain('overflow: hidden')
  })

  it('keeps the context resize affordance visible and aligned to its track', async () => {
    const css = await readCss()
    expect(ruleBody(css, '.verevon-inbox-resize-handle::after')).toContain('background: var(--verevon-border-soft)')
    expect(ruleBody(css, '.verevon-inbox-resize-handle--aside')).toContain('right: calc(var(--inbox-aside-track) + 4px)')
  })
})
