import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
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
  // The room reads its own record through the Space route, not Chat's
  // owner-bound one — that is what lets it show another member's turns.
  getSpaceThreadTranscript: vi.fn(),
  setSpaceAgentState: vi.fn(),
  revokeSpaceAgent: vi.fn(),
  // The Work tab stays mounted like every other panel, so it fetches on every
  // render of this page whether or not a test is looking at it.
  getSpaceWork: vi.fn(),
  getSpaceKnowledge: vi.fn(),
  getSpaceActivity: vi.fn(),
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
  getSpaceThreadTranscript: spacesClient.getSpaceThreadTranscript,
  setSpaceAgentState: spacesClient.setSpaceAgentState,
  revokeSpaceAgent: spacesClient.revokeSpaceAgent,
  getSpaceWork: spacesClient.getSpaceWork,
  getSpaceKnowledge: spacesClient.getSpaceKnowledge,
  getSpaceActivity: spacesClient.getSpaceActivity,
}))

vi.mock('@/features/chat/lib/chat-thread-history', () => ({
  selectChatThread: vi.fn(),
}))

const orchestration = vi.hoisted(() => ({
  listApprovals: vi.fn(),
  decideApproval: vi.fn(),
  resumeRun: vi.fn(),
  cancelRun: vi.fn(),
}))

vi.mock('@/shared/api/orchestration-client', () => orchestration)

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

function renderSpacePage(path = '/spaces/space_personal_1') {
  // Router v2 drives the route through `memoryHistory`, which never touches
  // `window.location` — but `SpaceCockpit` reads `window.location.hash` to adopt
  // a deep-linked tab, and `select()` writes it back with `history.replaceState`.
  // Under Router v1 this helper set the location itself, so every render started
  // from a hash-free URL; without that reset the jsdom window keeps the previous
  // test's `#agent` / `#medlemmer`, the cockpit opens on that tab, and the Chat
  // panel — which holds the room timeline and composer — renders `hidden`, i.e.
  // invisible to every role query. Reset the window URL alongside the memory
  // history so each render starts on the Chat tab, as a fresh page load does.
  window.history.replaceState({}, '', path)
  const TestRouter = createRouter({
    routes: [{ path: '/spaces/:spaceId', component: SpacePage }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
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
    spacesClient.getSpaceThreadTranscript.mockReset()
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ threadId: '', turns: [] })
    spacesClient.setSpaceAgentState.mockReset()
    spacesClient.setSpaceAgentState.mockResolvedValue({ status: 'paused', changed: true })
    spacesClient.revokeSpaceAgent.mockReset()
    spacesClient.revokeSpaceAgent.mockResolvedValue({ status: 'revoked' })
    spacesClient.getSpaceWork.mockReset()
    spacesClient.getSpaceWork.mockResolvedValue({
      space: personalContext.space, membership: personalContext.membership,
      runs: [], schedules: [], unavailable: [],
    })
    spacesClient.getSpaceKnowledge.mockReset()
    spacesClient.getSpaceKnowledge.mockResolvedValue({
      space: personalContext.space, membership: personalContext.membership,
      binding: null, documents: [], documents_truncated: false, wiki_pages: [], unavailable: [],
    })
    spacesClient.getSpaceActivity.mockReset()
    spacesClient.getSpaceActivity.mockResolvedValue({
      space: personalContext.space, membership: personalContext.membership,
      runs: [], approvals: [], operations: [], authority: [], unavailable: [],
    })
    orchestration.listApprovals.mockReset()
    orchestration.listApprovals.mockResolvedValue([])
    orchestration.decideApproval.mockReset()
    orchestration.decideApproval.mockResolvedValue({ id: 'ap-1', status: 'GRANTED' })
    orchestration.resumeRun.mockReset()
    orchestration.resumeRun.mockResolvedValue(undefined)
    orchestration.cancelRun.mockReset()
    orchestration.cancelRun.mockResolvedValue({ cancelled: true })
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

    // The Space ref is 'space / personal', so the route param arrives
    // percent-encoded; the v2 router takes it through memoryHistory rather than
    // window.history, but the decoding path under test is the same.
    renderSpacePage('/spaces/space%20%2F%20personal')

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
    flush()
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
    flush()
    expect(document.activeElement).toBe(composer)

    fireEvent.click(screen.getByRole('tab', { name: 'Medlemmer' }))
    flush()
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
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({
      threadId: 'thread_drift',
      turns: [
        { role: 'user', content: 'Hva er status?', authorSubjectId: 'user_1' },
        { role: 'assistant', content: '**Alt** er grønt.', agentName: 'Driftsassistent' },
      ],
    })
    renderSpacePage()

    // The human turn carries the name the SERVER recorded as its author,
    // resolved through Control's roster, and the agent turn carries the turn's
    // own recorded persona with an Agent chip — rendered as real markdown, not
    // raw asterisks. Queries are scoped to the timeline because the cockpit
    // keeps every tab panel mounted, so the same names also exist in the
    // (hidden) member and agent panels. The await targets assistant-only
    // content: the post's preview fallback shows the same text as the user
    // turn, so waiting on that would pass before the transcript resolved.
    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    expect(await timeline.findByText('Driftsassistent')).toBeTruthy()
    expect(timeline.getByText('Hva er status?')).toBeTruthy()
    expect(timeline.getByText('Kari Nordmann (deg)')).toBeTruthy()
    expect(timeline.getByText('Agent')).toBeTruthy()
    expect(timeline.getByText('Alt')).toBeTruthy()
    expect(timeline.queryByText('**Alt** er grønt.')).toBeNull()
    // The room read its own record, not Chat's owner-bound one.
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledWith('space_personal_1', 'thread_drift')
    expect(chatClient.getChatThreadTranscript).not.toHaveBeenCalled()
  })

  /** Open the Agent tab and return a scope for querying inside its panel.
   *
   * The cockpit keeps every tab mounted and hides the inactive ones, so a
   * role-based query against the whole screen cannot see the agent controls
   * while another tab is open — and a query that passes only because the
   * element is hidden proves nothing. */
  async function openAgentTab() {
    const tab = await screen.findByRole('tab', { name: 'Agent' })
    tab.click()
    return within(await screen.findByRole('tabpanel', { name: 'Agent' }))
  }

  /** Open the Members tab and return a scope for querying inside its panel. */
  async function openMembersTab() {
    const tab = await screen.findByRole('tab', { name: 'Medlemmer' })
    tab.click()
    return within(await screen.findByRole('tabpanel', { name: 'Medlemmer' }))
  }

  const roomContext = {
    space: { space_ref: 'space_room_1', name: 'Leveranse', kind: 'room', lifecycle: 'active', is_organization_room: false },
    membership: {
      space_ref: 'space_room_1', org_id: 'org_1', subject_id: 'user_1', kind: 'room', role: 'owner',
      revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
    },
  }

  it('lets an owner manage the people in a room they made', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(roomContext)
    spacesClient.getSpaceThreads.mockResolvedValue({ ...roomContext, threads: [] })
    renderSpacePage()

    const members = await openMembersTab()
    expect(await members.findByRole('button', { name: /Legg til personer/ })).toBeTruthy()
  })

  // The organization channel's roster is derived from the organization, so an
  // editor here would be overwritten on the next sync while appearing to work.
  it('offers no member editor for the organization channel, and says why', async () => {
    spacesClient.getSpaceContext.mockResolvedValue({
      ...roomContext,
      space: { ...roomContext.space, is_organization_room: true },
    })
    spacesClient.getSpaceThreads.mockResolvedValue({ ...roomContext, threads: [] })
    renderSpacePage()

    const members = await openMembersTab()
    expect(members.queryByRole('button', { name: /Legg til personer/ })).toBeNull()
    expect(await members.findByText(/følger organisasjonen/)).toBeTruthy()
  })

  // Absence is not a denial: an older gateway omits the flag entirely, and
  // showing the editor on that silence offers a door the server then refuses.
  it('withholds the member editor when the server did not say which room this is', async () => {
    const { is_organization_room: _omitted, ...spaceWithoutFlag } = roomContext.space
    spacesClient.getSpaceContext.mockResolvedValue({ ...roomContext, space: spaceWithoutFlag })
    spacesClient.getSpaceThreads.mockResolvedValue({ ...roomContext, threads: [] })
    renderSpacePage()

    const members = await openMembersTab()
    expect(members.queryByRole('button', { name: /Legg til personer/ })).toBeNull()
    // Nor does it claim the roster is derived — it says nothing, which is what
    // it knows.
    expect(members.queryByText(/følger organisasjonen/)).toBeNull()
  })

  // "Pause / mute / remove HERE" is the room half of the product model's
  // dividing rule. Until now the room could add an agent and never take one
  // back, so a wrongly added agent stayed invokable.
  it('lets an owner pause a bound agent, then re-reads the roster', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'active',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    const pause = await agentPanel.findByRole('button', { name: 'Sett på pause' })
    pause.click()

    await waitFor(() => expect(spacesClient.setSpaceAgentState).toHaveBeenCalled())
    expect(spacesClient.setSpaceAgentState).toHaveBeenCalledWith('space_personal_1', 'sab_drift', 'paused')
    // The card's standing comes from the server, so the panel re-reads rather
    // than flipping the label locally.
    await waitFor(() => expect(spacesClient.getSpaceAgents.mock.calls.length).toBeGreaterThan(1))
  })

  it('offers Resume for an agent that is already paused', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'paused',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    spacesClient.setSpaceAgentState.mockResolvedValue({ status: 'active', changed: true })
    renderSpacePage()

    const agentPanel = await openAgentTab()
    ;(await agentPanel.findByRole('button', { name: 'Gjenoppta' })).click()
    await waitFor(() =>
      expect(spacesClient.setSpaceAgentState).toHaveBeenCalledWith('space_personal_1', 'sab_drift', 'active'),
    )
    expect(agentPanel.queryByRole('button', { name: 'Sett på pause' })).toBeNull()
  })

  // Removal is the one control the other button cannot undo.
  it('arms before removing an agent, and the first press changes nothing', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'active',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    ;(await agentPanel.findByRole('button', { name: 'Fjern fra rommet' })).click()
    expect(spacesClient.revokeSpaceAgent).not.toHaveBeenCalled()
    expect(await agentPanel.findByText(/legges til på nytt/)).toBeTruthy()

    ;(await agentPanel.findByRole('button', { name: /Bekreft at Driftsassistent fjernes/ })).click()
    await waitFor(() =>
      expect(spacesClient.revokeSpaceAgent).toHaveBeenCalledWith('space_personal_1', 'sab_drift'),
    )
  })

  // A pending binding is waiting on Control; a failed one records that
  // provisioning did not work. Neither can be governed, so neither gets a
  // control the server would refuse.
  it('shows no lifecycle controls for a binding that is not settled', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-pending', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_pending', name: 'Ventende', status: 'pending',
        definition_status: 'active', delivery_targets: [],
      },
      {
        subject_id: 'agent-noref', role: 'editor', revision: 1, identity_published: true,
        name: 'Uten binding', status: 'active',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    await agentPanel.findByText('Ventende')
    expect(agentPanel.queryByRole('button', { name: 'Sett på pause' })).toBeNull()
    expect(agentPanel.queryByRole('button', { name: 'Fjern fra rommet' })).toBeNull()
  })

  // Governing a binding is the same class of decision as granting one.
  it('hides the lifecycle controls from a role that cannot grant', async () => {
    spacesClient.getSpaceContext.mockResolvedValue({
      ...personalContext,
      membership: { ...personalContext.membership, role: 'viewer' },
    })
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'active',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    await agentPanel.findByText('Driftsassistent')
    expect(agentPanel.queryByRole('button', { name: 'Sett på pause' })).toBeNull()
    expect(agentPanel.queryByRole('button', { name: 'Fjern fra rommet' })).toBeNull()
  })

  // A refused change must say the agent is unchanged, not leave the room
  // implying something happened.
  it('reports a failed pause without claiming the agent changed', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceAgents.mockResolvedValue([
      {
        subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
        binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'active',
        definition_status: 'active', delivery_targets: [],
      },
    ])
    spacesClient.setSpaceAgentState.mockRejectedValue(new Error('nope'))
    renderSpacePage()

    const agentPanel = await openAgentTab()
    ;(await agentPanel.findByRole('button', { name: 'Sett på pause' })).click()
    expect(await agentPanel.findByRole('alert')).toBeTruthy()
    expect(agentPanel.getByText(/Ingenting er endret/)).toBeTruthy()
  })

  // The room already COUNTED work that needed a person ("Oppmerksomhet" in the
  // pulse rail) and then offered nowhere to act on it. Counting a duty and
  // routing the person off the page to discharge it is the side-panel shape the
  // product model argues against.
  it('lets a paused run be decided in the room, and refreshes the room afterwards', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_gate',
        space_id: 'space_personal_1',
        title: 'Utsendelse',
        preview: 'Send oppsummeringen',
        latest_run_id: 'run_gate',
        latest_run_status: 'awaiting_approval',
        updated_at: '2026-09-06T10:00:00Z',
      }],
    })
    orchestration.listApprovals.mockResolvedValue([
      { id: 'ap-1', status: 'PENDING', detail: 'Send oppsummeringen til kunden' },
    ])
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    expect(await timeline.findByText('Send oppsummeringen til kunden')).toBeTruthy()

    const approve = timeline.getByRole('button', { name: 'Godkjenn' })
    approve.click()

    await waitFor(() => expect(orchestration.decideApproval).toHaveBeenCalled())
    expect(orchestration.decideApproval).toHaveBeenCalledWith('ap-1', 'approve', undefined, undefined)
    // Approving continues the run; the room then re-reads so the post stops
    // claiming it is waiting.
    await waitFor(() => expect(orchestration.resumeRun).toHaveBeenCalledWith('run_gate', undefined))
    await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(1))
  })

  // A thread paused on a person does not take new input: a reply would queue
  // behind a gate the same person is standing at.
  it('refuses to send into a thread that is waiting on an approval', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_gate',
        space_id: 'space_personal_1',
        title: 'Utsendelse',
        preview: 'Send oppsummeringen',
        latest_run_id: 'run_gate',
        latest_run_status: 'awaiting_approval',
        updated_at: '2026-09-06T10:00:00Z',
      }],
    })
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    timeline.getByRole('button', { name: 'Svar' }).click()

    expect(await screen.findByText('Godkjenn eller avslå for å fortsette denne samtalen.')).toBeTruthy()
    const composer = screen.getByPlaceholderText('Denne samtalen venter på en godkjenning.') as HTMLTextAreaElement
    expect(composer.disabled).toBe(true)
    expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  // A run that is merely running has nothing to decide, and a thread with no
  // run id gives the panel nothing to act on.
  it('shows no approval surface when nothing is actually gated', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [
        {
          thread_id: 'thread_running',
          space_id: 'space_personal_1',
          title: 'Pågår',
          preview: 'Jobber',
          latest_run_id: 'run_running',
          latest_run_status: 'running',
          updated_at: '2026-09-06T10:00:00Z',
        },
        {
          thread_id: 'thread_no_run',
          space_id: 'space_personal_1',
          title: 'Uten kjøring',
          preview: 'Ingen kjøring',
          latest_run_status: 'awaiting_approval',
          updated_at: '2026-09-06T10:01:00Z',
        },
      ],
    })
    renderSpacePage()

    await screen.findByRole('list', { name: 'Samtaler i rommet' })
    expect(screen.queryByText('Venter på din godkjenning')).toBeNull()
    expect(orchestration.listApprovals).not.toHaveBeenCalled()
  })

  // The regression this whole slice exists to prevent: before shared reads,
  // every readable user turn was provably the reader's own, so the timeline
  // labelled human turns with the one human in the roster. Applied to a room
  // with several members that rule puts words in the wrong person's mouth.
  it('attributes another member\'s turn to that member, never to the reader', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceRoster.mockResolvedValue([
      { subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
      { subject_type: 'user', subject_id: 'user_2', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
    ])
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_shared',
        space_id: 'space_personal_1',
        owner_subject_id: 'user_2',
        title: 'Leveranse',
        preview: 'Når kommer leveransen?',
        updated_at: '2026-09-06T10:00:00Z',
      }],
    })
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({
      threadId: 'thread_shared',
      turns: [
        { role: 'user', content: 'Når kommer leveransen?', authorSubjectId: 'user_2' },
        // No recorded author: it must stay unnamed rather than borrow one.
        { role: 'user', content: 'Uten registrert avsender.' },
      ],
    })
    renderSpacePage()

    // Await transcript-only content: the post preview shows the first turn's
    // text and the fallback header already names the thread starter, so
    // waiting on either would pass before the transcript ever resolved.
    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    expect(await timeline.findByText('Uten registrert avsender.')).toBeTruthy()
    expect(timeline.getByText('Ola Hansen')).toBeTruthy()
    expect(timeline.getByText('Ukjent avsender')).toBeTruthy()
    // The reader is user_1. Their name must not appear on a turn they did not
    // write, with or without the "(deg)" marker.
    expect(timeline.queryByText('Kari Nordmann')).toBeNull()
    expect(timeline.queryByText('Kari Nordmann (deg)')).toBeNull()
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
    flush()

    // The banner names the target and the cursor lands in the composer.
    expect(screen.getByText(/Svarer i/)).toBeTruthy()
    const composer = screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.')
    expect(document.activeElement).toBe(composer)

    fireEvent.input(composer, { target: { value: 'Og lagerstatus?' } })
    flush()
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
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett en agent' }))

    const dialog = within(await screen.findByRole('dialog', { name: 'Opprett en agent' }))
    fireEvent.click(dialog.getByRole('button', { name: /Møtereferent/ }))
    flush()
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
    flush()
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
    flush()
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
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Avbryt svar' }))
    flush()
    expect(screen.queryByText(/Svarer i/)).toBeNull()

    fireEvent.input(screen.getByPlaceholderText('Skriv i rommet. Skriv @ for å nevne noen.'), {
      target: { value: 'Ny sak' },
    })
    flush()
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
      flush()
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
      flush()
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
      flush()
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
      flush()
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
      flush()
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
      flush()
      fireEvent.click(screen.getByRole('button', { name: 'Send' }))

      await waitFor(() =>
        expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(callsBeforeSend),
      )
    })
  })
})
