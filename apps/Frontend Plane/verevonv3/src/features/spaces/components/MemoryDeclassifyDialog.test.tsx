import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// W3-FIX-13, WS17-S7 and WS17-S7b: the memory share dialog as the SPA runs it. The room list, the roster, the session and
// the action call are mocked, so no request leaves the test. The writes are asserted at `executeAction`, the one call that
// carries them, under the action ids the registry names: `memory.declassify_to_space` (share) and
// `memory.retract_from_space` (take back). The copy is the Norwegian default, as in the other space page tests.

const actions = vi.hoisted(() => ({ executeAction: vi.fn() }))
const spacesClient = vi.hoisted(() => ({ listSpaces: vi.fn(), getSpaceRoster: vi.fn() }))
const sessionStore = vi.hoisted(() => ({
  getSession: vi.fn(() => ({ user: { id: 'user-kari' }, activeOrg: { id: 'org-1' } })),
}))

vi.mock('@/shared/actions/action-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/actions/action-client')>()),
  ...actions,
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/shared/session/session-store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/session/session-store')>()),
  ...sessionStore,
}))

const { ApiError } = await import('@/shared/api/api-error')
const { getActionDescriptor } = await import('@/shared/actions/action-registry')
const { MemoryDeclassifyDialog } = await import('./MemoryDeclassifyDialog')

const MEMORY_ID = 'mem-1'
const MEMORY_TEXT = 'Vi leverer til Bergen på fredager.'
const ACTOR = { type: 'human', userId: 'user-kari', orgId: 'org-1' }
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

// Mixed on purpose: only Room A and Room B are rooms a memory may go to.
const SPACES = [
  { space_ref: 'personal-kari', name: 'Kari', kind: 'personal', lifecycle: 'active' },
  { space_ref: 'dm-kari-ola', name: 'Ola', kind: 'dm', lifecycle: 'active' },
  { space_ref: 'group-dm-1', name: 'Kari, Ola and Assistant', kind: 'group_dm', lifecycle: 'active' },
  { space_ref: 'room-old', name: 'Room Old', kind: 'room', lifecycle: 'active', archived: true },
  { space_ref: 'room-pending', name: 'Room Pending', kind: 'room', lifecycle: 'pending_registration' },
  { space_ref: 'room-b', name: 'Room B', kind: 'room', lifecycle: 'active' },
  { space_ref: 'room-a', name: 'Room A', kind: 'room', lifecycle: 'active' },
]

const ROSTER = [
  { subject_type: 'user', subject_id: 'user-kari', role: 'owner', revision: 1, display_name: 'Kari' },
  { subject_type: 'user', subject_id: 'user-ola', role: 'editor', revision: 1, display_name: 'Ola' },
  { subject_type: 'service', subject_id: 'assistant-1', role: 'editor', revision: 1, display_name: 'Assistant' },
]

/** The room's answer to a share, as `memory.declassify_to_space` returns it. */
const shareOutcome = (over: Record<string, unknown> = {}) => ({
  actionId: 'memory.declassify_to_space',
  runId: 'run-share',
  status: 'completed',
  auditId: 'audit-share',
  eventStream: '',
  result: { room_memory_id: 'room-copy-1', space_ref: 'room-a', source_memory_id: MEMORY_ID, deduplicated: false, ...over },
})

/** The room's answer to a take-back, as `memory.retract_from_space` returns it. */
const takeBackOutcome = (over: Record<string, unknown> = {}) => ({
  actionId: 'memory.retract_from_space',
  runId: 'run-take-back',
  status: 'completed',
  auditId: 'audit-take-back',
  eventStream: '',
  result: { memory_id: 'room-copy-1', space_ref: 'room-a', deleted: true, ...over },
})

const textarea = () => screen.getByLabelText('Minnet du deler') as HTMLTextAreaElement
const picker = () => screen.getByLabelText('Rommet') as HTMLSelectElement
const shareButton = () => screen.getByRole('button', { name: 'Del i rommet' }) as HTMLButtonElement
const takeBackButton = () => screen.queryByRole('button', { name: 'Ta tilbake fra rommet' }) as HTMLButtonElement | null

/** One `executeAction` call as the dialog sent it: the action id, the actor, the input and the options. */
type ExecuteCall = [
  actionId: string,
  actor: unknown,
  input: { clientRequestId: string } & Record<string, unknown>,
  options: unknown,
]

const executeCalls = (): ExecuteCall[] => actions.executeAction.mock.calls as ExecuteCall[]

/** The n-th `executeAction` call. A call that was never made fails the test here, not later on an undefined read. */
function callAt(index: number): ExecuteCall {
  const found = executeCalls()[index]
  if (found === undefined) throw new Error(`executeAction call ${index} was never made`)
  return found
}

function renderDialog(options: { onClose?: () => void; onShared?: () => void } = {}) {
  return render(() => (
    <MemoryDeclassifyDialog
      memoryId={MEMORY_ID}
      initialText={MEMORY_TEXT}
      onClose={options.onClose ?? (() => {})}
      onShared={options.onShared}
    />
  ))
}

/** Waits for the room list, then picks the room by its name, as the person does in the picker. */
async function pickRoom(name: string): Promise<void> {
  const option = (await screen.findByRole('option', { name })) as HTMLOptionElement
  fireEvent.change(picker(), { target: { value: option.value } })
  flush()
}

/** Replaces the memory text, as the person does in the text box. */
function typeText(text: string): void {
  fireEvent.input(textarea(), { target: { value: text } })
  flush()
}

/** Whether the action's own input contract (the server mirror, read from the registry) accepts this reviewed text. */
function actionAccepts(reviewedContent: string): boolean | undefined {
  return getActionDescriptor('memory.declassify_to_space')?.inputSchema.safeParse({
    memoryId: MEMORY_ID,
    spaceRef: 'room-a',
    reviewedContent,
    clientRequestId: 'request-1',
  }).success
}

/** Presses Share and waits until the attempt has settled, when the button reads "Del i rommet" again. */
async function share(): Promise<void> {
  fireEvent.click(shareButton())
  flush()
  await waitFor(() => {
    flush()
    expect(screen.queryByRole('button', { name: 'Deler …' })).toBeNull()
  })
}

/** Arms the take-back control, then confirms it, as the person does. The caller waits for the room's answer. */
function takeBack(): void {
  fireEvent.click(screen.getByRole('button', { name: 'Ta tilbake fra rommet' }))
  flush()
  fireEvent.click(screen.getByRole('button', { name: 'Bekreft at minnet fjernes fra rommet' }))
  flush()
}

/** Opens the dialog, picks Room A and shares the memory once, so the dialog is left on the copy. */
async function shareToRoomA(outcome = shareOutcome()): Promise<void> {
  actions.executeAction.mockResolvedValueOnce(outcome)
  renderDialog()
  await pickRoom('Room A')
  await share()
  await screen.findByRole('heading', { name: 'Delt med rommet' })
}

beforeEach(() => {
  actions.executeAction.mockReset()
  spacesClient.listSpaces.mockReset().mockResolvedValue(SPACES)
  spacesClient.getSpaceRoster.mockReset().mockResolvedValue(ROSTER)
})

afterEach(() => {
  cleanup()
})

describe('W3-FIX-13: the share dialog', () => {
  it('sends nothing on open; the picker lists only shared, active, unarchived rooms', async () => {
    renderDialog()
    await screen.findByRole('option', { name: 'Room A' })
    expect(within(picker()).getAllByRole('option').map((option) => option.textContent)).toEqual([
      'Velg et rom …',
      'Room A',
      'Room B',
    ])
    expect(actions.executeAction).not.toHaveBeenCalled()
    expect(spacesClient.getSpaceRoster).not.toHaveBeenCalled()
    expect(shareButton().disabled).toBe(true)
    expect(localStorage.length).toBe(0)
  })

  it('shares the trimmed text to the picked room with the memory id, the room, the text and one request key, and nothing else', async () => {
    actions.executeAction.mockResolvedValueOnce(shareOutcome())
    renderDialog()
    await pickRoom('Room A')
    typeText(`  ${MEMORY_TEXT}  `)
    await share()
    await screen.findByRole('heading', { name: 'Delt med rommet' })

    expect(actions.executeAction).toHaveBeenCalledTimes(1)
    const [actionId, actor, input, options] = callAt(0)
    expect(actionId).toBe('memory.declassify_to_space')
    expect(actor).toEqual(ACTOR)
    expect(input).toEqual({
      memoryId: MEMORY_ID,
      spaceRef: 'room-a',
      reviewedContent: MEMORY_TEXT,
      clientRequestId: expect.any(String),
    })
    expect(input.clientRequestId).toMatch(UUID)
    expect(options).toEqual({ idempotencyKey: input.clientRequestId })
  })

  it('keeps share off for blank text and for more than 1000 code points after trimming; exactly 1000 is allowed', async () => {
    renderDialog()
    await pickRoom('Room A')

    typeText('   ')
    expect(shareButton().disabled).toBe(true)

    typeText(`  ${'a'.repeat(1000)}  `)
    expect(shareButton().disabled).toBe(false)
    expect(screen.getByText('0 tegn igjen')).toBeTruthy()

    typeText('a'.repeat(1001))
    expect(shareButton().disabled).toBe(true)
    expect(textarea().getAttribute('aria-invalid')).toBe('true')
    expect(screen.getByText('For lang for rommet. Forkort teksten før du deler den.')).toBeTruthy()
    expect(actions.executeAction).not.toHaveBeenCalled()

    // The dialog's limit is the action's own: the registry refuses the same 1001 characters the dialog refuses.
    expect(actionAccepts('a'.repeat(1000))).toBe(true)
    expect(actionAccepts('a'.repeat(1001))).toBe(false)
  })

  it('counts code points as the server does: 1000 astral characters fit, 1001 do not', async () => {
    renderDialog()
    await pickRoom('Room A')

    // Each character is one code point and two UTF-16 units: 1000 of them are 2000 units, and still allowed.
    typeText('\u{1F600}'.repeat(1000))
    expect(shareButton().disabled).toBe(false)
    expect(actionAccepts('\u{1F600}'.repeat(1000))).toBe(true)
    typeText('\u{1F600}'.repeat(1001))
    expect(shareButton().disabled).toBe(true)
    expect(actionAccepts('\u{1F600}'.repeat(1001))).toBe(false)
  })

  it('reuses the request id for the same text to the same room, and mints a new one for changed text or a changed room', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('unavailable', 503, 'space_decision_unavailable'))
    renderDialog()
    await pickRoom('Room A')
    await share()
    await share()
    typeText(`${MEMORY_TEXT} Og fredag.`)
    await share()
    await pickRoom('Room B')
    await share()

    const ids = executeCalls().map((call) => call[2].clientRequestId)
    expect(ids).toHaveLength(4)
    expect(ids[1]).toBe(ids[0])
    expect(ids[2]).not.toBe(ids[1])
    expect(ids[3]).not.toBe(ids[2])
    executeCalls().forEach((call) => {
      expect(call[3]).toEqual({ idempotencyKey: call[2].clientRequestId })
    })
  })

  it('disables share after a typed 412, and a later edit or pick does not re-arm it', async () => {
    actions.executeAction.mockRejectedValueOnce(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
    renderDialog()
    await pickRoom('Room A')
    await share()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Legitimasjon med Zero Data Retention kan ikke opprette, endre eller slette varig data.',
    )
    expect(shareButton().disabled).toBe(true)

    await pickRoom('Room B')
    typeText('En ny tekst.')
    fireEvent.click(shareButton())
    flush()
    expect(shareButton().disabled).toBe(true)
    expect(actions.executeAction).toHaveBeenCalledTimes(1)
  })

  it('keeps the text and shows the memory copy on a 403, even when the refusal carries a Space reason', async () => {
    actions.executeAction.mockRejectedValueOnce(
      new ApiError('no', 403, 'memory_declassify_not_authorized', { details: { reason: 'space_scope' } }),
    )
    renderDialog()
    await pickRoom('Room A')
    typeText('Vi leverer til Bergen.')
    await share()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Du har ikke tilgang til å dele dette minnet i rommet. Ingenting er delt.',
    )
    expect(textarea().value).toBe('Vi leverer til Bergen.')
  })

  it('shows the share sentence for a 503 space_decision_unavailable, not the posting copy, and keeps the text', async () => {
    actions.executeAction.mockRejectedValueOnce(new ApiError('unavailable', 503, 'space_decision_unavailable'))
    renderDialog()
    await pickRoom('Room A')
    typeText('Vi leverer til Bergen.')
    await share()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Rommet kunne ikke bekrefte tilgangen din akkurat nå. Ingenting er delt; prøv igjen om litt.',
    )
    expect(textarea().value).toBe('Vi leverer til Bergen.')
  })

  it('keeps the dialog open on success, names the room, and offers to take the copy back', async () => {
    const onClose = vi.fn()
    const onShared = vi.fn()
    actions.executeAction.mockResolvedValueOnce(shareOutcome())
    renderDialog({ onClose, onShared })
    await pickRoom('Room A')
    await share()
    await screen.findByRole('heading', { name: 'Delt med rommet' })
    expect(screen.getByText('Rom: Room A')).toBeTruthy()
    expect(takeBackButton()).not.toBeNull()
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(onShared).toHaveBeenCalledTimes(1)
    expect(onClose).not.toHaveBeenCalled()
  })

  it('takes the copy back with the copy\'s room id, and the same request id when the same take-back is retried', async () => {
    await shareToRoomA()
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('boom', 500, 'boom_unknown'))
      .mockResolvedValueOnce(takeBackOutcome())
    takeBack()
    await screen.findByText('Minnet ble ikke tatt tilbake. Prøv igjen.')
    takeBack()
    await screen.findByText('Minnet er tatt tilbake fra rommet.')

    const retracts = executeCalls().filter((call) => call[0] === 'memory.retract_from_space')
    expect(retracts).toHaveLength(2)
    const [first, second] = retracts as [ExecuteCall, ExecuteCall]
    expect(first[1]).toEqual(ACTOR)
    expect(first[2]).toEqual({ memoryId: 'room-copy-1', spaceRef: 'room-a', clientRequestId: expect.any(String) })
    expect(second[2]).toEqual(first[2])
    expect(first[3]).toEqual({ idempotencyKey: first[2].clientRequestId })
  })

  it('shows the copy as taken back when the room answers memory_not_found', async () => {
    await shareToRoomA()
    actions.executeAction.mockRejectedValueOnce(new ApiError('gone', 404, 'memory_not_found'))
    takeBack()
    await screen.findByText('Minnet er tatt tilbake fra rommet.')
    expect(takeBackButton()).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows the failure copy, and keeps the take-back control, when the room answers deleted false', async () => {
    await shareToRoomA()
    actions.executeAction.mockResolvedValueOnce(takeBackOutcome({ deleted: false }))
    takeBack()
    expect((await screen.findByRole('alert')).textContent).toBe('Minnet ble ikke tatt tilbake. Prøv igjen.')
    expect(screen.queryByText('Minnet er tatt tilbake fra rommet.')).toBeNull()
    expect(takeBackButton()).not.toBeNull()
  })
})

describe('WS17-S7: sharing a memory to a room', () => {
  it('sends nothing on open: no share, no roster read, and nothing kept on the device', async () => {
    renderDialog()
    await screen.findByRole('option', { name: 'Room A' })
    expect(actions.executeAction).not.toHaveBeenCalled()
    expect(spacesClient.getSpaceRoster).not.toHaveBeenCalled()
    expect(localStorage.length).toBe(0)
  })

  it('keeps confirm off for blank text and for 1001 code points, and on for a short text', async () => {
    renderDialog()
    await pickRoom('Room A')
    typeText('   ')
    expect(shareButton().disabled).toBe(true)
    typeText('a'.repeat(1001))
    expect(shareButton().disabled).toBe(true)
    typeText('Kort tekst.')
    expect(shareButton().disabled).toBe(false)
    expect(actions.executeAction).not.toHaveBeenCalled()
  })

  it('reuses clientRequestId when the same text is sent to the same room again', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('unavailable', 503, 'space_decision_unavailable'))
    renderDialog()
    await pickRoom('Room A')
    await share()
    await share()
    const [first, second] = executeCalls().map((call) => call[2].clientRequestId)
    expect(first).toMatch(UUID)
    expect(second).toBe(first)
    expect(callAt(1)[3]).toEqual({ idempotencyKey: first })
  })

  it('mints a new clientRequestId when the text changes', async () => {
    actions.executeAction.mockRejectedValue(new ApiError('unavailable', 503, 'space_decision_unavailable'))
    renderDialog()
    await pickRoom('Room A')
    await share()
    typeText('Vi leverer til Bergen mandag.')
    await share()
    const [first, second] = executeCalls().map((call) => call[2].clientRequestId)
    expect(second).not.toBe(first)
  })

  it('keeps the text and shows the memory copy on a 403', async () => {
    actions.executeAction.mockRejectedValueOnce(new ApiError('no', 403, 'memory_declassify_not_authorized'))
    renderDialog()
    await pickRoom('Room A')
    typeText('Vi leverer til Bergen.')
    await share()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Du har ikke tilgang til å dele dette minnet i rommet. Ingenting er delt.',
    )
    expect(textarea().value).toBe('Vi leverer til Bergen.')
  })

  it('disables confirm after a 412', async () => {
    actions.executeAction.mockRejectedValueOnce(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
    renderDialog()
    await pickRoom('Room A')
    await share()
    await screen.findByRole('alert')
    expect(shareButton().disabled).toBe(true)
  })

  it('keeps the text on a 503, and lets the person try again', async () => {
    actions.executeAction
      .mockRejectedValueOnce(new ApiError('unavailable', 503, 'space_decision_unavailable'))
      .mockResolvedValueOnce(shareOutcome())
    renderDialog()
    await pickRoom('Room A')
    typeText('Vi leverer til Bergen.')
    await share()
    await screen.findByRole('alert')
    expect(textarea().value).toBe('Vi leverer til Bergen.')
    expect(shareButton().disabled).toBe(false)
    await share()
    await screen.findByRole('heading', { name: 'Delt med rommet' })
    expect(actions.executeAction).toHaveBeenCalledTimes(2)
  })
})

describe('WS17-S7b: taking a shared copy back', () => {
  it('keeps the dialog open on the copy after a successful share, with the room named and take-back offered', async () => {
    const onClose = vi.fn()
    actions.executeAction.mockResolvedValueOnce(shareOutcome())
    renderDialog({ onClose })
    await pickRoom('Room A')
    await share()
    await screen.findByRole('heading', { name: 'Delt med rommet' })
    expect(screen.getByText('Rom: Room A')).toBeTruthy()
    expect(
      screen.getByText(
        'Alle i dette rommet kan se teksten. Du kan ta den tilbake når som helst. Ditt eget minne endres ikke.',
      ),
    ).toBeTruthy()
    expect(textarea().disabled).toBe(true)
    expect(takeBackButton()).not.toBeNull()
    expect(onClose).not.toHaveBeenCalled()
  })

  it('takes the copy back with the room copy id and a fresh request id, never the share\'s own id', async () => {
    await shareToRoomA()
    actions.executeAction.mockResolvedValueOnce(takeBackOutcome())
    takeBack()
    await screen.findByText('Minnet er tatt tilbake fra rommet.')

    const shareCall = callAt(0)
    const takeBackCall = callAt(1)
    expect(takeBackCall[0]).toBe('memory.retract_from_space')
    expect(takeBackCall[1]).toEqual(ACTOR)
    expect(takeBackCall[2]).toEqual({ memoryId: 'room-copy-1', spaceRef: 'room-a', clientRequestId: expect.any(String) })
    expect(takeBackCall[2].clientRequestId).toMatch(UUID)
    expect(takeBackCall[2].clientRequestId).not.toBe(shareCall[2].clientRequestId)
    expect(takeBackCall[3]).toEqual({ idempotencyKey: takeBackCall[2].clientRequestId })
  })

  it('shows the copy as taken back when the room answers memory_not_found', async () => {
    await shareToRoomA()
    actions.executeAction.mockRejectedValueOnce(new ApiError('gone', 404, 'memory_not_found'))
    takeBack()
    await screen.findByText('Minnet er tatt tilbake fra rommet.')
    expect(takeBackButton()).toBeNull()
  })

  it('keeps the take-back control and shows its copy on another error', async () => {
    await shareToRoomA()
    actions.executeAction.mockRejectedValueOnce(new ApiError('no', 403, 'memory_retract_not_authorized'))
    takeBack()
    expect((await screen.findByRole('alert')).textContent).toBe(
      'Du har ikke tilgang til å ta dette minnet tilbake fra rommet. Ingenting er endret.',
    )
    expect(takeBackButton()).not.toBeNull()
    expect(takeBackButton()?.disabled).toBe(false)
    expect(screen.queryByText('Minnet er tatt tilbake fra rommet.')).toBeNull()
  })
})
