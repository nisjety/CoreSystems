import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChapterEntry } from '../lib/chapter-toc'
import { ReaderTools } from './ReaderTools'

afterEach(() => {
  cleanup()
  flush()
})

const outline: ChapterEntry[] = [
  { turnId: 'a1', level: 1, title: 'Første' },
  { turnId: 'a2', level: 3, title: 'Detalj' },
]

/** The font toggle is the last button; the outline trigger, when there is one, comes before it. */
function fontToggle(container: HTMLElement): HTMLButtonElement {
  const buttons = container.querySelectorAll<HTMLButtonElement>('.verevon-chat-reader-tools__button')
  const button = buttons[buttons.length - 1]
  if (!button) throw new Error('font toggle is not rendered')
  return button
}

function outlineTrigger(container: HTMLElement): HTMLButtonElement | null {
  return container.querySelector<HTMLButtonElement>(
    '.verevon-chat-reader-tools__outline > .verevon-chat-reader-tools__button',
  )
}

describe('ReaderTools font toggle', () => {
  it('reports the next choice when the font is pressed while it is off', () => {
    const onDyslexiaFont = vi.fn()
    const view = render(() => (
      <ReaderTools chapters={[]} dyslexiaFont={false} onDyslexiaFont={onDyslexiaFont} onJump={vi.fn()} />
    ))
    expect(fontToggle(view.container).getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(fontToggle(view.container))
    expect(onDyslexiaFont).toHaveBeenCalledWith(true)
  })

  it('shows the font as pressed while it is on, and reports off when it is pressed again', () => {
    const onDyslexiaFont = vi.fn()
    const view = render(() => (
      <ReaderTools chapters={[]} dyslexiaFont={true} onDyslexiaFont={onDyslexiaFont} onJump={vi.fn()} />
    ))
    expect(fontToggle(view.container).getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(fontToggle(view.container))
    expect(onDyslexiaFont).toHaveBeenCalledWith(false)
  })
})

describe('ReaderTools outline', () => {
  it('offers no outline for a single heading', () => {
    const view = render(() => (
      <ReaderTools chapters={outline.slice(0, 1)} dyslexiaFont={false} onDyslexiaFont={vi.fn()} onJump={vi.fn()} />
    ))
    expect(outlineTrigger(view.container)).toBeNull()
  })

  it('opens with one entry per heading, marked by its level', () => {
    const view = render(() => (
      <ReaderTools chapters={outline} dyslexiaFont={false} onDyslexiaFont={vi.fn()} onJump={vi.fn()} />
    ))
    expect(outlineTrigger(view.container)?.getAttribute('aria-expanded')).toBe('false')
    expect(view.container.querySelector('nav')).toBeNull()
    fireEvent.click(outlineTrigger(view.container) as HTMLButtonElement)
    flush()
    expect(outlineTrigger(view.container)?.getAttribute('aria-expanded')).toBe('true')
    const items = Array.from(view.container.querySelectorAll<HTMLButtonElement>('.verevon-chat-reader-tools__item'))
    expect(items.map((item) => item.textContent)).toEqual(['Første', 'Detalj'])
    expect(items.map((item) => item.dataset.level)).toEqual(['1', '3'])
  })

  it('jumps to the chosen turn, closes the outline and returns the focus to its button', () => {
    const onJump = vi.fn()
    const view = render(() => (
      <ReaderTools chapters={outline} dyslexiaFont={false} onDyslexiaFont={vi.fn()} onJump={onJump} />
    ))
    fireEvent.click(outlineTrigger(view.container) as HTMLButtonElement)
    flush()
    const items = view.container.querySelectorAll<HTMLButtonElement>('.verevon-chat-reader-tools__item')
    fireEvent.click(items[1] as HTMLButtonElement)
    flush()
    expect(onJump).toHaveBeenCalledWith('a2')
    expect(view.container.querySelector('nav')).toBeNull()
    expect(document.activeElement).toBe(outlineTrigger(view.container))
  })

  it('closes on Escape without jumping, and returns the focus to its button', () => {
    const onJump = vi.fn()
    const view = render(() => (
      <ReaderTools chapters={outline} dyslexiaFont={false} onDyslexiaFont={vi.fn()} onJump={onJump} />
    ))
    fireEvent.click(outlineTrigger(view.container) as HTMLButtonElement)
    flush()
    const panel = view.container.querySelector('nav') as HTMLElement
    fireEvent.keyDown(panel, { key: 'Escape' })
    flush()
    expect(onJump).not.toHaveBeenCalled()
    expect(view.container.querySelector('nav')).toBeNull()
    expect(document.activeElement).toBe(outlineTrigger(view.container))
  })
})
