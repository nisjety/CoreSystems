import { describe, expect, it } from 'vitest'
import {
  MAX_LIVE_WIDGETS,
  MAX_WIDGET_CHARS,
  isWidgetFence,
  isWithinLiveWindow,
  widgetThemeCss,
  widgetTitle,
  widgetTooLong,
} from './inline-widget'

describe('isWidgetFence', () => {
  it('a closed widget fence is a widget', () => {
    expect(isWidgetFence({ lang: 'widget', closed: true })).toBe(true)
    expect(isWidgetFence({ lang: ' Widget ', closed: true })).toBe(true)
  })

  it('a streaming fence is not', () => {
    expect(isWidgetFence({ lang: 'widget', closed: false })).toBe(false)
  })

  it('html and svg fences are not widgets', () => {
    expect(isWidgetFence({ lang: 'html', closed: true })).toBe(false)
    expect(isWidgetFence({ lang: 'svg', closed: true })).toBe(false)
    expect(isWidgetFence({ lang: '', closed: true })).toBe(false)
  })
})

describe('widgetTooLong', () => {
  it('over the cap is refused, at the cap is not', () => {
    expect(widgetTooLong('x'.repeat(MAX_WIDGET_CHARS))).toBe(false)
    expect(widgetTooLong('x'.repeat(MAX_WIDGET_CHARS + 1))).toBe(true)
  })
})

describe('widgetThemeCss', () => {
  const host: Record<string, string> = {
    '--verevon-bg': ' #fff ',
    '--verevon-text': 'rgb(17, 17, 17)',
    '--verevon-accent': '#0a5',
    '--font-sans': '"Inter", system-ui, sans-serif',
    'font-size': '15px',
  }

  it('maps host tokens onto --vv-* and leaves out the empty ones', () => {
    const css = widgetThemeCss((name) => host[name] ?? '')
    expect(css.startsWith(':root{')).toBe(true)
    expect(css).toContain('--vv-background:#fff')
    expect(css).toContain('--vv-text:rgb(17, 17, 17)')
    expect(css).toContain('--vv-accent:#0a5')
    expect(css).toContain('--vv-font-family:Inter, system-ui, sans-serif')
    expect(css).toContain('--vv-font-size:15px')
    expect(css).not.toContain('--vv-surface')
  })

  it('theme values cannot close the style element', () => {
    const css = widgetThemeCss((name) =>
      name === '--verevon-accent' ? 'red}</style><script>alert(1)</script><style>:root{x:y;' : '',
    )
    expect(css).not.toContain('<')
    expect(css).not.toContain('>')
    expect(css).not.toContain('/')
    expect(css).not.toContain(';')
    expect(css.indexOf('}')).toBe(css.length - 1)
    expect(css.indexOf('{')).toBe(':root'.length)
  })

  it('bounds a very long value', () => {
    const css = widgetThemeCss((name) => (name === '--verevon-text' ? 'a'.repeat(5_000) : ''))
    expect(css.length).toBeLessThan(300)
  })
})

describe('widgetTitle', () => {
  it('reads the first title, then an aria-label', () => {
    expect(widgetTitle('<title> Lånekalkulator </title><div aria-label="x"></div>')).toBe('Lånekalkulator')
    expect(widgetTitle('<svg aria-label="Rentekurve"></svg>')).toBe('Rentekurve')
  })

  it('is null when the widget names nothing', () => {
    expect(widgetTitle('<div>hei</div>')).toBeNull()
    expect(widgetTitle('<title>   </title>')).toBeNull()
  })
})

describe('isWithinLiveWindow', () => {
  it('keeps only the newest widgets live', () => {
    const mounted = Array.from({ length: MAX_LIVE_WIDGETS + 2 }, (_, index) => index)
    expect(isWithinLiveWindow(mounted, 0)).toBe(false)
    expect(isWithinLiveWindow(mounted, 1)).toBe(false)
    expect(isWithinLiveWindow(mounted, 2)).toBe(true)
    expect(isWithinLiveWindow(mounted, MAX_LIVE_WIDGETS + 1)).toBe(true)
  })

  it('treats a widget not yet registered as the newest', () => {
    const mounted = Array.from({ length: 20 }, (_, index) => index)
    expect(isWithinLiveWindow(mounted, 20)).toBe(true)
  })
})
