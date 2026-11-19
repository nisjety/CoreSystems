import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  searchDirectory: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)

const { DirectoryPicker } = await import('./DirectoryPicker')

const person = (id: string, name: string) => ({
  kind: 'person' as const,
  subjectId: id,
  displayName: name,
  email: `${id}@example.test`,
  isSelf: false,
})
const agent = (ref: string, name: string) => ({
  kind: 'agent' as const,
  agentRef: ref,
  displayName: name,
  description: 'Svarer på driftsspørsmål',
  installedIn: ['room_a'],
})
const page = (results: readonly unknown[], nextCursor: string | null = null) => ({
  results,
  nextCursor,
  presenceOnly: true as const,
})

type PickerProps = Parameters<typeof DirectoryPicker>[0]

function renderPicker(overrides: Partial<PickerProps> = {}) {
  const onPickPerson = vi.fn()
  render(() => (
    <DirectoryPicker
      excludeMemberOf="space_1"
      pickLabel="Legg til"
      pickingLabel="Legger til …"
      onPickPerson={onPickPerson}
      {...overrides}
    />
  ))
  return { onPickPerson }
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
const box = () => screen.getByRole('searchbox') as HTMLInputElement
const type = (value: string) => fireEvent.input(box(), { target: { value } })
const searchCalls = () => spacesClient.searchDirectory.mock.calls as Array<[string, Record<string, unknown>]>

beforeEach(() => {
  spacesClient.searchDirectory.mockReset()
  spacesClient.searchDirectory.mockResolvedValue(page([]))
})

afterEach(() => cleanup())

describe('DirectoryPicker (WS04-S9)', () => {
  it('searches for the empty text first, a page of 20, with the room left out', async () => {
    renderPicker()
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    expect(searchCalls()[0]?.[0]).toBe('')
    expect(searchCalls()[0]?.[1]).toMatchObject({ limit: 20, cursor: null, excludeMemberOf: 'space_1' })
  })

  it('passes the kinds the caller asked for, and names the search box by its label', async () => {
    renderPicker({ kinds: ['person'], label: 'Velg en person' })
    expect(screen.getByText('Velg en person')).toBeTruthy()
    await waitFor(() => expect(searchCalls()[0]?.[1]).toMatchObject({ kinds: ['person'] }))
  })

  it('sends one request per burst of typing, after the box has been still for 250 ms', async () => {
    renderPicker()
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    type('k')
    await sleep(100)
    type('ka')
    await sleep(100)
    type('kar')
    // 200 ms since the first keystroke and 0 since the last: still only the first search.
    expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(2))
    await sleep(400)
    expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(2)
    expect(searchCalls()[1]?.[0]).toBe('kar')
  })

  it('sends the typed text trimmed', async () => {
    renderPicker()
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    type('  nina  ')
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(2))
    expect(searchCalls()[1]?.[0]).toBe('nina')
  })

  it('drops a slow answer that arrives after a newer search was made', async () => {
    let answerFirst: (value: unknown) => void = () => undefined
    spacesClient.searchDirectory
      .mockImplementationOnce(() => new Promise((resolve) => { answerFirst = resolve }))
      .mockResolvedValueOnce(page([person('user_4', 'Kari Solheim')]))
    renderPicker()
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    type('kari')
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()

    answerFirst(page([person('user_3', 'Nina Berg')]))
    await sleep(50)
    expect(screen.queryByText('Nina Berg')).toBeNull()
    expect(screen.getByText('Kari Solheim')).toBeTruthy()
  })

  it('gives a person row a pick action, and a pick hands that person to the caller', async () => {
    spacesClient.searchDirectory.mockResolvedValue(page([person('user_3', 'Nina Berg')]))
    const { onPickPerson } = renderPicker()
    fireEvent.click(await screen.findByRole('button', { name: 'Legg til' }))
    expect(onPickPerson).toHaveBeenCalledWith(expect.objectContaining({ subjectId: 'user_3', displayName: 'Nina Berg' }))
  })

  it('labels an agent "Agent, not a person" and offers it no action at all', async () => {
    spacesClient.searchDirectory.mockResolvedValue(page([agent('agent-ops', 'Driftsassistent')]))
    const { onPickPerson } = renderPicker()
    expect(await screen.findByText('Driftsassistent')).toBeTruthy()
    expect(screen.getByText('Agent, ikke en person')).toBeTruthy()
    expect(screen.getByText('I 1 av rommene dine')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Legg til' })).toBeNull()
    expect(onPickPerson).not.toHaveBeenCalled()
  })

  it('hides people the caller says are already in the room, and never hides an agent', async () => {
    spacesClient.searchDirectory.mockResolvedValue(
      page([person('user_2', 'Ola Hansen'), person('user_4', 'Kari Solheim'), agent('agent-ops', 'Driftsassistent')]),
    )
    renderPicker({ isExcluded: (subjectId) => subjectId === 'user_2' })
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()
    expect(screen.queryByText('Ola Hansen')).toBeNull()
    expect(screen.getByText('Driftsassistent')).toBeTruthy()
  })

  it('says there is no one else to show when everyone found is already in the room', async () => {
    spacesClient.searchDirectory.mockResolvedValue(page([person('user_2', 'Ola Hansen')]))
    renderPicker({ isExcluded: () => true })
    expect(await screen.findByText('Ingen flere å vise.')).toBeTruthy()
    expect(screen.queryByText('Ola Hansen')).toBeNull()
  })

  it('says a search that matches nobody is no match, not an empty directory', async () => {
    renderPicker()
    expect(await screen.findByText('Ingen flere å vise.')).toBeTruthy()
    type('zzz')
    expect(await screen.findByText('Ingen treff på søket.')).toBeTruthy()
  })

  it('appends the next page on Show more, keeps the list, and drops the button at the end', async () => {
    spacesClient.searchDirectory
      .mockResolvedValueOnce(page([person('user_2', 'Ola Hansen')], 'cursor-2'))
      .mockResolvedValueOnce(page([person('user_4', 'Kari Solheim')], null))
    renderPicker()
    expect(await screen.findByText('Ola Hansen')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Vis flere' }))
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()
    expect(screen.getByText('Ola Hansen')).toBeTruthy()
    expect(searchCalls()[1]?.[1]).toMatchObject({ cursor: 'cursor-2', limit: 20, excludeMemberOf: 'space_1' })
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('shows a failed search as a failure, never as "no one to show"', async () => {
    spacesClient.searchDirectory.mockRejectedValue(new Error('down'))
    renderPicker()
    expect((await screen.findByRole('alert')).textContent).toBe('Søket mislyktes. Prøv igjen om litt.')
    expect(screen.queryByText('Ingen flere å vise.')).toBeNull()
  })

  it('shows a typed directory error with its own sentence', async () => {
    spacesClient.searchDirectory.mockRejectedValue(new ApiError('x', 503, 'directory_unavailable'))
    renderPicker()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Katalogen er utilgjengelig akkurat nå. Prøv igjen om litt.',
    )
  })

  it('disables every pick while one is in flight, and says the busy one is being added', async () => {
    spacesClient.searchDirectory.mockResolvedValue(
      page([person('user_3', 'Nina Berg'), person('user_4', 'Kari Solheim')]),
    )
    renderPicker({ busyId: 'user_3' })
    expect(await screen.findByText('Kari Solheim')).toBeTruthy()
    const busy = screen.getByRole('button', { name: 'Legger til …' }) as HTMLButtonElement
    expect(busy.disabled).toBe(true)
    const others = screen.getAllByRole('button', { name: /Legg til/ }) as HTMLButtonElement[]
    expect(others.every((button) => button.disabled)).toBe(true)
  })

  // jsdom has no implicit submission, so this does what a browser does with a key that nothing took: Enter in a text
  // field presses the form's default button (its first submit button) unless that button is disabled.
  function pressEnterInBrowser(input: HTMLElement): void {
    const taken = !fireEvent.keyDown(input, { key: 'Enter' })
    if (taken) return
    const button = input.closest('form')?.querySelector<HTMLButtonElement>('button[type="submit"]')
    if (button && !button.disabled) button.click()
  }

  it('does not let Enter in the search box press the default button of the form around it', async () => {
    const submitted = vi.fn()
    render(() => (
      <form
        onSubmit={(event) => {
          event.preventDefault()
          submitted()
        }}
      >
        <DirectoryPicker pickLabel="Legg til" pickingLabel="Legger til …" onPickPerson={vi.fn()} />
        <button type="submit">Lagre</button>
      </form>
    ))
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    type('kari')

    // The key is taken: the browser has no default action left to run, so nothing presses the default button.
    expect(fireEvent.keyDown(box(), { key: 'Enter' })).toBe(false)
    pressEnterInBrowser(box())
    expect(submitted).not.toHaveBeenCalled()

    // The button still works as a button: this is about the key, not about the form.
    fireEvent.click(screen.getByRole('button', { name: 'Lagre' }))
    expect(submitted).toHaveBeenCalledTimes(1)
  })

  it('leaves every other key to the box, so typing is not taken', async () => {
    renderPicker()
    await waitFor(() => expect(spacesClient.searchDirectory).toHaveBeenCalledTimes(1))
    for (const key of ['a', ' ', 'Backspace', 'ArrowDown', 'Tab', 'Escape']) {
      expect(fireEvent.keyDown(box(), { key }), key).toBe(true)
    }
  })
})
