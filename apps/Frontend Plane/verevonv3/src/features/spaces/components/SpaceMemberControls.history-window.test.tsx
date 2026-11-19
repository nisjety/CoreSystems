import { cleanup, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import { fireEvent } from '@/test/fire-event'
import { expectNoNorwegian, renderInLocale } from '@/test/locale-leak'
import { render } from '@/test/render'

// D18: the invite's "share past conversation" switch in the member picker. On by default, hidden with the gateway's
// `member_history_windows` switch off, and an add that hides the past is never assumed to have taken: Control's answer
// must say so, or the person is told it could not be confirmed.

const orgClient = vi.hoisted(() => ({ listOrganizationMembers: vi.fn() }))
const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  addSpaceMember: vi.fn(),
  removeSpaceMember: vi.fn(),
  changeSpaceMemberRole: vi.fn(),
  leaveSpaceRoom: vi.fn(),
  searchDirectoryPeople: vi.fn(),
  searchDirectory: vi.fn(),
}))

vi.mock('@/shared/api/organization-client', () => orgClient)
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const { SpaceMemberControls } = await import('./SpaceMemberControls')

const roster = [
  { subject_type: 'user' as const, subject_id: 'user_1', role: 'owner' as const, revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user' as const, subject_id: 'user_2', role: 'editor' as const, revision: 1, display_name: 'Ola Hansen' },
]

const hiddenWindow = { source: 'member', mode: 'since_join', sinceAt: null, revision: 1 }
const sharedWindow = { source: 'room', mode: 'full', sinceAt: null, revision: 0 }

type Props = {
  memberHistoryWindows?: boolean
  roomHistoryMode?: string
  directoryResolver?: boolean
  writerApi?: boolean
  onChanged?: () => void
}

function renderControls(props: Props = {}) {
  return render(() => (
    <SpaceMemberControls
      spaceRef="space_1"
      orgId="org_1"
      roster={() => roster}
      actorRole="owner"
      writerApi={props.writerApi ?? true}
      memberHistoryWindows={props.memberHistoryWindows}
      roomHistoryMode={props.roomHistoryMode}
      directoryResolver={props.directoryResolver}
      onChanged={props.onChanged}
    />
  ))
}

const openPicker = () => fireEvent.click(screen.getByRole('button', { name: /Legg til personer/ }))
const share = () => screen.getByRole('switch', { name: 'Del tidligere samtale' }) as HTMLInputElement
const addButton = async () => (await screen.findByRole('button', { name: 'Legg til' })) as HTMLButtonElement

beforeEach(() => {
  orgClient.listOrganizationMembers.mockReset().mockResolvedValue([
    { userId: 'user_1', name: 'Kari Nordmann', email: 'kari@example.com', role: 'admin', status: 'active' },
    { userId: 'user_2', name: 'Ola Hansen', email: 'ola@example.com', role: 'member', status: 'active' },
    { userId: 'user_3', name: 'Nina Berg', email: 'nina@example.com', role: 'member', status: 'active' },
  ])
  spacesClient.addSpaceMember.mockReset().mockResolvedValue({ memberCount: 3, changed: true, historyWindow: sharedWindow })
  spacesClient.searchDirectoryPeople.mockReset().mockResolvedValue({ people: [], nextCursor: null })
  spacesClient.searchDirectory.mockReset().mockResolvedValue({ results: [], nextCursor: null, presenceOnly: true })
})

afterEach(() => cleanup())

describe('the invite’s share past conversation switch (D18)', () => {
  describe('with the gateway switch off', () => {
    it.each([false, undefined])('draws no switch and sends no choice when it is %s, exactly as before', async (flag) => {
      renderControls({ memberHistoryWindows: flag })
      openPicker()
      const add = await addButton()
      expect(screen.queryByRole('switch')).toBeNull()
      expect(screen.queryByText(/Del tidligere samtale/)).toBeNull()
      expect(screen.queryByText(/Nye medlemmer/)).toBeNull()
      fireEvent.click(add)
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      // Three arguments, as the legacy add: no fourth one that could carry a choice.
      expect(spacesClient.addSpaceMember.mock.calls[0]).toEqual(['space_1', 'user_3', 'editor'])
      expect(spacesClient.addSpaceMember.mock.calls[0]).toHaveLength(3)
    })

    it('shows no confirmation or warning about hidden history after an add', async () => {
      renderControls({ memberHistoryWindows: false })
      openPicker()
      fireEvent.click(await addButton())
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalled())
      expect(screen.queryByText(/ser bare meldinger/)).toBeNull()
      expect(screen.queryByText(/bekrefte/)).toBeNull()
    })
  })

  describe('with the gateway switch on', () => {
    it('offers the switch on, with the one line that says what it means', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      await addButton()
      expect(share().checked).toBe(true)
      expect(screen.getByText('Nye medlemmer kan lese tidligere meldinger')).toBeTruthy()
      // It is a switch the person can reach by its label.
      expect(screen.getByLabelText('Del tidligere samtale')).toBe(share())
      expect(screen.queryByRole('switch', { name: /skjul|hide/i })).toBeNull()
    })

    it('describes the switch by its line, so a screen reader reads what it means, and the line follows the switch', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      await addButton()
      const describedBy = share().getAttribute('aria-describedby') as string
      expect(describedBy).toBeTruthy()
      expect(document.getElementById(describedBy)?.textContent).toBe('Nye medlemmer kan lese tidligere meldinger')
      fireEvent.click(share())
      expect(document.getElementById(describedBy)?.textContent).toBe('Nye medlemmer ser bare meldinger fra de blir med')
    })

    it('does not draw the switch while the picker is closed', () => {
      renderControls({ memberHistoryWindows: true })
      expect(screen.queryByRole('switch')).toBeNull()
    })

    it('states the choice to share on an add that left it on, beside the role', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      fireEvent.click(await addButton())
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', 'editor', { sharePastConversation: true })
    })

    it('turns the line over and states the choice to hide when the switch is turned off', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      expect(share().checked).toBe(false)
      expect(screen.getByText('Nye medlemmer ser bare meldinger fra de blir med')).toBeTruthy()
      expect(screen.queryByText('Nye medlemmer kan lese tidligere meldinger')).toBeNull()
      fireEvent.click(add)
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', 'editor', { sharePastConversation: false })
    })

    it('turning it back on states the choice to share again', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      fireEvent.click(share())
      expect(share().checked).toBe(true)
      fireEvent.click(add)
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_3', 'editor', { sharePastConversation: true })
    })

    it('puts the switch back on whenever the picker is closed, so a hide is a choice for the people added now', async () => {
      renderControls({ memberHistoryWindows: true })
      openPicker()
      await addButton()
      fireEvent.click(share())
      expect(share().checked).toBe(false)
      fireEvent.click(screen.getByRole('button', { name: 'Lukk' }))
      expect(screen.queryByRole('switch')).toBeNull()
      openPicker()
      await addButton()
      expect(share().checked).toBe(true)
    })

    it('says, when the room keeps its own history, that the room’s setting decides what a shared invite reads', async () => {
      renderControls({ memberHistoryWindows: true, roomHistoryMode: 'since_join' })
      openPicker()
      await addButton()
      expect(screen.getByText('Rommets egen innstilling avgjør hva nye medlemmer kan lese')).toBeTruthy()
      expect(screen.queryByText('Nye medlemmer kan lese tidligere meldinger')).toBeNull()
      // Turned off, the line is the same one: the person is hidden from the past either way.
      fireEvent.click(share())
      expect(screen.getByText('Nye medlemmer ser bare meldinger fra de blir med')).toBeTruthy()
    })

    it('says nothing different for a room that keeps full history', async () => {
      renderControls({ memberHistoryWindows: true, roomHistoryMode: 'full' })
      openPicker()
      await addButton()
      expect(screen.getByText('Nye medlemmer kan lese tidligere meldinger')).toBeTruthy()
    })

    it('holds the switch still while an add is in flight', async () => {
      let answer: (value: unknown) => void = () => undefined
      spacesClient.addSpaceMember.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve }))
      renderControls({ memberHistoryWindows: true })
      openPicker()
      fireEvent.click(await addButton())
      await waitFor(() => expect(share().disabled).toBe(true))
      answer({ memberCount: 3, changed: true, historyWindow: sharedWindow })
      await waitFor(() => expect(share().disabled).toBe(false))
    })

    it('sends the choice on the directory picker path too', async () => {
      spacesClient.searchDirectory.mockResolvedValue({
        results: [{ kind: 'person', subjectId: 'user_4', displayName: 'Kari Solheim', email: 'user_4@example.test', isSelf: false }],
        nextCursor: null,
        presenceOnly: true,
      })
      renderControls({ memberHistoryWindows: true, directoryResolver: true })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      fireEvent.click(add)
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      expect(spacesClient.addSpaceMember).toHaveBeenCalledWith('space_1', 'user_4', 'editor', { sharePastConversation: false })
    })
  })

  describe('an add that hides the past is not assumed to have taken', () => {
    async function addWithoutSharing(answer: unknown, props: Props = {}) {
      spacesClient.addSpaceMember.mockResolvedValueOnce(answer)
      renderControls({ memberHistoryWindows: true, ...props })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      fireEvent.click(add)
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
    }

    it('says so when Control confirmed that the person reads since they joined', async () => {
      const onChanged = vi.fn()
      await addWithoutSharing({ memberCount: 3, changed: true, historyWindow: hiddenWindow }, { onChanged })
      expect(await screen.findByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeTruthy()
      expect(screen.queryByRole('alert')).toBeNull()
      expect(onChanged).toHaveBeenCalled()
    })

    it.each([
      ['no window at all (an older gateway)', undefined],
      ['a window that shares the past', sharedWindow],
      ['a member window that is not since_join', { source: 'member', mode: 'full', sinceAt: null, revision: 2 }],
      ['a room window that happens to be since_join', { source: 'room', mode: 'since_join', sinceAt: null, revision: 0 }],
    ])('warns that it could not be confirmed when the answer carries %s, and still re-reads the roster', async (_name, window) => {
      const onChanged = vi.fn()
      await addWithoutSharing({ memberCount: 3, changed: true, historyWindow: window }, { onChanged })
      const alert = await screen.findByRole('alert')
      expect(alert.textContent).toBe(
        'Personen er lagt til, men vi kunne ikke bekrefte at tidligere meldinger er skjult for dem. Sjekk historikken deres i medlemslisten.',
      )
      expect(screen.queryByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeNull()
      // The person was added, so the roster is read again whatever the window said.
      expect(onChanged).toHaveBeenCalled()
    })

    it('makes no claim about an add that changed nothing', async () => {
      await addWithoutSharing({ memberCount: 3, changed: false })
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalled())
      expect(screen.queryByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeNull()
      expect(screen.queryByText(/bekrefte/)).toBeNull()
      expect(screen.queryByRole('alert')).toBeNull()
    })

    it('never claims anything for an add that shared the past', async () => {
      spacesClient.addSpaceMember.mockResolvedValueOnce({ memberCount: 3, changed: true, historyWindow: sharedWindow })
      renderControls({ memberHistoryWindows: true })
      openPicker()
      fireEvent.click(await addButton())
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(1))
      expect(screen.queryByText(/ser bare meldinger/)).toBeNull()
      expect(screen.queryByRole('alert')).toBeNull()
    })

    it('clears the earlier confirmation when the next add starts', async () => {
      await addWithoutSharing({ memberCount: 3, changed: true, historyWindow: hiddenWindow })
      expect(await screen.findByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeTruthy()
      let answer: (value: unknown) => void = () => undefined
      spacesClient.addSpaceMember.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve }))
      fireEvent.click(await addButton())
      await waitFor(() => expect(spacesClient.addSpaceMember).toHaveBeenCalledTimes(2))
      expect(screen.queryByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeNull()
      answer({ memberCount: 4, changed: true, historyWindow: hiddenWindow })
    })

    it('says nobody was added when Control’s switch is off, and re-reads nothing', async () => {
      const onChanged = vi.fn()
      spacesClient.addSpaceMember.mockRejectedValueOnce(new ApiError('off', 503, 'member_history_window_unavailable'))
      renderControls({ memberHistoryWindows: true, onChanged })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      fireEvent.click(add)
      expect((await screen.findByRole('alert')).textContent).toBe(
        'Å skjule tidligere meldinger for nye medlemmer er ikke slått på ennå. Ingen er lagt til.',
      )
      expect(screen.queryByText('Personen er lagt til og ser bare meldinger fra de ble med.')).toBeNull()
      expect(onChanged).not.toHaveBeenCalled()
    })

    it('words an outage of the room admin as nobody added, and any other failure as nothing changed', async () => {
      spacesClient.addSpaceMember.mockRejectedValueOnce(new ApiError('down', 503, 'room_admin_unavailable'))
      renderControls({ memberHistoryWindows: true })
      openPicker()
      const add = await addButton()
      fireEvent.click(share())
      fireEvent.click(add)
      expect((await screen.findByRole('alert')).textContent).toBe(
        'Personen kunne ikke legges til akkurat nå. Ingen er lagt til. Prøv igjen om litt.',
      )
      cleanup()
      spacesClient.addSpaceMember.mockRejectedValueOnce(new Error('network'))
      renderControls({ memberHistoryWindows: true })
      openPicker()
      const again = await addButton()
      fireEvent.click(share())
      fireEvent.click(again)
      expect((await screen.findByRole('alert')).textContent).toBe('Personen kunne ikke legges til. Ingenting er endret.')
    })

    it('uses the old failure sentence, not the history one, for a shared invite that failed', async () => {
      spacesClient.addSpaceMember.mockRejectedValueOnce(new ApiError('off', 503, 'member_history_window_unavailable'))
      renderControls({ memberHistoryWindows: true })
      openPicker()
      fireEvent.click(await addButton())
      // The person did not ask to hide anything: Control's code is worded by the shared table, and says nothing about "nobody".
      const alert = await screen.findByRole('alert')
      expect(alert.textContent).toBe('Historikk per medlem er ikke slått på ennå. Ingenting er endret.')
    })
  })

  describe('in English', () => {
    it('says every word of the switch, the confirmation and the warning in English', async () => {
      spacesClient.addSpaceMember.mockResolvedValueOnce({ memberCount: 3, changed: true, historyWindow: hiddenWindow })
      const view = renderInLocale('en', () => (
        <SpaceMemberControls
          spaceRef="space_1"
          orgId="org_1"
          roster={() => roster}
          actorRole="owner"
          writerApi
          memberHistoryWindows
        />
      ))
      fireEvent.click(screen.getByRole('button', { name: /Add people/ }))
      const add = await screen.findByRole('button', { name: 'Add' })
      expect(screen.getByRole('switch', { name: 'Share past conversation' })).toBeTruthy()
      expect(screen.getByText('New members can read earlier messages')).toBeTruthy()
      fireEvent.click(screen.getByRole('switch', { name: 'Share past conversation' }))
      expect(screen.getByText('New members only see messages from when they join')).toBeTruthy()
      fireEvent.click(add)
      expect(await screen.findByText('The person was added and only sees messages from when they joined.')).toBeTruthy()
      expectNoNorwegian(view.container)
    })

    it('says the unconfirmed warning and the refusal in English', async () => {
      spacesClient.addSpaceMember.mockResolvedValueOnce({ memberCount: 3, changed: true })
      const view = renderInLocale('en', () => (
        <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} actorRole="owner" writerApi memberHistoryWindows />
      ))
      fireEvent.click(screen.getByRole('button', { name: /Add people/ }))
      const add = await screen.findByRole('button', { name: 'Add' })
      fireEvent.click(screen.getByRole('switch', { name: 'Share past conversation' }))
      fireEvent.click(add)
      expect((await screen.findByRole('alert')).textContent).toBe(
        'The person was added, but we could not confirm that earlier messages are hidden from them. Check their history in the member list.',
      )
      expectNoNorwegian(view.container)
      cleanup()
      spacesClient.addSpaceMember.mockRejectedValueOnce(new ApiError('off', 503, 'member_history_window_unavailable'))
      renderInLocale('en', () => (
        <SpaceMemberControls spaceRef="space_1" orgId="org_1" roster={() => roster} actorRole="owner" writerApi memberHistoryWindows />
      ))
      fireEvent.click(screen.getByRole('button', { name: /Add people/ }))
      const addAgain = await screen.findByRole('button', { name: 'Add' })
      fireEvent.click(screen.getByRole('switch', { name: 'Share past conversation' }))
      fireEvent.click(addAgain)
      expect((await screen.findByRole('alert')).textContent).toBe(
        'Hiding earlier messages from new members is not turned on yet. Nobody was added.',
      )
    })
  })
})
