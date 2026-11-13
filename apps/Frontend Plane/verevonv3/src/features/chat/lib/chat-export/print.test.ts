// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { printAnswerAsPdf } from './print'

// W3-EP-4-S1-export: "Save as PDF" prints a framed widget or MCP app as its code, never as its live,
// sandboxed iframe or its toolbar. The swap is made on the print's own clone; the live answer keeps
// its frames.

function answerWithFrames(): HTMLElement {
  const answer = document.createElement('div')
  answer.innerHTML = `
    <p>Here is the chart you asked for.</p>
    <figure class="verevon-inline-widget" data-source="ignored-when-a-twin-exists">
      <div class="verevon-inline-widget__toolbar"><button type="button">Copy</button></div>
      <iframe sandbox="allow-scripts" srcdoc="<p>live widget</p>"></iframe>
      <pre class="verevon-inline-widget__export" hidden><code>const widget = render(chart)</code></pre>
    </figure>
    <figure class="verevon-inline-widget verevon-mcp-app" data-mcp-server="server-1">
      <iframe sandbox="allow-scripts"></iframe>
      <pre class="verevon-inline-widget__export" hidden><code>&lt;div id="mcp-app"&gt;&lt;/div&gt;</code></pre>
    </figure>
    <figure class="verevon-inline-widget" data-source="plot(x, y)">
      <iframe sandbox="allow-scripts"></iframe>
    </figure>
    <figure class="verevon-inline-widget">
      <iframe sandbox="allow-scripts"></iframe>
      <div class="verevon-inline-widget__toolbar"></div>
    </figure>
    <iframe class="stray"></iframe>
  `
  document.body.appendChild(answer)
  return answer
}

function capturePrintFrame(): () => HTMLIFrameElement {
  let frame: HTMLIFrameElement | undefined
  const append = document.body.appendChild.bind(document.body)
  vi.spyOn(document.body, 'appendChild').mockImplementation(<T extends Node>(node: T): T => {
    const appended = append(node)
    if (node instanceof HTMLIFrameElement && !frame) {
      frame = node
      // jsdom has no print dialog; the swap under test is synchronous, before the print is asked.
      if (node.contentWindow) node.contentWindow.print = vi.fn()
    }
    return appended
  })
  return () => {
    if (!frame) throw new Error('printAnswerAsPdf appended no print frame')
    return frame
  }
}

afterEach(() => {
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

describe('printAnswerAsPdf', () => {
  it('prints frames as their code, with no iframe and no toolbar in the printed document', () => {
    const answer = answerWithFrames()
    const printFrame = capturePrintFrame()

    printAnswerAsPdf(answer, 'Answer')

    const printed = printFrame().contentDocument!
    const root = printed.querySelector('.verevon-chat-print-root')!
    expect(root).not.toBeNull()
    expect(root.querySelectorAll('iframe')).toHaveLength(0)
    expect(root.querySelectorAll('.verevon-inline-widget__toolbar')).toHaveLength(0)
    expect(root.querySelectorAll('.verevon-inline-widget')).toHaveLength(0)

    const text = root.textContent ?? ''
    expect(text).toContain('Here is the chart you asked for.')
    expect(text).toContain('const widget = render(chart)')
    expect(text).toContain('<div id="mcp-app"></div>')
    // A frame with no twin prints its data-source.
    expect(text).toContain('plot(x, y)')
    expect(text).not.toContain('ignored-when-a-twin-exists')

    // The twins that print are visible in the copy.
    for (const twin of root.querySelectorAll('.verevon-inline-widget__export')) {
      expect(twin.hasAttribute('hidden')).toBe(false)
    }
    expect(printed.title).toBe('Answer')
  })

  it('leaves the live answer untouched', () => {
    const answer = answerWithFrames()
    const before = answer.innerHTML
    capturePrintFrame()

    printAnswerAsPdf(answer, 'Answer')

    expect(answer.innerHTML).toBe(before)
    expect(answer.querySelectorAll('iframe')).toHaveLength(5)
    for (const twin of answer.querySelectorAll('.verevon-inline-widget__export')) {
      expect(twin.hasAttribute('hidden')).toBe(true)
    }
  })

  it('prints an answer with no frames as it is', () => {
    const answer = document.createElement('div')
    answer.innerHTML = '<p>Plain <code>answer</code></p>'
    document.body.appendChild(answer)
    const printFrame = capturePrintFrame()

    printAnswerAsPdf(answer, 'Plain')

    const root = printFrame().contentDocument!.querySelector('.verevon-chat-print-root')!
    expect(root.innerHTML).toBe(`<div>${answer.innerHTML}</div>`)
  })
})
