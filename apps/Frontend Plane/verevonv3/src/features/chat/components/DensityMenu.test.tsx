import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { DensityMenu } from './DensityMenu'

afterEach(() => {
  cleanup()
  flush()
})

function trigger(container: HTMLElement): HTMLButtonElement {
  const button = container.querySelector<HTMLButtonElement>('.verevon-chat-density__trigger')
  if (!button) throw new Error('density trigger is not rendered')
  return button
}

function options(container: HTMLElement): HTMLButtonElement[] {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('.verevon-chat-density__option'))
}

describe('DensityMenu', () => {
  it('names the level in effect, with the list closed until it is opened', () => {
    const view = render(() => <DensityMenu density="standard" onChoose={vi.fn()} />)
    flush()
    expect(trigger(view.container).textContent).toContain('Standard')
    expect(trigger(view.container).getAttribute('aria-expanded')).toBe('false')
    expect(view.container.querySelector('.verevon-chat-density__panel')).toBeNull()
  })

  it('opens the four levels as a group and marks the level in effect as pressed', () => {
    const view = render(() => <DensityMenu density="compact" onChoose={vi.fn()} />)
    fireEvent.click(trigger(view.container))
    flush()
    expect(trigger(view.container).getAttribute('aria-expanded')).toBe('true')
    expect(view.container.querySelector('[role="group"]')).not.toBeNull()
    expect(options(view.container).map((option) => option.getAttribute('aria-pressed'))).toEqual([
      'false',
      'false',
      'true',
      'false',
    ])
  })

  it('chooses a level, closes the list and returns the focus to the trigger', () => {
    const onChoose = vi.fn()
    const view = render(() => <DensityMenu density="standard" onChoose={onChoose} />)
    fireEvent.click(trigger(view.container))
    flush()
    fireEvent.click(options(view.container)[3] as HTMLButtonElement)
    flush()
    expect(onChoose).toHaveBeenCalledWith('minimal')
    expect(view.container.querySelector('.verevon-chat-density__panel')).toBeNull()
    expect(document.activeElement).toBe(trigger(view.container))
  })

  it('closes on Escape without choosing, and returns the focus to the trigger', () => {
    const onChoose = vi.fn()
    const view = render(() => <DensityMenu density="standard" onChoose={onChoose} />)
    fireEvent.click(trigger(view.container))
    flush()
    const panel = view.container.querySelector('.verevon-chat-density__panel') as HTMLElement
    fireEvent.keyDown(panel, { key: 'Escape' })
    flush()
    expect(onChoose).not.toHaveBeenCalled()
    expect(view.container.querySelector('.verevon-chat-density__panel')).toBeNull()
    expect(document.activeElement).toBe(trigger(view.container))
  })
})
