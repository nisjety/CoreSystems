import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const { SpaceMessageEdit } = await import('./SpaceMessageEdit')

// No I18nProvider wraps these renders, so the copy is the default Norwegian.
const ORIGINAL = 'Hei alle'
const FALLBACK = 'Meldingen kunne ikke endres. Ingenting er endret. Prøv igjen om litt.'

afterEach(() => {
  cleanup()
})

function renderEdit(save?: (content: string) => Promise<void>) {
  const onSave = vi.fn(save ?? (async (_content: string) => {}))
  const onCancel = vi.fn()
  render(() => <SpaceMessageEdit initial={ORIGINAL} maxChars={4000} onSave={onSave} onCancel={onCancel} />)
  const editor = screen.getByRole('textbox', { name: 'Rediger melding' }) as HTMLTextAreaElement
  return { editor, onSave, onCancel }
}

// Signal writes apply at flush, so the keyboard handler must not run before the typed text has landed.
function typeInto(editor: HTMLTextAreaElement, value: string) {
  fireEvent.input(editor, { target: { value } })
  flush()
}

describe('SpaceMessageEdit keyboard save and cancel (WS12-S7)', () => {
  it('Ctrl+Enter saves the trimmed text, while a plain Enter only starts a new line', () => {
    const { editor, onSave, onCancel } = renderEdit()
    typeInto(editor, '  Ny tekst  ')
    fireEvent.keyDown(editor, { key: 'Enter' })
    expect(onSave).not.toHaveBeenCalled()

    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true })
    expect(onSave).toHaveBeenCalledTimes(1)
    expect(onSave).toHaveBeenCalledWith('Ny tekst')
    expect(onCancel).not.toHaveBeenCalled()
  })

  it('Cmd+Enter saves as well', () => {
    const { editor, onSave } = renderEdit()
    typeInto(editor, 'Ny tekst')
    fireEvent.keyDown(editor, { key: 'Enter', metaKey: true })
    expect(onSave).toHaveBeenCalledWith('Ny tekst')
  })

  it('Escape cancels and saves nothing', () => {
    const { editor, onSave, onCancel } = renderEdit()
    typeInto(editor, 'Ny tekst')
    fireEvent.keyDown(editor, { key: 'Escape' })
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onSave).not.toHaveBeenCalled()
  })

  it('Ctrl+Enter on unchanged text cancels instead of saving the same words again', () => {
    const { editor, onSave, onCancel } = renderEdit()
    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true })
    expect(onCancel).toHaveBeenCalledTimes(1)
    expect(onSave).not.toHaveBeenCalled()
  })

  it('an empty text cannot be saved, by the keyboard or by the Save button', () => {
    const { editor, onSave, onCancel } = renderEdit()
    typeInto(editor, '   ')
    expect((screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true })
    expect(onSave).not.toHaveBeenCalled()
    expect(onCancel).not.toHaveBeenCalled()
  })
})

describe('SpaceMessageEdit refusals (WS12-S7)', () => {
  it('a refused save says why in words, keeps the typed text and leaves the editor usable', async () => {
    const refusal = new ApiError('Forbidden: message_revise_forbidden for subject kari', 403, 'message_revise_forbidden')
    const { editor, onSave, onCancel } = renderEdit(async () => {
      throw refusal
    })
    typeInto(editor, 'Ny tekst som ikke ble lagret')
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))

    const alert = await screen.findByRole('alert')
    expect(onSave).toHaveBeenCalledWith('Ny tekst som ikke ble lagret')
    expect(alert.textContent).toContain('Du kan bare endre dine egne meldinger')
    expect(alert.textContent).not.toContain('Forbidden')
    expect(editor.value).toBe('Ny tekst som ikke ble lagret')
    await waitFor(() => expect(editor.disabled).toBe(false))
    expect((screen.getByRole('button', { name: 'Lagre' }) as HTMLButtonElement).disabled).toBe(false)
    expect(onCancel).not.toHaveBeenCalled()
    // The draft lives only in the box: a refused save writes nothing to browser storage.
    expect(window.localStorage.length).toBe(0)
  })

  it('an unknown failure shows the plain fallback words, never the raw error text', async () => {
    const { editor } = renderEdit(async () => {
      throw new Error('socket reset by peer')
    })
    typeInto(editor, 'Ny tekst')
    fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true })

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe(FALLBACK)
    expect(editor.value).toBe('Ny tekst')
  })
})
