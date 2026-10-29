import { Route, Router } from '@solidjs/router'
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import SpacePage from './SpacePage'

const spacesClient = vi.hoisted(() => ({
  createSpaceAgent: vi.fn(),
  bindSpaceAgent: vi.fn(),
  getInstallableSpaceAgents: vi.fn(),
  getSpaceAgents: vi.fn(),
  getSpaceContext: vi.fn(),
  getSpaceRoster: vi.fn(),
  getSpaceThreads: vi.fn(),
  getPersonalSpaceDeletionReceipt: vi.fn(),
  listSpaces: vi.fn(),
  requestPersonalSpaceDeletion: vi.fn(),
  // ADR-0003: the Agent tab now renders SpaceInstructionsSection, which reads
  // (and, for an editor/manager/owner, writes) the Space's authored
  // instructions. Both must exist on the mock or every test in this file
  // fails at module-resolution time, before it renders anything.
  getSpaceInstructions: vi.fn(),
  updateSpaceInstructions: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  createSpaceAgent: spacesClient.createSpaceAgent,
  bindSpaceAgent: spacesClient.bindSpaceAgent,
  getInstallableSpaceAgents: spacesClient.getInstallableSpaceAgents,
  getSpaceAgents: spacesClient.getSpaceAgents,
  getSpaceContext: spacesClient.getSpaceContext,
  getSpaceRoster: spacesClient.getSpaceRoster,
  getSpaceThreads: spacesClient.getSpaceThreads,
  getPersonalSpaceDeletionReceipt: spacesClient.getPersonalSpaceDeletionReceipt,
  listSpaces: spacesClient.listSpaces,
  requestPersonalSpaceDeletion: spacesClient.requestPersonalSpaceDeletion,
  getSpaceInstructions: spacesClient.getSpaceInstructions,
  updateSpaceInstructions: spacesClient.updateSpaceInstructions,
}))

vi.mock('@/features/chat/lib/chat-thread-history', () => ({
  selectChatThread: vi.fn(),
}))

const chatClient = vi.hoisted(() => ({
  streamChat: vi.fn(),
  getChatThreadTranscript: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', () => ({
  streamChat: chatClient.streamChat,
  getChatThreadTranscript: chatClient.getChatThreadTranscript,
}))

const personalContext = {
  space: { space_ref: 'space_personal_1', name: 'Personal Space', kind: 'personal', lifecycle: 'active' },
  membership: {
    space_ref: 'space_personal_1', org_id: 'org_1', subject_id: 'user_1', kind: 'personal', role: 'owner',
    revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
  },
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve
    reject = promiseReject
  })
  return { promise, resolve, reject }
}

function renderSpacePage() {
  window.history.replaceState({}, '', '/spaces/space_personal_1')
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/spaces/:spaceId" component={SpacePage} />
    </Router>
  ))
}

// No I18nProvider wraps these renders, so useI18n() resolves to the module's
// fallback context — whose `tr` always picks the Norwegian argument, exactly
// matching defaultLocale ('no'). That is real default behavior, not a test
// shortcut: a fresh session with no stored locale preference renders
// Norwegian, so these assertions pin what an actual first-time user sees.
describe('SpacePage', () => {
  beforeEach(() => {
    spacesClient.getSpaceContext.mockReset()
    spacesClient.getSpaceThreads.mockReset()
    spacesClient.getPersonalSpaceDeletionReceipt.mockReset()
    spacesClient.listSpaces.mockReset()
    spacesClient.requestPersonalSpaceDeletion.mockReset()
    spacesClient.getSpaceAgents.mockReset()
    spacesClient.getSpaceAgents.mockResolvedValue([])
    spacesClient.getSpaceRoster.mockResolvedValue([])
    // Default to "nothing authored" so the Agent tab's instructions section
    // resolves without asserting anything about it here; the section's own
    // read/write gating is covered server-side in the gateway's tests.
    spacesClient.getSpaceInstructions.mockReset()
    spacesClient.getSpaceInstructions.mockResolvedValue('')
    spacesClient.updateSpaceInstructions.mockReset()
    spacesClient.updateSpaceInstructions.mockResolvedValue('')
    spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [] })
    spacesClient.listSpaces.mockResolvedValue([personalContext.space])
    chatClient.streamChat.mockReset()
    chatClient.streamChat.mockResolvedValue(undefined)
    chatClient.getChatThreadTranscript.mockReset()
    chatClient.getChatThreadTranscript.mockResolvedValue(null)
    spacesClient.createSpaceAgent.mockReset()
    spacesClient.createSpaceAgent.mockResolvedValue({ agent_ref: 'agent123', subject_id: 'agent-agent123', status: 'active' })
    spacesClient.bindSpaceAgent.mockReset()
    spacesClient.bindSpaceAgent.mockResolvedValue({ subject_id: 'agent-agent2', status: 'active' })
    spacesClient.getInstallableSpaceAgents.mockReset()
    spacesClient.getInstallableSpaceAgents.mockResolvedValue([])
  })

  afterEach(() => vi.restoreAllMocks())

  it('fails closed on the next membership recheck instead of keeping stale Space content visible', async () => {
    spacesClient.getSpaceContext
      .mockResolvedValueOnce(personalContext)
      .mockRejectedValueOnce(new Error('membership revoked'))
    let recheck: (() => void) | undefined
    vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
      if (timeout === 30_000) recheck = handler as () => void
      return 1 as unknown as number
    }) as typeof window.setInterval)

    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()

    expect(recheck).toBeTypeOf('function')
    recheck!()
    await waitFor(() => expect(spacesClient.getSpaceContext).toHaveBeenCalledTimes(2))

    expect(await screen.findByRole('heading', { name: 'Rom utilgjengelig' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Personlig rom' })).toBeNull()
    expect(screen.queryByRole('link', { name: 'Chat' })).toBeNull()
  })

  it('composes the room projection into the cockpit with a visible work pulse', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [
        {
          thread_id: 'thread_running',
          space_id: 'space_personal_1',
          title: 'Prepare launch brief',
          preview: 'Collecting the latest release evidence.',
          latest_run_status: 'running',
          latest_run_updated_at: '2026-08-14T10:00:00Z',
        },
      ],
    })

    renderSpacePage()

    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()
    expect(screen.getByRole('tab', { name: 'Samtaler' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Rompuls' })).toBeTruthy()
    expect(screen.getByText('Verevon jobber')).toBeTruthy()
    // The pulse card reports state; it must not route room work out to /chat.
    expect(screen.queryByRole('link', { name: /Verevon jobber/ })).toBeNull()
    expect(screen.queryByRole('link', { name: 'Åpne Agent Studio' })).toBeNull()
  })

  it('keeps the Space composer link encoded without rendering a duplicate Space overview rail', async () => {
    spacesClient.getSpaceContext.mockResolvedValue({
      ...personalContext,
      space: { ...personalContext.space, space_ref: 'space / personal' },
      membership: { ...personalContext.membership, space_ref: 'space / personal' },
    })
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      space: { ...personalContext.space, space_ref: 'space / personal' },
      membership: { ...personalContext.membership, space_ref: 'space / personal' },
      threads: [
        {
          thread_id: 'thread / launch',
          space_id: 'space / personal',
          title: 'Launch plan',
          preview: 'Latest release preparation.',
          latest_run_status: 'running',
        },
        {
          thread_id: 'thread-retro',
          space_id: 'space / personal',
          title: 'Retro notes',
          preview: 'Capture the learnings.',
          latest_run_status: 'completed',
        },
      ],
    })

    window.history.replaceState({}, '', '/spaces/space%20%2F%20personal')
    render(() => (
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/spaces/:spaceId" component={SpacePage} />
      </Router>
    ))

    // The room's conversations render inline as posts — the shared record
    // stays in the room, with no per-thread link out to /chat to encode.
    expect(await screen.findByText('Latest release preparation.')).toBeTruthy()
    const timeline = within(screen.getByRole('list', { name: 'Samtaler i rommet' }))
    expect(timeline.getByText('Capture the learnings.')).toBeTruthy()
    expect(timeline.getByText('Arbeider')).toBeTruthy()
    expect(timeline.getByText('Fullført')).toBeTruthy()
    // The room's own composer targets this exact, unencoded Space reference —
    // no href-encoding step to get right or wrong, since it is a component
    // prop reaching `streamChat` directly rather than text baked into a link.
    fireEvent.input(screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.'), {
      target: { value: 'Hei' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      content: 'Hei',
      spaceRef: 'space / personal',
    })
    expect(screen.queryByRole('complementary', { name: 'Space overview' })).toBeNull()
  })

  it('opens a fresh room with the buzz-style intro and drops the cursor in the composer', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [] })
    renderSpacePage()

    // The intro is the room's own beginning — no link out to Chat and no
    // detour to Agent Studio just to have a first conversation.
    expect(await screen.findByText(/Dette er begynnelsen på det delte arkivet for/)).toBeTruthy()
    // "Personlig rom" now appears twice on purpose: the eyebrow labels the kind,
    // and the title is the translated provisioning default. What this pins is
    // that the room reads as personal rather than shared.
    expect(screen.getAllByText('Personlig rom').length).toBeGreaterThan(0)
    expect(screen.queryByText('Delt arbeidsrom')).toBeNull()
    expect(screen.queryByRole('link', { name: 'Chat' })).toBeNull()
    expect(screen.queryByRole('link', { name: 'Åpne Agent Studio' })).toBeNull()

    const composer = screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.')
    const [introStart] = screen.getAllByRole('button', { name: /Start en samtale/ }).reverse()
    fireEvent.click(introStart!)
    expect(document.activeElement).toBe(composer)

    fireEvent.click(screen.getByRole('tab', { name: 'Medlemmer' }))
    // The Members tab no longer says a roster is unpublished — Control now
    // publishes one, so the tab states whose list it is showing instead.
    expect(screen.getByText(/listen under kommer fra Control/)).toBeTruthy()
  })

  it('renders the shared record as attributed inline turns', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceRoster.mockResolvedValue([
      { subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
      { subject_type: 'service', subject_id: 'agent-drift', role: 'editor', revision: 1, display_name: 'Driftsassistent' },
    ])
    // TWO active agents on purpose: the room-level fallback heuristic cannot
    // name either, so the rendered name below can only come from the turn's
    // own recorded persona.
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        name: 'Driftsassistent', status: 'active', definition_status: 'published', delivery_targets: [],
      },
      {
        subject_id: 'agent-status', role: 'editor', revision: 1, identity_published: true,
        name: 'Statusagent', status: 'active', definition_status: 'published', delivery_targets: [],
      },
    ])
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_drift',
        space_id: 'space_personal_1',
        title: 'Driftsstatus',
        preview: 'Hva er status?',
        latest_run_status: 'completed',
        updated_at: '2026-08-16T10:00:00Z',
      }],
    })
    chatClient.getChatThreadTranscript.mockResolvedValue({
      threadId: 'thread_drift',
      updatedAt: '2026-08-16T10:00:00Z',
      turns: [
        { id: 'canonical-1', role: 'user', content: 'Hva er status?' },
        { id: 'canonical-2', role: 'assistant', content: '**Alt** er grønt.', agentName: 'Driftsassistent' },
      ],
    })
    renderSpacePage()

    // The human turn carries the sole human member's name (transcript reads
    // are owner-bound, so the readable user turn is provably theirs), and the
    // agent turn carries the room's single active agent with an Agent chip —
    // rendered as real markdown, not raw asterisks. Queries are scoped to the
    // timeline because the cockpit keeps every tab panel mounted, so the same
    // names also exist in the (hidden) member and agent panels. The await
    // targets assistant-only content: the post's preview fallback shows the
    // same text as the user turn, so waiting on that would pass before the
    // transcript resource ever resolved.
    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    expect(await timeline.findByText('Driftsassistent')).toBeTruthy()
    expect(timeline.getByText('Hva er status?')).toBeTruthy()
    expect(timeline.getByText('Kari Nordmann')).toBeTruthy()
    expect(timeline.getByText('Agent')).toBeTruthy()
    expect(timeline.getByText('Alt')).toBeTruthy()
    expect(timeline.queryByText('**Alt** er grønt.')).toBeNull()
  })

  it('continues an existing room thread when a post is answered through the reply target', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_drift',
        space_id: 'space_personal_1',
        title: 'Driftsstatus',
        preview: 'Hva er status?',
        latest_run_status: 'completed',
        updated_at: '2026-08-16T10:00:00Z',
      }],
    })
    chatClient.streamChat.mockImplementation(async (_request: unknown, handlers: { onDone?: () => void }) => {
      handlers.onDone?.()
    })
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    fireEvent.click(timeline.getByRole('button', { name: 'Svar' }))

    // The banner names the target and the cursor lands in the composer.
    expect(screen.getByText(/Svarer i/)).toBeTruthy()
    const composer = screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.')
    expect(document.activeElement).toBe(composer)

    fireEvent.input(composer, { target: { value: 'Og lagerstatus?' } })
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      content: 'Og lagerstatus?',
      spaceRef: 'space_personal_1',
      threadId: 'thread_drift',
    })
    // A settled reply releases the target — the next message is a new post.
    await waitFor(() => expect(screen.queryByText(/Svarer i/)).toBeNull())
  })

  it('an owner creates a room agent from the Agent tab through a template', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: 'Agent' }))
    fireEvent.click(screen.getByRole('button', { name: 'Opprett en agent' }))

    const dialog = within(await screen.findByRole('dialog', { name: 'Opprett en agent' }))
    fireEvent.click(dialog.getByRole('button', { name: /Møtereferent/ }))
    // The template prefills both fields; the name stays editable.
    expect((dialog.getByPlaceholderText('Hva skal agenten hete?') as HTMLInputElement).value).toBe('Møtereferent')
    fireEvent.click(dialog.getByRole('button', { name: 'Opprett agent' }))

    await waitFor(() => expect(spacesClient.createSpaceAgent).toHaveBeenCalled())
    const [spaceRef, input] = spacesClient.createSpaceAgent.mock.calls[0] as [string, { name: string; instructions?: string; avatarColor?: string }]
    expect(spaceRef).toBe('space_personal_1')
    expect(input.name).toBe('Møtereferent')
    expect(input.instructions).toContain('møtereferent')
    expect(input.avatarColor).toMatch(/^#[0-9a-f]{6}$/i)
    // A confirmed create closes the dialog and refetches the room's agents so
    // the new member appears without a reload.
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Opprett en agent' })).toBeNull())
    expect(spacesClient.getSpaceAgents.mock.calls.length).toBeGreaterThan(1)
  })

  it('an owner binds an existing agent definition from the Agent tab', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getInstallableSpaceAgents.mockResolvedValue([
      { agent_ref: 'agent-1', name: 'Driftsassistent', description: '', definition_status: 'active', already_bound: true },
      { agent_ref: 'agent-2', name: 'Møtereferent', description: 'Skriver referat', definition_status: 'active', already_bound: false },
    ])
    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: 'Agent' }))
    fireEvent.click(screen.getByRole('button', { name: 'Legg til eksisterende agent' }))

    const dialog = within(await screen.findByRole('dialog', { name: 'Legg til en eksisterende agent' }))
    // Already-bound definitions report their state instead of offering an
    // "Add" control a second time.
    expect(await dialog.findByText('Allerede lagt til')).toBeTruthy()
    fireEvent.click(dialog.getByRole('button', { name: 'Legg til' }))

    await waitFor(() => expect(spacesClient.bindSpaceAgent).toHaveBeenCalledWith('space_personal_1', 'agent-2'))
    // A confirmed bind refetches the room's agents so the new member appears
    // without a reload.
    await waitFor(() => expect(spacesClient.getSpaceAgents.mock.calls.length).toBeGreaterThan(1))
  })

  it('members without grant authority get no create-agent or bind-agent entry points', async () => {
    spacesClient.getSpaceContext.mockResolvedValue({
      ...personalContext,
      membership: { ...personalContext.membership, role: 'editor' },
    })
    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()

    fireEvent.click(screen.getByRole('tab', { name: 'Agent' }))
    expect(screen.queryByRole('button', { name: 'Opprett en agent' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Legg til eksisterende agent' })).toBeNull()
  })

  it('a dismissed reply target starts a fresh thread instead of appending', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_drift',
        space_id: 'space_personal_1',
        title: 'Driftsstatus',
        preview: 'Hva er status?',
        latest_run_status: 'completed',
        updated_at: '2026-08-16T10:00:00Z',
      }],
    })
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    fireEvent.click(timeline.getByRole('button', { name: 'Svar' }))
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt svar' }))
    expect(screen.queryByText(/Svarer i/)).toBeNull()

    fireEvent.input(screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.'), {
      target: { value: 'Ny sak' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      content: 'Ny sak',
      spaceRef: 'space_personal_1',
    })
    expect((chatClient.streamChat.mock.calls[0]?.[0] as { threadId?: string }).threadId).toBeUndefined()
  })

  it('does not mistake a failed conversation projection for a fresh Space', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockRejectedValue(new Error('projection unavailable'))
    renderSpacePage()

    expect(await screen.findByText(/Samtaleaktiviteten i rommet er midlertidig utilgjengelig/)).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Start the conversation in Personal Space' })).toBeNull()
    expect(screen.getByRole('heading', { name: 'Samtalearkivet er utilgjengelig' })).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Åpne Agent Studio' })).toBeNull()
  })

  /**
   * The Agent tab renders room participants, not a catalog. These pin the three
   * states the scope plan insists stay distinguishable: a real "nobody is
   * bound", a "we could not check", and an agent Control authorized that no
   * plane has named yet.
   */
  describe('the Agent view shows room participants truthfully', () => {
    async function openAgentTab() {
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      renderSpacePage()
      expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()
      fireEvent.click(screen.getByRole('tab', { name: 'Agent' }))
    }

    it('renders a bound agent with its role, status and published channels', async () => {
      spacesClient.getSpaceAgents.mockResolvedValue([
        {
          subject_id: 'svc-support',
          role: 'editor',
          revision: 4,
          identity_published: true,
          binding_ref: 'sab_1',
          agent_ref: 'agent_1',
          name: 'Kundestøtte',
          title: 'Support',
          description: 'Svarer på henvendelser i rommet.',
          status: 'active',
          definition_status: 'active',
          delivery_targets: [
            { channel: 'teams', label: 'Drift', status: 'active' },
            { channel: 'messenger', label: 'Verevon AS', status: 'pending' },
          ],
        },
      ])
      await openAgentTab()

      // Scoped to the card: "Aktiv" is also the Space's own lifecycle in the
      // header, and asserting globally would pass on the wrong element.
      const card = (await screen.findByText('Kundestøtte')).closest('li')
      expect(card).toBeTruthy()
      const agentCard = within(card as HTMLElement)
      expect(agentCard.getByText('Svarer på henvendelser i rommet.')).toBeTruthy()
      // Role comes from Control; status from the Application binding.
      expect(agentCard.getByText('Redaktør')).toBeTruthy()
      expect(agentCard.getByText('Aktiv')).toBeTruthy()
      expect(agentCard.getByText(/Microsoft Teams/)).toBeTruthy()
      expect(agentCard.getByText(/Messenger/)).toBeTruthy()
      expect(agentCard.getByText(/venter/)).toBeTruthy()
    })

    it('says plainly when Control authorized an agent nobody has named', async () => {
      spacesClient.getSpaceAgents.mockResolvedValue([
        {
          subject_id: 'svc-orphan',
          role: 'editor',
          revision: 2,
          identity_published: false,
          delivery_targets: [],
        },
      ])
      await openAgentTab()

      expect(await screen.findByText(/ingen identitet er publisert/)).toBeTruthy()
      expect(screen.getByText('Ingen kanaler publisert')).toBeTruthy()
      // The subject id is not a name and must never be dressed up as one.
      expect(screen.queryByText('svc-orphan')).toBeNull()
    })

    it('separates "no agents are bound" from "we could not check"', async () => {
      spacesClient.getSpaceAgents.mockResolvedValue([])
      await openAgentTab()
      expect(await screen.findByText('Ingen agenter er bundet til dette rommet ennå.')).toBeTruthy()
    })

    it('never reports an empty room when the agent list fails to load', async () => {
      spacesClient.getSpaceAgents.mockRejectedValue(new Error('projection unavailable'))
      await openAgentTab()

      expect(await screen.findByText(/Agentlisten kunne ikke hentes/)).toBeTruthy()
      expect(screen.queryByText('Ingen agenter er bundet til dette rommet ennå.')).toBeNull()
    })

    it('shows binding policy chips only when the binding carries a policy', async () => {
      spacesClient.getSpaceAgents.mockResolvedValue([
        {
          subject_id: 'svc-born', role: 'editor', revision: 1, identity_published: true,
          name: 'Statusagent', status: 'active', definition_status: 'active',
          trigger_modes: ['mention'], allowed_tools: [], approval_mode: 'require_confirmation',
          delivery_targets: [],
        },
        {
          subject_id: 'svc-legacy', role: 'editor', revision: 1, identity_published: true,
          name: 'Driftsassistent', status: 'active', definition_status: 'active',
          delivery_targets: [],
        },
      ])
      await openAgentTab()

      // The room-born policy renders as chips; the legacy binding renders no
      // policy at all, because absence is legacy behavior — not a choice.
      expect(await screen.findByText('Kun @-nevning')).toBeTruthy()
      expect(screen.getByText('Krever bekreftelse')).toBeTruthy()
      expect(screen.getByText('Uten verktøy')).toBeTruthy()
      expect(screen.getAllByRole('list', { name: 'Bindingspolicy' }).length).toBe(1)
    })

    it('surfaces a draft definition sitting behind an active binding', async () => {
      spacesClient.getSpaceAgents.mockResolvedValue([
        {
          subject_id: 'svc-draft',
          role: 'viewer',
          revision: 1,
          identity_published: true,
          name: 'Utkastagent',
          status: 'active',
          definition_status: 'draft',
          delivery_targets: [],
        },
      ])
      await openAgentTab()

      expect(await screen.findByText(/Agentdefinisjonen er et utkast/)).toBeTruthy()
    })
  })

  it('keeps a newer thread projection available when an older request fails late', async () => {
    const staleThreads = deferred<Awaited<ReturnType<typeof spacesClient.getSpaceThreads>>>()
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads
      .mockReturnValueOnce(staleThreads.promise)
      .mockResolvedValueOnce({ ...personalContext, threads: [] })
    let recheck: (() => void) | undefined
    vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
      if (timeout === 30_000) recheck = handler as () => void
      return 1 as unknown as number
    }) as typeof window.setInterval)

    renderSpacePage()
    expect(await screen.findByRole('heading', { name: 'Personlig rom' })).toBeTruthy()
    recheck!()

    expect(await screen.findByText(/Dette er begynnelsen på det delte arkivet for/)).toBeTruthy()
    staleThreads.reject(new Error('stale projection failed'))

    await waitFor(() => expect(screen.queryByRole('heading', { name: 'Samtalearkivet er utilgjengelig' })).toBeNull())
    expect(screen.getByText(/Dette er begynnelsen på det delte arkivet for/)).toBeTruthy()
  })

  /**
   * The room's own composer, per `docs/space-defenition.md`'s invocation
   * rule: `@` offers this room's roster, an agent mention rides the existing
   * chat SSE pipeline as a structured ref (never as decorative text), and a
   * person mention never does — only an agent mention may invoke anything.
   */
  describe('the room composer', () => {
    const composerPlaceholder = 'Skriv i rommet. Skriv @ for å nevne noen.'

    beforeEach(() => {
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      spacesClient.getSpaceRoster.mockResolvedValue([
        { subject_type: 'user', subject_id: 'user_2', role: 'editor', revision: 1, display_name: 'Kari' },
      ])
      spacesClient.getSpaceAgents.mockResolvedValue([
        {
          subject_id: 'svc-support',
          role: 'editor',
          revision: 1,
          identity_published: true,
          name: 'Driftsassistent',
          status: 'active',
          delivery_targets: [],
        },
      ])
    })

    it('sends a plain message with no mentioned agent', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), { target: { value: 'Hei alle' } })
      fireEvent.click(screen.getByRole('button', { name: 'Send' }))

      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
        content: 'Hei alle',
        spaceRef: 'space_personal_1',
        mentionedAgentRef: undefined,
      })
    })

    it('offers both people and agents from this room\'s own roster on @', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), { target: { value: '@' } })

      expect(await screen.findByRole('option', { name: /Driftsassistent/ })).toBeTruthy()
      expect(screen.getByRole('option', { name: /Kari/ })).toBeTruthy()
    })

    it('selecting an agent mention sends its Control subject id as mentionedAgentRef', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      const textarea = screen.getByPlaceholderText(composerPlaceholder)
      fireEvent.input(textarea, { target: { value: '@Drift' } })
      fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
      expect(await screen.findByText(/Vil invokere en agent/)).toBeTruthy()

      fireEvent.click(screen.getByRole('button', { name: 'Send' }))
      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      const [request] = chatClient.streamChat.mock.calls[0] ?? []
      expect(request).toMatchObject({ mentionedAgentRef: 'svc-support' })
      expect((request as { content?: string }).content).toContain('@Driftsassistent')
    })

    it('selecting a person mention never sets mentionedAgentRef', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), { target: { value: '@Kar' } })
      fireEvent.click(await screen.findByRole('option', { name: /Kari/ }))
      expect(screen.queryByText(/Vil invokere en agent/)).toBeNull()

      fireEvent.click(screen.getByRole('button', { name: 'Send' }))
      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ mentionedAgentRef: undefined })
    })

    it('streams the agent reply inline as it arrives', async () => {
      chatClient.streamChat.mockImplementation(async (_request, handlers) => {
        handlers.onMessage?.({ content: 'Hallo' })
        handlers.onMessage?.({ content: ' der' })
        handlers.onDone?.({})
      })
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), { target: { value: 'Hei' } })
      fireEvent.click(screen.getByRole('button', { name: 'Send' }))

      expect(await screen.findByText('Hallo der')).toBeTruthy()
    })

    it('refetches the thread list once the exchange settles, so the new thread appears', async () => {
      chatClient.streamChat.mockImplementation(async (_request, handlers) => {
        handlers.onDone?.({})
      })
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })
      const callsBeforeSend = spacesClient.getSpaceThreads.mock.calls.length

      fireEvent.input(screen.getByPlaceholderText(composerPlaceholder), { target: { value: 'Hei' } })
      fireEvent.click(screen.getByRole('button', { name: 'Send' }))

      await waitFor(() =>
        expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(callsBeforeSend),
      )
    })
  })
})
