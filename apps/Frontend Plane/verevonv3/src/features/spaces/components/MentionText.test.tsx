import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

// DEPTH-WS08-S6-2: mentions on the markdown path. ChatMarkdown is stubbed to a block that holds a
// link, inline code and plain text, the three cases the highlight must tell apart.
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => (
    <div class="chat-md-stub">
      <p>
        {props.content} <a href="https://example.test/@Ola Hansen">@Ola Hansen</a> <code>@Kari Nordmann</code>
      </p>
    </div>
  ),
}))

const { highlightMentionsIn, MentionMarkdown } = await import('./MentionText')

const names = ['Kari Nordmann', 'Ola Hansen', 'Lagerhjelpen']

afterEach(cleanup)

describe('highlightMentionsIn (DEPTH-WS08-S6-2)', () => {
  function element(html: string): HTMLElement {
    const root = document.createElement('div')
    root.innerHTML = html
    return root
  }

  it('marks a known name in a text node and the reader as self', () => {
    const root = element('<p>Hei @Ola Hansen og @Kari Nordmann, se <strong>@Lagerhjelpen</strong></p>')
    highlightMentionsIn(root, names, 'Kari Nordmann')
    const marks = [...root.querySelectorAll('mark.verevon-space-mention')]
    expect(marks.map((mark) => mark.textContent)).toEqual(['@Ola Hansen', '@Kari Nordmann', '@Lagerhjelpen'])
    expect(marks.map((mark) => (mark as HTMLElement).dataset.self ?? null)).toEqual([null, 'true', null])
    expect(root.textContent).toBe('Hei @Ola Hansen og @Kari Nordmann, se @Lagerhjelpen')
  })

  it('leaves links, code and unknown names alone', () => {
    const root = element(
      '<p><a href="https://x.test/@Ola">@Ola Hansen</a> <code>@Ola Hansen</code> <pre>@Ola Hansen</pre> @Nobody here e-post@Ola Hansen</p>',
    )
    highlightMentionsIn(root, names)
    expect(root.querySelectorAll('mark').length).toBe(0)
    expect(root.querySelector('a')?.getAttribute('href')).toBe('https://x.test/@Ola')
  })

  it('never parses author text as HTML and is idempotent', () => {
    const root = element('<p></p>')
    root.querySelector('p')!.appendChild(document.createTextNode('@Ola Hansen <img src=x onerror=alert(1)>'))
    highlightMentionsIn(root, names)
    highlightMentionsIn(root, names)
    expect(root.querySelectorAll('mark').length).toBe(1)
    expect(root.querySelector('img')).toBeNull()
    expect(root.textContent).toBe('@Ola Hansen <img src=x onerror=alert(1)>')
  })
})

describe('MentionMarkdown (DEPTH-WS08-S6-2)', () => {
  it('highlights mentions in the rendered markdown text, not in its link or code', async () => {
    render(() => <MentionMarkdown class="md" text="Takk @Ola Hansen" names={names} viewerName="Kari Nordmann" />)
    await Promise.resolve()
    const root = document.querySelector('.md') as HTMLElement
    const marks = [...root.querySelectorAll('mark.verevon-space-mention')]
    expect(marks.map((mark) => mark.textContent)).toEqual(['@Ola Hansen'])
    expect(marks[0]?.closest('a')).toBeNull()
    expect(root.querySelector('a')?.textContent).toBe('@Ola Hansen')
    expect(root.querySelector('code')?.textContent).toBe('@Kari Nordmann')
  })
})
