import { cleanup, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import { ERROR_COPY } from '@/shared/i18n/errors'
import { fireEvent } from '@/test/fire-event'
import { expectNoNorwegian, renderInLocale } from '@/test/locale-leak'
import { render } from '@/test/render'

// D18: the history control of one roster row. It reads the member's window from Control when it is opened, offers
// the three choices, sends the one chosen with the revision it read, and reads the window again when it moved. A
// window only hides; Control re-checks the role on every call, so these tests are about what is offered and sent.

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceMemberHistoryWindow: vi.fn(),
  setSpaceMemberHistoryWindow: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceMemberHistoryControl } = await import('./SpaceMemberHistoryControl')

const member = {
  subject_type: 'user' as const,
  subject_id: 'user_2',
  role: 'editor' as const,
  revision: 1,
  display_name: 'Ola Hansen',
}

type Window = { source: string; mode: string; sinceAt: string | null; revision: number }
const win = (over: Partial<Window> = {}): Window => ({ source: 'member', mode: 'since_join', sinceAt: null, revision: 3, ...over })
const readOf = (window: Window) => ({ spaceRef: 'space_1', subjectId: 'user_2', window })
const changeOf = (window: Window, changed = true) => ({ ...readOf(window), changed })

/** The local day, as `<input type="date">` writes it, `offset` days from today. */
function day(offset: number): string {
  const at = new Date()
  at.setDate(at.getDate() + offset)
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`
}

function renderControl(props: Record<string, unknown> = {}) {
  return render(() => <SpaceMemberHistoryControl spaceRef="space_1" member={member} {...(props as object)} />)
}

const toggle = () => screen.getByRole('button', { name: 'Historikk for Ola Hansen' })
const radio = (name: string) => screen.getByRole('radio', { name }) as HTMLInputElement
const dateInput = () => screen.getByLabelText('Dato for Ola Hansen') as HTMLInputElement
const save = () => screen.getByRole('button', { name: /^Lagre/ }) as HTMLButtonElement

async function open() {
  fireEvent.click(toggle())
  return screen.findByRole('group', { name: 'Hvilke meldinger kan Ola Hansen lese?' })
}

/** The window that was sent on the n-th change. */
const sentChange = (call = 0) => spacesClient.setSpaceMemberHistoryWindow.mock.calls[call] as [string, string, Record<string, unknown>]

beforeEach(() => {
  spacesClient.getSpaceMemberHistoryWindow.mockReset().mockResolvedValue(readOf(win()))
  spacesClient.setSpaceMemberHistoryWindow.mockReset().mockResolvedValue(changeOf(win({ mode: 'full', revision: 4 })))
})

afterEach(() => cleanup())

describe('SpaceMemberHistoryControl (D18)', () => {
  it('is a closed button that reads nothing until it is opened', () => {
    renderControl()
    expect(toggle().getAttribute('aria-expanded')).toBe('false')
    expect(toggle().textContent).toContain('Historikk')
    expect(screen.queryByRole('group')).toBeNull()
    expect(spacesClient.getSpaceMemberHistoryWindow).not.toHaveBeenCalled()
  })

  it('reads the member’s window from Control when opened, and shows it as the current value', async () => {
    renderControl()
    await open()
    expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1)
    expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledWith('space_1', 'user_2')
    expect(toggle().getAttribute('aria-expanded')).toBe('true')
    expect(toggle().getAttribute('aria-controls')).toBeTruthy()
    expect(screen.getByText('Nå: Siden de ble med (satt for dette medlemmet)')).toBeTruthy()
    expect(radio('Siden de ble med').checked).toBe(true)
    expect(radio('All historikk').checked).toBe(false)
    expect(radio('Fra en dato').checked).toBe(false)
  })

  it('says a member with no setting of their own follows the room, and shows the room’s mode', async () => {
    spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ source: 'room', mode: 'full', revision: 0 })))
    renderControl()
    await open()
    expect(screen.getByText('Nå: All historikk (følger rommets innstilling)')).toBeTruthy()
    expect(radio('All historikk').checked).toBe(true)
  })

  it('shows a date setting with the local day of its instant, and enables only that picker', async () => {
    const since = new Date(2026, 10, 12, 0, 0, 0).toISOString()
    spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ mode: 'since_date', sinceAt: since })))
    renderControl()
    await open()
    expect(radio('Fra en dato').checked).toBe(true)
    expect(dateInput().value).toBe('2026-11-12')
    expect(dateInput().disabled).toBe(false)
    expect(screen.getByText(/^Nå: Fra 12\.? nov\.? 2026 \(satt for dette medlemmet\)$/i)).toBeTruthy()
    // Another choice puts the picker away: a date goes only with "from a date".
    fireEvent.click(radio('All historikk'))
    expect(dateInput().disabled).toBe(true)
  })

  it('offers no future day to the picker', async () => {
    renderControl()
    await open()
    expect(dateInput().type).toBe('date')
    expect(dateInput().max).toBe(day(0))
  })

  it('describes each choice by its hint, so a screen reader reads the hint with the choice', async () => {
    renderControl()
    await open()
    const hints: Array<[string, string]> = [
      ['All historikk', 'Alt som er sagt i rommet, også før de ble med.'],
      ['Fra en dato', 'Meldinger fra starten av den dagen, i din tidssone.'],
      ['Siden de ble med', 'Bare det som sies etter at de ble med.'],
    ]
    for (const [name, hint] of hints) {
      const described = radio(name).getAttribute('aria-describedby')
      expect(described, name).toBeTruthy()
      expect(document.getElementById(described as string)?.textContent, name).toBe(hint)
    }
  })

  it('keeps Save off until the choice differs from what Control confirmed', async () => {
    renderControl()
    await open()
    expect(save().disabled).toBe(true)
    fireEvent.click(radio('All historikk'))
    expect(save().disabled).toBe(false)
    fireEvent.click(radio('Siden de ble med'))
    expect(save().disabled).toBe(true)
    // Saving nothing sends nothing, whichever way it is asked for.
    fireEvent.submit(save().closest('form') as HTMLFormElement)
    expect(spacesClient.setSpaceMemberHistoryWindow).not.toHaveBeenCalled()
  })

  describe('each choice', () => {
    it('sends full history with the revision it read, and shows what Control confirmed', async () => {
      const onChanged = vi.fn()
      renderControl({ onChanged })
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(sentChange()[0]).toBe('space_1')
      expect(sentChange()[1]).toBe('user_2')
      expect(sentChange()[2]).toMatchObject({ mode: 'full', expectedRevision: 3 })
      expect(sentChange()[2]?.sinceAt).toBeUndefined()
      expect(await screen.findByText('Lagret.')).toBeTruthy()
      expect(screen.getByText('Nå: All historikk (satt for dette medlemmet)')).toBeTruthy()
      expect(onChanged).toHaveBeenCalledWith({ source: 'member', mode: 'full', sinceAt: null, revision: 4 })
      // The next change compares against the revision just confirmed.
      expect(save().disabled).toBe(true)
    })

    it('sends since they joined, back from a full setting', async () => {
      spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ mode: 'full', revision: 6 })))
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValue(changeOf(win({ mode: 'since_join', revision: 7 })))
      renderControl()
      await open()
      fireEvent.click(radio('Siden de ble med'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(sentChange()[2]).toMatchObject({ mode: 'since_join', expectedRevision: 6 })
      expect(sentChange()[2]?.sinceAt).toBeUndefined()
      expect(await screen.findByText('Nå: Siden de ble med (satt for dette medlemmet)')).toBeTruthy()
    })

    it('sends a date as the instant its local day starts, with the revision, and shows the confirmed date', async () => {
      const picked = '2026-11-12'
      const instant = new Date(2026, 10, 12, 0, 0, 0, 0).toISOString()
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValue(changeOf(win({ mode: 'since_date', sinceAt: instant, revision: 4 })))
      renderControl()
      await open()
      fireEvent.click(radio('Fra en dato'))
      fireEvent.input(dateInput(), { target: { value: picked } })
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(sentChange()[2]).toEqual({ mode: 'since_date', sinceAt: instant, expectedRevision: 3 })
      expect(await screen.findByText(/^Nå: Fra 12\.? nov\.? 2026 \(satt for dette medlemmet\)$/i)).toBeTruthy()
      expect(dateInput().value).toBe(picked)
    })

    it('sends today, whose start has already passed', async () => {
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValue(
        changeOf(win({ mode: 'since_date', sinceAt: new Date().toISOString(), revision: 4 })),
      )
      renderControl()
      await open()
      fireEvent.click(radio('Fra en dato'))
      fireEvent.input(dateInput(), { target: { value: day(0) } })
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(sentChange()[2]?.mode).toBe('since_date')
      const sent = sentChange()[2]?.sinceAt as string
      const start = new Date(sent)
      expect([start.getHours(), start.getMinutes(), start.getSeconds()]).toEqual([0, 0, 0])
    })

    it('sends a changed day for a date setting, and nothing while the day is the same', async () => {
      const since = new Date(2026, 10, 12, 0, 0, 0).toISOString()
      spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ mode: 'since_date', sinceAt: since, revision: 2 })))
      renderControl()
      await open()
      expect(save().disabled).toBe(true)
      fireEvent.input(dateInput(), { target: { value: '2026-11-15' } })
      expect(save().disabled).toBe(false)
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(sentChange()[2]).toEqual({
        mode: 'since_date',
        sinceAt: new Date(2026, 10, 15, 0, 0, 0, 0).toISOString(),
        expectedRevision: 2,
      })
    })
  })

  describe('a date that cannot be sent', () => {
    it('refuses a day in the future before any request, and says so', async () => {
      renderControl()
      await open()
      fireEvent.click(radio('Fra en dato'))
      fireEvent.input(dateInput(), { target: { value: day(1) } })
      fireEvent.click(save())
      expect((await screen.findByRole('alert')).textContent).toBe('Velg i dag eller en tidligere dato.')
      expect(spacesClient.setSpaceMemberHistoryWindow).not.toHaveBeenCalled()
      // Fixing the day clears the sentence and sends.
      fireEvent.input(dateInput(), { target: { value: day(-1) } })
      expect(screen.queryByRole('alert')).toBeNull()
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
    })

    it('refuses a far-future day and a day that does not exist', async () => {
      renderControl()
      await open()
      fireEvent.click(radio('Fra en dato'))
      fireEvent.input(dateInput(), { target: { value: '2099-01-01' } })
      fireEvent.click(save())
      expect((await screen.findByRole('alert')).textContent).toBe('Velg i dag eller en tidligere dato.')
      expect(spacesClient.setSpaceMemberHistoryWindow).not.toHaveBeenCalled()
    })

    it('asks for a date when "from a date" is chosen with none', async () => {
      renderControl()
      await open()
      fireEvent.click(radio('Fra en dato'))
      expect(dateInput().value).toBe('')
      fireEvent.click(save())
      expect((await screen.findByRole('alert')).textContent).toBe('Velg en dato.')
      expect(spacesClient.setSpaceMemberHistoryWindow).not.toHaveBeenCalled()
    })
  })

  describe('a window that moved under the person (409)', () => {
    it('reads it again, says so after the reload, and sends the next change with the new revision', async () => {
      spacesClient.getSpaceMemberHistoryWindow
        .mockResolvedValueOnce(readOf(win({ mode: 'since_join', revision: 3 })))
        .mockResolvedValueOnce(readOf(win({ mode: 'since_date', sinceAt: new Date(2026, 10, 1, 0, 0, 0).toISOString(), revision: 5 })))
      spacesClient.setSpaceMemberHistoryWindow.mockRejectedValueOnce(
        new ApiError('moved', 409, 'room_member_changed', { details: { current_revision: 5 } }),
      )
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())

      expect((await screen.findByRole('alert')).textContent).toBe(
        'Innstillingen ble endret av noen andre. Vi har lastet den på nytt; sjekk valget og lagre igjen.',
      )
      expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2)
      // The form shows what Control holds now, not the choice made against the old window.
      expect(screen.getByText(/^Nå: Fra 1\.? nov\.? 2026 \(satt for dette medlemmet\)$/i)).toBeTruthy()
      expect(radio('Fra en dato').checked).toBe(true)
      expect(radio('All historikk').checked).toBe(false)

      // The next save compares against the revision it just read.
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValueOnce(changeOf(win({ mode: 'full', revision: 6 })))
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2))
      expect(sentChange(1)[2]).toMatchObject({ mode: 'full', expectedRevision: 5 })
      expect(await screen.findByText('Lagret.')).toBeTruthy()
      expect(screen.queryByText(/endret av noen andre/)).toBeNull()
    })

    it('says the window moved only once it has been read again, never before', async () => {
      let answerReload: (value: unknown) => void = () => undefined
      spacesClient.getSpaceMemberHistoryWindow
        .mockResolvedValueOnce(readOf(win({ mode: 'since_join', revision: 3 })))
        .mockImplementationOnce(() => new Promise((resolve) => { answerReload = resolve }))
      spacesClient.setSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('moved', 409, 'room_member_changed'))
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2))
      // The window is being read again: "we reloaded it" is not yet true, so it is not yet said.
      await new Promise((resolve) => setTimeout(resolve, 20))
      expect(screen.queryByRole('alert')).toBeNull()
      answerReload(readOf(win({ mode: 'since_date', sinceAt: new Date(2026, 10, 1, 0, 0, 0).toISOString(), revision: 5 })))
      expect((await screen.findByRole('alert')).textContent).toMatch(/endret av noen andre/)
      // By the time it is said, the form already shows what Control holds.
      expect(radio('Fra en dato').checked).toBe(true)
    })

    it('says so in English, once, after the reload', async () => {
      spacesClient.setSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('moved', 409, 'room_member_changed'))
      renderInLocale('en', () => <SpaceMemberHistoryControl spaceRef="space_1" member={member} />)
      fireEvent.click(screen.getByRole('button', { name: 'History for Ola Hansen' }))
      await screen.findByRole('group', { name: 'Which messages can Ola Hansen read?' })
      fireEvent.click(screen.getByRole('radio', { name: 'Full history' }))
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      expect((await screen.findByRole('alert')).textContent).toBe(
        'The setting was changed by someone else. We reloaded it; check the choice and save again.',
      )
      expect(screen.getAllByRole('alert')).toHaveLength(1)
    })

    it('shows the failed reload instead of a form when the window cannot be read again', async () => {
      spacesClient.getSpaceMemberHistoryWindow
        .mockResolvedValueOnce(readOf(win()))
        .mockRejectedValueOnce(new ApiError('down', 503, 'space_authority_unavailable'))
      spacesClient.setSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('moved', 409, 'room_member_changed'))
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2))
      // The window on screen was confirmed before the conflict: the conflict sentence is still said, and Save re-reads.
      expect(await screen.findByText(/endret av noen andre/)).toBeTruthy()
    })
  })

  describe('each typed refusal is worded', () => {
    async function refuse(failure: unknown) {
      spacesClient.setSpaceMemberHistoryWindow.mockRejectedValueOnce(failure)
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      return (await screen.findByRole('alert')).textContent
    }

    it('names the field of a 422 member_history_window_invalid', async () => {
      expect(
        await refuse(new ApiError('bad', 422, 'member_history_window_invalid', { details: { field: 'since_at' } })),
      ).toBe('Velg i dag eller en tidligere dato.')
      cleanup()
      expect(
        await refuse(new ApiError('bad', 422, 'member_history_window_invalid', { details: { field: 'mode' } })),
      ).toBe('Velg all historikk, fra en dato eller siden de ble med.')
    })

    it('says per-member history is not turned on, and that nothing was changed', async () => {
      expect(await refuse(new ApiError('off', 503, 'member_history_window_unavailable'))).toBe(
        'Historikk per medlem er ikke slått på ennå. Ingenting er endret.',
      )
    })

    it('words the room admin outage as nothing changed', async () => {
      expect(await refuse(new ApiError('down', 503, 'room_admin_unavailable'))).toBe(
        'Historikken kunne ikke endres akkurat nå. Ingenting er endret. Prøv igjen om litt.',
      )
    })

    it.each([
      [409, 'room_roster_derived'],
      [409, 'room_archived'],
      [403, 'room_role_forbidden'],
      [403, 'room_delegation_required'],
      [404, 'room_not_found'],
      [404, 'member_not_found'],
      [400, 'invalid_request'],
      [404, 'feature_disabled'],
      [502, 'upstream_unavailable'],
    ])('words %i %s with the shared copy, never the server’s text', async (status, code) => {
      const text = await refuse(new ApiError('server text: postgres://leak', status, code))
      expect(text).toBe(ERROR_COPY[code]?.no)
      expect(text).not.toMatch(/postgres|leak/)
    })

    it('says the change could not be made, and nothing was changed, for a code nobody worded', async () => {
      expect(await refuse(new ApiError('x', 500, 'a_code_nobody_registered'))).toBe(
        'Historikken kunne ikke endres. Ingenting er endret.',
      )
      cleanup()
      expect(await refuse(new Error('network'))).toBe('Historikken kunne ikke endres. Ingenting er endret.')
    })

    it('keeps the choice on screen after a refusal, so it can be fixed and sent again', async () => {
      await refuse(new ApiError('off', 503, 'member_history_window_unavailable'))
      expect(radio('All historikk').checked).toBe(true)
      expect(save().disabled).toBe(false)
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValueOnce(changeOf(win({ mode: 'full', revision: 4 })))
      fireEvent.click(save())
      expect(await screen.findByText('Lagret.')).toBeTruthy()
      expect(screen.queryByRole('alert')).toBeNull()
    })
  })

  describe('a window that cannot be read', () => {
    it('shows the refusal and offers a retry, and no form with a made-up value', async () => {
      spacesClient.getSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('down', 503, 'room_admin_unavailable'))
      renderControl()
      fireEvent.click(toggle())
      expect((await screen.findByRole('alert')).textContent).toBe('Historikken kunne ikke hentes akkurat nå. Prøv igjen om litt.')
      expect(screen.queryByRole('radio')).toBeNull()
      expect(screen.queryByRole('button', { name: /^Lagre/ })).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
      expect(await screen.findByRole('group', { name: 'Hvilke meldinger kan Ola Hansen lese?' })).toBeTruthy()
      expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2)
      expect(screen.queryByRole('alert')).toBeNull()
    })

    it('words a 404 for a member who is gone, and a 403 for a role that may not read it', async () => {
      spacesClient.getSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('gone', 404, 'member_not_found'))
      renderControl()
      fireEvent.click(toggle())
      expect((await screen.findByRole('alert')).textContent).toBe(ERROR_COPY.member_not_found?.no)
      cleanup()
      spacesClient.getSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('no', 403, 'room_role_forbidden'))
      renderControl()
      fireEvent.click(toggle())
      expect((await screen.findByRole('alert')).textContent).toBe(ERROR_COPY.room_role_forbidden?.no)
    })
  })

  describe('the answer of a change', () => {
    it('says an unchanged window was already set, and does not claim a save', async () => {
      spacesClient.setSpaceMemberHistoryWindow.mockResolvedValueOnce(changeOf(win({ mode: 'full', revision: 3 }), false))
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      expect(await screen.findByText('Allerede satt slik. Ingenting er endret.')).toBeTruthy()
      expect(screen.queryByText('Lagret.')).toBeNull()
    })

    it('sends once while a change is in flight, and says it is saving', async () => {
      let answer: (value: unknown) => void = () => undefined
      spacesClient.setSpaceMemberHistoryWindow.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve }))
      renderControl()
      await open()
      fireEvent.click(radio('All historikk'))
      fireEvent.click(save())
      await waitFor(() => expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1))
      expect(screen.getByRole('button', { name: 'Lagrer...' }).hasAttribute('disabled')).toBe(true)
      expect((screen.getByRole('group') as HTMLFieldSetElement).disabled).toBe(true)
      fireEvent.submit(document.querySelector('form') as HTMLFormElement)
      expect(spacesClient.setSpaceMemberHistoryWindow).toHaveBeenCalledTimes(1)
      answer(changeOf(win({ mode: 'full', revision: 4 })))
      expect(await screen.findByText('Lagret.')).toBeTruthy()
    })
  })

  describe('opening and closing', () => {
    it('reads the window again every time it is opened, so the revision is never an old one', async () => {
      renderControl()
      await open()
      fireEvent.click(toggle())
      expect(screen.queryByRole('group')).toBeNull()
      expect(toggle().getAttribute('aria-expanded')).toBe('false')
      spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ mode: 'full', revision: 9 })))
      await open()
      expect(spacesClient.getSpaceMemberHistoryWindow).toHaveBeenCalledTimes(2)
      expect(radio('All historikk').checked).toBe(true)
    })

    it('ignores a read that was answered after the panel was closed and opened again', async () => {
      let answerFirst: (value: unknown) => void = () => undefined
      spacesClient.getSpaceMemberHistoryWindow
        .mockImplementationOnce(() => new Promise((resolve) => { answerFirst = resolve }))
        .mockResolvedValueOnce(readOf(win({ mode: 'full', revision: 8 })))
      renderControl()
      fireEvent.click(toggle())
      fireEvent.click(toggle())
      fireEvent.click(toggle())
      await screen.findByRole('group', { name: 'Hvilke meldinger kan Ola Hansen lese?' })
      expect(radio('All historikk').checked).toBe(true)
      // The slow first answer arrives last and must not replace the newer one.
      answerFirst(readOf(win({ mode: 'since_join', revision: 1 })))
      await new Promise((resolve) => setTimeout(resolve, 20))
      expect(radio('All historikk').checked).toBe(true)
      expect(screen.getByText('Nå: All historikk (satt for dette medlemmet)')).toBeTruthy()
    })
  })

  describe('what the window says that this build does not know', () => {
    it('chooses nothing for a mode it does not know, and offers only the three it does', async () => {
      spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ mode: 'since_message' })))
      renderControl()
      await open()
      expect(screen.getByText('Nå: Ukjent innstilling (satt for dette medlemmet)')).toBeTruthy()
      for (const name of ['All historikk', 'Fra en dato', 'Siden de ble med']) expect(radio(name).checked).toBe(false)
      expect(screen.getAllByRole('radio')).toHaveLength(3)
      expect(save().disabled).toBe(true)
      fireEvent.click(radio('All historikk'))
      expect(save().disabled).toBe(false)
    })
  })

  describe('the row it belongs to', () => {
    it('names the member by their id when there is no display name', async () => {
      render(() => (
        <SpaceMemberHistoryControl spaceRef="space_1" member={{ ...member, display_name: '' }} />
      ))
      fireEvent.click(screen.getByRole('button', { name: 'Historikk for user_2' }))
      expect(await screen.findByRole('group', { name: 'Hvilke meldinger kan user_2 lese?' })).toBeTruthy()
    })

    it('gives every row its own radio group and label targets', async () => {
      render(() => (
        <>
          <SpaceMemberHistoryControl spaceRef="space_1" member={member} />
          <SpaceMemberHistoryControl spaceRef="space_1" member={{ ...member, subject_id: 'user_3', display_name: 'Nina Berg' }} />
        </>
      ))
      fireEvent.click(screen.getByRole('button', { name: 'Historikk for Ola Hansen' }))
      fireEvent.click(screen.getByRole('button', { name: 'Historikk for Nina Berg' }))
      await waitFor(() => expect(screen.getAllByRole('group')).toHaveLength(2))
      const names = screen.getAllByRole('radio').map((input) => (input as HTMLInputElement).name)
      expect(new Set(names).size).toBe(2)
      const ids = [...document.querySelectorAll('[id]')].map((element) => element.id)
      expect(new Set(ids).size).toBe(ids.length)
      // One row's choice does not move the other's.
      fireEvent.click(within(screen.getAllByRole('group')[0] as HTMLElement).getByRole('radio', { name: 'All historikk' }))
      expect(
        (within(screen.getAllByRole('group')[1] as HTMLElement).getByRole('radio', { name: 'All historikk' }) as HTMLInputElement).checked,
      ).toBe(false)
    })
  })

  describe('in English', () => {
    it('says every word of the control in English, with none of it Norwegian', async () => {
      const since = new Date(2026, 10, 12, 0, 0, 0).toISOString()
      spacesClient.getSpaceMemberHistoryWindow.mockResolvedValue(readOf(win({ source: 'room', mode: 'since_date', sinceAt: since })))
      const view = renderInLocale('en', () => <SpaceMemberHistoryControl spaceRef="space_1" member={member} />)
      fireEvent.click(screen.getByRole('button', { name: 'History for Ola Hansen' }))
      await screen.findByRole('group', { name: 'Which messages can Ola Hansen read?' })
      expect(screen.getByText('Now: From Nov 12, 2026 (follows the room setting)')).toBeTruthy()
      for (const label of ['Full history', 'From a date', 'Since they joined']) expect(screen.getByRole('radio', { name: label })).toBeTruthy()
      expect(screen.getByText('Everything said in the room, also before they joined.')).toBeTruthy()
      expect(screen.getByText('Messages from the start of that day, in your time zone.')).toBeTruthy()
      expect(screen.getByText('Only what is said after they joined.')).toBeTruthy()
      expect(screen.getByLabelText('Date for Ola Hansen')).toBeTruthy()
      // Refused days and refusals read in English too.
      fireEvent.input(screen.getByLabelText('Date for Ola Hansen'), { target: { value: day(1) } })
      fireEvent.click(screen.getByRole('radio', { name: 'From a date' }))
      fireEvent.input(screen.getByLabelText('Date for Ola Hansen'), { target: { value: day(1) } })
      fireEvent.click(screen.getByRole('button', { name: 'Save' }))
      expect((await screen.findByRole('alert')).textContent).toBe('Pick today or an earlier date.')
      expectNoNorwegian(view.container)
    })

    it('says a failed read and a refusal in English', async () => {
      spacesClient.getSpaceMemberHistoryWindow.mockRejectedValueOnce(new ApiError('down', 500, 'a_code_nobody_registered'))
      renderInLocale('en', () => <SpaceMemberHistoryControl spaceRef="space_1" member={member} />)
      fireEvent.click(screen.getByRole('button', { name: 'History for Ola Hansen' }))
      expect((await screen.findByRole('alert')).textContent).toBe('The history setting could not be loaded. Try again shortly.')
      expect(screen.getByRole('button', { name: 'Try again' })).toBeTruthy()
    })
  })
})
