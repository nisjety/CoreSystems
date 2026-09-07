import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { SpaceConfirmButton } from './SpaceConfirmButton'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

function renderButton(onConfirm = vi.fn()) {
  render(() => (
    <SpaceConfirmButton
      label="Fjern"
      confirmLabel="Bekreft at Ola fjernes"
      consequence="De mister tilgang."
      onConfirm={onConfirm}
    />
  ))
  return onConfirm
}

describe('SpaceConfirmButton', () => {
  it('does nothing on the first press, and states the consequence', async () => {
    const onConfirm = renderButton()
    screen.getByRole('button', { name: 'Fjern' }).click()

    expect(onConfirm).not.toHaveBeenCalled()
    expect(await screen.findByText('De mister tilgang.')).toBeTruthy()
    // The label becomes the confirmation, so the same control cannot be
    // mistaken for the resting one.
    expect(screen.getByRole('button', { name: 'Bekreft at Ola fjernes' })).toBeTruthy()
  })

  it('acts on the second press', async () => {
    const onConfirm = renderButton()
    screen.getByRole('button', { name: 'Fjern' }).click()
    ;(await screen.findByRole('button', { name: 'Bekreft at Ola fjernes' })).click()

    expect(onConfirm).toHaveBeenCalledTimes(1)
    // And it goes back to resting, so a third press cannot act again.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Fjern' })).toBeTruthy())
  })

  // An armed button that stays armed is a trap: the reader who walks away and
  // comes back must not find a one-click destructive action.
  it('disarms itself after a pause', async () => {
    vi.useFakeTimers()
    const onConfirm = vi.fn()
    render(() => (
      <SpaceConfirmButton
        label="Fjern"
        confirmLabel="Bekreft"
        consequence="De mister tilgang."
        onConfirm={onConfirm}
      />
    ))
    screen.getByRole('button', { name: 'Fjern' }).click()
    await vi.advanceTimersByTimeAsync(7_000)

    expect(screen.getByRole('button', { name: 'Fjern' })).toBeTruthy()
    expect(screen.queryByText('De mister tilgang.')).toBeNull()
    expect(onConfirm).not.toHaveBeenCalled()
  })

  it('does nothing at all while disabled', () => {
    const onConfirm = vi.fn()
    render(() => (
      <SpaceConfirmButton
        label="Fjern"
        confirmLabel="Bekreft"
        consequence="De mister tilgang."
        disabled
        onConfirm={onConfirm}
      />
    ))
    const button = screen.getByRole('button', { name: 'Fjern' })
    button.click()
    expect(onConfirm).not.toHaveBeenCalled()
    expect(screen.queryByText('De mister tilgang.')).toBeNull()
  })
})
