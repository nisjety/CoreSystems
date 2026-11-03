// @vitest-environment jsdom
import { fireEvent, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { I18nProvider } from '@/shared/i18n'
import { BrowserDockControls } from './BrowserDockControls'

const buttons = (): HTMLButtonElement[] =>
  Array.from(document.querySelectorAll<HTMLButtonElement>('.browser-dock-controls__button'))

const button = (position: string): HTMLButtonElement =>
  document.querySelector<HTMLButtonElement>(`.browser-dock-controls__button[data-position="${position}"]`)!

const group = (): HTMLElement => document.querySelector('.browser-dock-controls') as HTMLElement

afterEach(() => {
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('BrowserDockControls (WS9.G)', () => {
  it('offers the four layouts in order as a group, with the current one pressed', () => {
    render(() => <BrowserDockControls position="bottom" onPositionChange={vi.fn()} />)

    expect(group().getAttribute('role')).toBe('group')
    expect(group().dataset.position).toBe('bottom')
    expect(buttons().map((item) => item.dataset.position)).toEqual(['right', 'bottom', 'maximised', 'hidden'])
    expect(buttons().map((item) => item.getAttribute('aria-pressed'))).toEqual(['false', 'true', 'false', 'false'])
  })

  it('labels the layouts in Norwegian by default', () => {
    render(() => <BrowserDockControls position="right" onPositionChange={vi.fn()} />)

    expect(buttons().map((item) => item.textContent?.trim())).toEqual(['Høyre', 'Nederst', 'Maksimer', 'Skjul'])
  })

  it('labels the layouts in English under the English locale', () => {
    localStorage.setItem('verevon.locale', 'en')
    render(() => (
      <I18nProvider>
        <BrowserDockControls position="right" onPositionChange={vi.fn()} />
      </I18nProvider>
    ))

    expect(buttons().map((item) => item.textContent?.trim())).toEqual(['Right', 'Bottom', 'Maximise', 'Hide'])
  })

  it('reports the chosen layout and leaves the position to the parent', () => {
    const onPositionChange = vi.fn()
    render(() => <BrowserDockControls position="right" onPositionChange={onPositionChange} />)

    fireEvent.click(button('maximised'))

    expect(onPositionChange).toHaveBeenCalledWith('maximised')
    expect(button('right').getAttribute('aria-pressed')).toBe('true')
    expect(button('maximised').getAttribute('aria-pressed')).toBe('false')
  })

  it('presses Hide while the dock is collapsed', () => {
    render(() => <BrowserDockControls position="hidden" onPositionChange={vi.fn()} />)

    expect(button('hidden').getAttribute('aria-pressed')).toBe('true')
    expect(button('right').getAttribute('aria-pressed')).toBe('false')
  })
})
