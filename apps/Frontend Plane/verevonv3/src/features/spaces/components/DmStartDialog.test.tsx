// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import { I18nProvider } from '@/shared/i18n'

const spacesClient = vi.hoisted(() => ({
  openSpaceDm: vi.fn(),
  searchDirectoryPeople: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { DM_MAX_OTHERS, DmStartDialog } = await import('./DmStartDialog')

const people = (count: number) =>
  Array.from({ length: count }, (_, index) => ({
    subjectId: `u-${index + 1}`,
    displayName: `Person ${index + 1}`,
    email: `p${index + 1}@example.com`,
    isSelf: false,
  }))

function renderDialog(overrides: { onOpened?: () => void; onClose?: () => void } = {}) {
  const onOpened = overrides.onOpened ?? vi.fn()
  const onClose = overrides.onClose ?? vi.fn()
  render(() => (
    <I18nProvider>
      <DmStartDialog open onClose={onClose} onOpened={onOpened} />
    </I18nProvider>
  ))
  return { onOpened, onClose }
}

const checkbox = (n: number) => screen.findByRole('checkbox', { name: new RegExp(`^Person ${n}[ ]*p${n}@`) }) as Promise<HTMLInputElement>
const startButton = () => screen.getByRole('button', { name: /Start melding|Start message/ }) as HTMLButtonElement

beforeEach(() => {
  HTMLDialogElement.prototype.showModal = function showModal() {
    this.setAttribute('open', '')
  }
  HTMLDialogElement.prototype.close = function close() {
    this.removeAttribute('open')
  }
  spacesClient.openSpaceDm.mockReset()
  spacesClient.searchDirectoryPeople.mockReset()
  spacesClient.searchDirectoryPeople.mockResolvedValue({ people: [...people(10), { subjectId: 'me', displayName: 'Me', email: 'me@example.com', isSelf: true }], nextCursor: null })
})

afterEach(cleanup)

describe('DmStartDialog (WS12-S3)', () => {
  it('lists the found people without the caller and starts with Start disabled', async () => {
    renderDialog()
    expect(await checkbox(1)).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Me\b/ })).toBeNull()
    expect(startButton().disabled).toBe(true)
  })

  it('picks one person, opens the DM with that subject id and hands the result on', async () => {
    const result = { spaceRef: 'space-dm-1', kind: 'dm', lifecycle: 'pending_registration', created: true, participants: [] }
    spacesClient.openSpaceDm.mockResolvedValue(result)
    const { onOpened } = renderDialog()
    fireEvent.click(await checkbox(2))
    flush()
    await waitFor(() => expect(startButton().disabled).toBe(false))
    fireEvent.submit(startButton().closest('form') as HTMLFormElement)
    await waitFor(() => expect(spacesClient.openSpaceDm).toHaveBeenCalledWith(['u-2']))
    await waitFor(() => expect(onOpened).toHaveBeenCalledWith(result))
  })

  it('picks several people in the order chosen and removes one from the chips', async () => {
    spacesClient.openSpaceDm.mockResolvedValue({ spaceRef: 's', kind: 'group_dm', lifecycle: 'active', created: true, participants: [] })
    renderDialog()
    fireEvent.click(await checkbox(3))
    flush()
    fireEvent.click(await checkbox(1))
    flush()
    fireEvent.click(await checkbox(2))
    flush()
    fireEvent.click(await screen.findByRole('button', { name: /Fjern Person 1|Remove Person 1/ }))
    flush()
    await waitFor(() => expect(startButton().disabled).toBe(false))
    fireEvent.submit(startButton().closest('form') as HTMLFormElement)
    await waitFor(() => expect(spacesClient.openSpaceDm).toHaveBeenCalledWith(['u-3', 'u-2']))
  })

  it('stops at eight people: the rest are disabled and the limit is said', async () => {
    expect(DM_MAX_OTHERS).toBe(8)
    renderDialog()
    for (let index = 1; index <= DM_MAX_OTHERS; index += 1) {
      fireEvent.click(await checkbox(index))
      flush()
    }
    await waitFor(async () => expect((await checkbox(9)).disabled).toBe(true))
    expect((await checkbox(10)).disabled).toBe(true)
    expect(screen.getByText(/høyeste antallet|most people allowed/)).toBeTruthy()
    // A picked person stays enabled so it can be removed again.
    expect((await checkbox(1)).disabled).toBe(false)
  })

  it('shows a typed error from the open, keeps the dialog and does not hand anything on', async () => {
    spacesClient.openSpaceDm.mockRejectedValue(
      new ApiError('Direct messages are not enabled for this organization.', 403, 'dm_not_entitled'),
    )
    const { onOpened } = renderDialog()
    fireEvent.click(await checkbox(1))
    flush()
    await waitFor(() => expect(startButton().disabled).toBe(false))
    fireEvent.submit(startButton().closest('form') as HTMLFormElement)
    const alert = await screen.findByRole('alert')
    expect(alert.textContent?.length).toBeGreaterThan(0)
    expect(onOpened).not.toHaveBeenCalled()
    // The person can try again: Start is enabled once more.
    await waitFor(() => expect(startButton().disabled).toBe(false))
  })

  it('says so when the people search fails', async () => {
    spacesClient.searchDirectoryPeople.mockRejectedValue(new Error('down'))
    renderDialog()
    expect((await screen.findByRole('alert')).textContent).toMatch(/Personsøket mislyktes|people search failed/)
  })

  it('Cancel closes without opening anything', async () => {
    const { onClose } = renderDialog()
    await checkbox(1)
    fireEvent.click(screen.getByRole('button', { name: /Avbryt|Cancel/ }))
    expect(onClose).toHaveBeenCalled()
    expect(spacesClient.openSpaceDm).not.toHaveBeenCalled()
  })
})
