import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'

import { MentionMarkdown } from './MentionText'

// DEPTH-WS08-S6-2 review gap: the other MentionMarkdown tests stub ChatMarkdown. This file renders
// the real ChatMarkdown (its For/Switch block views), so a highlight effect that ran before the
// markdown children existed would fail here instead of passing silently.

const names = ['Kari Nordmann', 'Ola Hansen', 'Lagerhjelpen']

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i += 1) await new Promise((resolve) => setTimeout(resolve, 0))
}

afterEach(cleanup)

describe('MentionMarkdown with the real ChatMarkdown', () => {
  it('marks mentions in paragraphs, emphasis and list items', async () => {
    const text = 'Hei @Ola Hansen, **se @Lagerhjelpen**\n\n- takk @Kari Nordmann\n- @Nobody her'
    const { container } = render(() => <MentionMarkdown text={text} names={names} viewerName="Kari Nordmann" />)
    await settle()
    expect(container.querySelector('.verevon-chat-markdown')).not.toBeNull()
    const marks = [...container.querySelectorAll('mark.verevon-space-mention')] as HTMLElement[]
    expect(marks.map((mark) => mark.textContent)).toEqual(['@Ola Hansen', '@Lagerhjelpen', '@Kari Nordmann'])
    expect(marks.map((mark) => mark.dataset.self ?? null)).toEqual([null, null, 'true'])
  })

  it('leaves inline code and fenced code untouched', async () => {
    const text = 'Se `@Ola Hansen` her\n\n```\n@Kari Nordmann\n```'
    const { container } = render(() => <MentionMarkdown text={text} names={names} />)
    await settle()
    expect(container.textContent).toContain('@Ola Hansen')
    expect(container.textContent).toContain('@Kari Nordmann')
    expect(container.querySelectorAll('mark.verevon-space-mention').length).toBe(0)
  })
})
