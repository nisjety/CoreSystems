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
  markSpaceRead: vi.fn(),
  recordSpacePresence: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  createSpaceAgent: spacesClient.createSpaceAgent,
  bindSpaceAgent: spacesClient.bindSpaceAgent,
  getInstallableSpaceAgents: spacesClient.getInstallableSpaceAgents,
  getSpaceAgents: spacesClient.getSpaceAgents,
  getSpaceContext: spacesClient.getSpaceContext,
  getSpaceRoster: spacesClient.getSpaceRoster,
  // The page reads the roster through the view (WS04-S6), which also says whether the member writer
  // is on. Mirrors the real one over the hoisted roster read, with the writer off.
  getSpaceRosterView: async (spaceRef: string) => ({
    members: await spacesClient.getSpaceRoster(spaceRef),
    writerApi: false,
  }),
  getSpaceThreads: spacesClient.getSpaceThreads,
  getPersonalSpaceDeletionReceipt: spacesClient.getPersonalSpaceDeletionReceipt,
  listSpaces: spacesClient.listSpaces,
  requestPersonalSpaceDeletion: spacesClient.requestPersonalSpaceDeletion,
  getSpaceInstructions: spacesClient.getSpaceInstructions,
  updateSpaceInstructions: spacesClient.updateSpaceInstructions,
  getSpaceThreadTranscript: spacesClient.getSpaceThreadTranscript,
  // WS01-S6 human posts the composer imports.
  createSpacePost: spacesClient.createSpacePost,
  replyInSpaceThread: spacesClient.replyInSpaceThread,
  SPACE_POST_MAX_CHARS_DEFAULT: 8000,
  // The timeline and the artifacts tab read through loadSpaceTranscript (WS07-S4), which turns the
  // transcript read's outcome into a value. Mirrors the real one over the hoisted read.
  loadSpaceTranscript: async (spaceRef: string, threadId: string) => {
    try {
      const snapshot = await spacesClient.getSpaceThreadTranscript(spaceRef, threadId)
      return { state: 'ok' as const, threadId: snapshot.threadId, turns: snapshot.turns }
    } catch (error) {
      const status = (error as { status?: number } | null)?.status
      if (status === 403) return { state: 'restricted' as const }
      if (status === 404) return { state: 'missing' as const }
      return { state: 'error' as const }
    }
  },
  setSpaceAgentState: spacesClient.setSpaceAgentState,
  revokeSpaceAgent: spacesClient.revokeSpaceAgent,
  getSpaceWork: spacesClient.getSpaceWork,
  getSpaceKnowledge: spacesClient.getSpaceKnowledge,
  getSpaceActivity: spacesClient.getSpaceActivity,
  markSpaceRead: spacesClient.markSpaceRead,
  recordSpacePresence: spacesClient.recordSpacePresence,
  updateSpaceThreadPresentation: spacesClient.updateSpaceThreadPresentation,
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
  cancelInvocation: vi.fn(),
}))

// Partial mock: spread the real module so every other export (constants like
// VEREVON_BALANCE_MODE_ID, helpers the composer imports) stays real. A
// complete-replacement mock here has broken before as chat-client's surface
// grew — see the getSpaceInstructions comment above for the same lesson on
// spaces-client.
vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: chatClient.streamChat,
    getChatThreadTranscript: chatClient.getChatThreadTranscript,
    cancelInvocation: chatClient.cancelInvocation,
  }
})

// WS02-S5: the live stream controller is replaced by a recorder the tests drive by hand (its own
// behaviour is covered in lib/space-stream.test.ts). Nothing here opens a connection.
type StreamMockInstance = {
  options: { spaceRef: string; onChange: () => void; onState?: (state: string) => void }
  start: ReturnType<typeof vi.fn>
  stop: ReturnType<typeof vi.fn>
  rearm: ReturnType<typeof vi.fn>
  state: () => string
}
const streamMock = vi.hoisted(() => ({ instances: [] as unknown[] }) as { instances: StreamMockInstance[] })
vi.mock('../lib/space-stream', () => ({
  createSpaceStreamController: (options: StreamMockInstance['options']) => {
    const instance: StreamMockInstance = {
      options,
      start: vi.fn(),
      stop: vi.fn(),
      rearm: vi.fn(),
      state: () => 'idle',
    }
    streamMock.instances.push(instance)
    return instance
  },
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
    // DEPTH-SPA-2: the connectivity banner the composer mounts writes a signal inside its component
    // (REACTIVE_WRITE_IN_OWNED_SCOPE), which halts reactivity for the file. Off here; it has its own switch.
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'false')
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
    spacesClient.markSpaceRead.mockReset()
    spacesClient.recordSpacePresence.mockReset()
    spacesClient.recordSpacePresence.mockResolvedValue({ present: [], ttl_seconds: 30 })
    spacesClient.markSpaceRead.mockResolvedValue({ last_read_at: 1 })
    spacesClient.updateSpaceThreadPresentation.mockReset()
    spacesClient.updateSpaceThreadPresentation.mockResolvedValue({ thread_id: 't' })
    spacesClient.createSpacePost.mockReset()
    spacesClient.replyInSpaceThread.mockReset()
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

  afterEach(() => {
    vi.unstubAllEnvs()
    vi.restoreAllMocks()
  })

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
    fireEvent.input(screen.getByPlaceholderText('Skriv i rommet. @ nevner noen, / velger en ferdighet.'), {
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

    const composer = screen.getByPlaceholderText('Skriv i rommet. @ nevner noen, / velger en ferdighet.')
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
    const composer = screen.getByPlaceholderText('Skriv i rommet. @ nevner noen, / velger en ferdighet.')
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

    fireEvent.input(screen.getByPlaceholderText('Skriv i rommet. @ nevner noen, / velger en ferdighet.'), {
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
    const composerPlaceholder = 'Skriv i rommet. @ nevner noen, / velger en ferdighet.'

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

  // Regression: the Agent tab's instructions were fetched ten times per Space,
  // and again on every 30s membership recheck.
  //
  // `SpacePage` passes `tabs={{ chat: <.../>, agent: <.../>, ... }}` to
  // `SpaceCockpit`. Solid's JSX compiler wraps a dynamic prop expression in a
  // getter, so that object literal compiled to `get tabs() { return { ... } }`
  // — a factory, not a value. Every *read* of `props.tabs` re-ran
  // `createComponent` for every panel, so every read mounted a fresh
  // `SpaceInstructionsSection` (a `createResource` on mount => one GET) and a
  // fresh `SpaceRoomComposer` (losing whatever was typed into it).
  //
  // `SpaceCockpit` reads `props.tabs` twice per panel — once for `<Show>`'s
  // `when`, once for its `children` — over six tabs, of which four are
  // supplied: 6 `when` reads + 4 `children` reads = the ten observed requests.
  // Both reads sit inside memos that track `current()`, so the 30s recheck
  // resolving a fresh-but-equal context object replayed the whole burst.
  //
  // These tests pin the count and the composer's survival rather than the
  // mechanism, so they keep holding if the shell's plumbing changes again.
  describe('panel identity across context rechecks', () => {
    const composerPlaceholder = 'Skriv i rommet. @ nevner noen, / velger en ferdighet.'

    // The real gateway returns a freshly parsed object on every call, so a
    // recheck always changes the context's identity even when nothing about
    // the membership changed. `mockResolvedValue` would hand back one shared
    // reference and hide exactly the churn under test.
    function freshContext() {
      return structuredClone(personalContext)
    }

    // A refetch triggered by the recheck would be queued, not synchronous, so
    // `flush()` alone could let these assertions pass vacuously. Yield to the
    // macrotask queue first, then flush, so a stray fetch has actually landed
    // in the mock by the time it is counted.
    async function settle() {
      await new Promise((resolve) => setTimeout(resolve, 0))
      flush()
    }

    /** Captures the 30s membership recheck so a test can run it on demand. */
    function captureRecheck(): () => () => void {
      let recheck: (() => void) | undefined
      vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
        if (timeout === 30_000) recheck = handler as () => void
        return 1 as unknown as number
      }) as typeof window.setInterval)
      return () => {
        expect(recheck).toBeTypeOf('function')
        return recheck!
      }
    }

    it('loads the Space instructions exactly once per Space', async () => {
      spacesClient.getSpaceContext.mockImplementation(async () => freshContext())

      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      expect(spacesClient.getSpaceInstructions).toHaveBeenCalledTimes(1)
      expect(spacesClient.getSpaceInstructions).toHaveBeenCalledWith('space_personal_1')
    })

    it('does not refetch the instructions when the membership recheck resolves an equivalent context', async () => {
      spacesClient.getSpaceContext.mockImplementation(async () => freshContext())
      const getRecheck = captureRecheck()

      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })
      expect(spacesClient.getSpaceInstructions).toHaveBeenCalledTimes(1)

      getRecheck()()
      await waitFor(() => expect(spacesClient.getSpaceContext).toHaveBeenCalledTimes(2))
      await settle()

      expect(spacesClient.getSpaceInstructions).toHaveBeenCalledTimes(1)
    })

    it('keeps a half-typed room message across a membership recheck', async () => {
      spacesClient.getSpaceContext.mockImplementation(async () => freshContext())
      const getRecheck = captureRecheck()

      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom' })

      const composer = screen.getByPlaceholderText(composerPlaceholder) as HTMLTextAreaElement
      fireEvent.input(composer, { target: { value: 'halvskrevet melding' } })
      flush()

      getRecheck()()
      await waitFor(() => expect(spacesClient.getSpaceContext).toHaveBeenCalledTimes(2))
      await settle()

      // Same element, same text: a remount would replace the node and reset
      // the composer's local draft signal to ''.
      const afterRecheck = screen.getByPlaceholderText(composerPlaceholder) as HTMLTextAreaElement
      expect(afterRecheck).toBe(composer)
      expect(afterRecheck.value).toBe('halvskrevet melding')
    })
  })

  // Item 4b: "new since your last visit". The listing carries the reader's
  // arrival marker; a post another member changed after it is badged, the
  // reader's own is not, and having the room open advances the marker.
  it('badges posts new since arrival and records that the room was read', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      read_marker: { last_read_at: Date.parse('2026-09-07T10:00:00Z') },
      threads: [
        { thread_id: 'theirs', space_id: 'space_personal_1', title: 'Ny sak fra kollega', owner_subject_id: 'user_2',
          updated_at: '2026-09-07T11:00:00Z', latest_run_status: 'completed' },
        { thread_id: 'mine', space_id: 'space_personal_1', title: 'Min egen sak', owner_subject_id: 'user_1',
          updated_at: '2026-09-07T11:30:00Z', latest_run_status: 'completed' },
        { thread_id: 'old', space_id: 'space_personal_1', title: 'Gammel sak', owner_subject_id: 'user_2',
          updated_at: '2026-09-07T09:00:00Z', latest_run_status: 'completed' },
      ],
    })
    renderSpacePage()

    // Scoped to the timeline: the header's activity pulse names the same post.
    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    await timeline.findByText('Ny sak fra kollega')
    const badged = [...document.querySelectorAll('.verevon-room-post[data-unread]')].map((n) => n.textContent ?? '')
    expect(badged.length).toBe(1)
    expect(badged[0]).toContain('Ny sak fra kollega')
    await waitFor(() => expect(spacesClient.markSpaceRead).toHaveBeenCalledWith('space_personal_1'))
  })

  // A first visit has no last visit to be new since — nothing is badged, and
  // the marker is still recorded so the NEXT visit has one.
  it('badges nothing on a first visit but still records it', async () => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      read_marker: { last_read_at: null },
      threads: [
        { thread_id: 'theirs', space_id: 'space_personal_1', title: 'Første sak', owner_subject_id: 'user_2',
          updated_at: '2026-09-07T11:00:00Z', latest_run_status: 'completed' },
      ],
    })
    renderSpacePage()
    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    await timeline.findByText('Første sak')
    expect(document.querySelector('.verevon-room-post[data-unread]')).toBeNull()
    await waitFor(() => expect(spacesClient.markSpaceRead).toHaveBeenCalled())
  })


  // Presence. The room already polls; the heartbeat rides that beat and its
  // answer is who else is here, so there is no second timer and no second read.
  describe('who else is in the room', () => {
    const withRoster = () => {
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      spacesClient.getSpaceRoster.mockResolvedValue([
        { subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
        { subject_type: 'user', subject_id: 'user_2', role: 'editor', revision: 1, display_name: 'Ola Nordmann' },
      ])
    }

    it('beats presence on a visible listing and says who else is here', async () => {
      withRoster()
      spacesClient.recordSpacePresence.mockResolvedValue({
        present: [{ subject_id: 'user_2', status: 'online', last_seen_at: 1 }],
        ttl_seconds: 30,
      })
      renderSpacePage()

      await waitFor(() =>
        expect(spacesClient.recordSpacePresence).toHaveBeenCalledWith('space_personal_1', 'online'),
      )
      expect(await screen.findByText('Ola Nordmann er her nå')).toBeTruthy()
    })

    it('shows the room’s own typing line for a member who is writing', async () => {
      withRoster()
      spacesClient.recordSpacePresence.mockResolvedValue({
        present: [{ subject_id: 'user_2', status: 'typing', last_seen_at: 1 }],
        ttl_seconds: 30,
      })
      renderSpacePage()
      expect(await screen.findByText('Ola Nordmann skriver …')).toBeTruthy()
    })

    // An unreadable answer and an empty room are different facts. Rendering the
    // first as the second would tell a member they are alone.
    it('says nothing at all when presence cannot be read', async () => {
      withRoster()
      spacesClient.recordSpacePresence.mockRejectedValue(new Error('application down'))
      renderSpacePage()

      await waitFor(() => expect(spacesClient.recordSpacePresence).toHaveBeenCalled())
      // Level 1: the room has no threads, so the empty-state intro card also
      // names the room, as an h3 — a second, legitimate "Personlig rom" that
      // an unqualified query would trip over.
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(screen.queryByText(/er her nå/)).toBeNull()
      expect(screen.queryByText(/skriver …/)).toBeNull()
      expect(screen.queryByRole('alert')).toBeNull()
    })

    // The roster names people; presence only carries identifiers. A subject the
    // roster does not know is counted, never labelled with its raw id.
    it('counts a present subject the roster cannot name, and never prints the id', async () => {
      withRoster()
      spacesClient.recordSpacePresence.mockResolvedValue({
        present: [{ subject_id: 'user_ghost', status: 'online', last_seen_at: 1 }],
        ttl_seconds: 30,
      })
      renderSpacePage()
      expect(await screen.findByText('1 person er her nå')).toBeTruthy()
      expect(screen.queryByText(/user_ghost/)).toBeNull()
    })
  })

  // WS01-S7a: the side pane is a URL-addressable surface. The id in the URL only asks; the pane
  // resolves it through the Space transcript route, so every way the read can end is a named state.
  describe('the thread side pane (WS01-S7a)', () => {
    const thread = {
      thread_id: 'thread_1',
      space_id: 'space_personal_1',
      title: 'Innkjøp',
      preview: 'Forhåndsvisning',
      latest_run_status: 'completed',
      updated_at: '2026-10-27T10:00:00Z',
    }

    beforeEach(() => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [thread] })
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        threadId: 'thread_1',
        turns: [{ role: 'user', content: 'Hva koster det?', authorSubjectId: 'user_1' }],
      })
    })

    it('opens the pane for ?thread_id and names it after the thread', async () => {
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_1')
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      expect(await within(pane).findByText('Hva koster det?')).toBeTruthy()
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledWith('space_personal_1', 'thread_1')
    })

    it('opens no pane without the parameter, or with an invalid one', async () => {
      renderSpacePage('/spaces/space_personal_1?thread_id=')
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(document.querySelector('.verevon-space-auxpane')).toBeNull()
    })

    it('opens no pane while the pane is switched off', async () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'false')
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_1')
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(document.querySelector('.verevon-space-auxpane')).toBeNull()
      expect(screen.queryByRole('button', { name: /Åpne tråd/ })).toBeNull()
    })

    it('opens the pane from the post’s Open thread button, and Escape closes it', async () => {
      renderSpacePage()
      fireEvent.click(await screen.findByRole('button', { name: /Åpne tråd/ }))
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      fireEvent.keyDown(pane, { key: 'Escape' })
      flush()
      await waitFor(() => expect(document.querySelector('.verevon-space-auxpane')).toBeNull())
    })

    it('shows the restricted notice in the pane on a 403, never a sign-in message', async () => {
      const refused = Object.assign(new Error('forbidden'), { status: 403, code: 'space_read_not_authorized' })
      const { ApiError } = await import('@/shared/api/http')
      spacesClient.getSpaceThreadTranscript.mockRejectedValue(
        new ApiError(refused.message, 403, 'space_read_not_authorized'),
      )
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_1')
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      expect(await within(pane).findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
      expect(pane.textContent ?? '').not.toMatch(/logg inn/i)
    })

    it('still opens the pane for a thread the listing does not carry, and says it is not available', async () => {
      const { ApiError } = await import('@/shared/api/http')
      spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('missing', 404, 'thread_not_found'))
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_unknown')
      const pane = await screen.findByRole('complementary', { name: 'Tråd' })
      expect(await within(pane).findByText(/Samtalen er ikke tilgjengelig/)).toBeTruthy()
    })

    it('draws no reply composer in the pane while human posts are off', async () => {
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_1')
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      await within(pane).findByText('Hva koster det?')
      expect(within(pane).queryByRole('textbox')).toBeNull()
    })
  })

  // WS04-S2: the room's topic under its name, and the name/topic form for an owner or manager.
  describe('room name and topic (WS04-S2)', () => {
    const roomContext = (over: { topic?: string | null; role?: string; roomDetails?: boolean; kind?: string } = {}) => ({
      space: {
        space_ref: 'space_room_1', name: 'Leveranse', kind: over.kind ?? 'room', lifecycle: 'active',
        is_organization_room: false,
        ...(over.topic === undefined ? {} : { topic: over.topic }),
      },
      membership: {
        space_ref: 'space_room_1', org_id: 'org_1', subject_id: 'user_1', kind: over.kind ?? 'room', role: over.role ?? 'owner',
        revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
      },
      ...(over.roomDetails === undefined ? {} : { features: { room_details: over.roomDetails } }),
    })

    const open = async (context: ReturnType<typeof roomContext>) => {
      spacesClient.getSpaceContext.mockResolvedValue(context)
      spacesClient.getSpaceThreads.mockResolvedValue({ ...context, threads: [] })
      renderSpacePage('/spaces/space_room_1')
      await screen.findByRole('heading', { name: 'Leveranse', level: 1 })
    }

    it('shows the topic as a paragraph under the room name when one is set', async () => {
      await open(roomContext({ topic: 'Ukentlig levering til kundene' }))
      const topic = document.querySelector('.verevon-space-header__topic')
      expect(topic?.textContent).toBe('Ukentlig levering til kundene')
      expect(topic?.tagName).toBe('P')
    })

    it('shows no topic paragraph when the topic is null, absent or only whitespace', async () => {
      for (const topic of [null, undefined, '   ']) {
        const view = renderSpacePageForTopic(topic)
        await screen.findByRole('heading', { name: 'Leveranse', level: 1 })
        expect(document.querySelector('.verevon-space-header__topic'), String(topic)).toBeNull()
        view.unmount()
      }
    })

    function renderSpacePageForTopic(topic: string | null | undefined) {
      const context = roomContext(topic === undefined ? {} : { topic })
      spacesClient.getSpaceContext.mockResolvedValue(context)
      spacesClient.getSpaceThreads.mockResolvedValue({ ...context, threads: [] })
      return renderSpacePage('/spaces/space_room_1')
    }

    it('offers the edit form to an owner when features.room_details is on', async () => {
      await open(roomContext({ roomDetails: true }))
      expect(screen.getByRole('button', { name: 'Rediger navn og emne' })).toBeTruthy()
    })

    it('offers it to a manager too', async () => {
      await open(roomContext({ roomDetails: true, role: 'manager' }))
      expect(screen.getByRole('button', { name: 'Rediger navn og emne' })).toBeTruthy()
    })

    it('has no edit form for an editor', async () => {
      await open(roomContext({ roomDetails: true, role: 'editor' }))
      expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    })

    it('has no edit form for a viewer', async () => {
      await open(roomContext({ roomDetails: true, role: 'viewer' }))
      expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    })

    it('has no edit form on a personal Space even with the flag on', async () => {
      const context = roomContext({ roomDetails: true, kind: 'personal' })
      spacesClient.getSpaceContext.mockResolvedValue(context)
      spacesClient.getSpaceThreads.mockResolvedValue({ ...context, threads: [] })
      renderSpacePage('/spaces/space_room_1')
      await screen.findByRole('heading', { level: 1 })
      expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    })

    it('has no edit form with the flag off', async () => {
      await open(roomContext({ roomDetails: false }))
      expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    })

    it('has no edit form when the context carries no features at all', async () => {
      await open(roomContext())
      expect(screen.queryByRole('button', { name: 'Rediger navn og emne' })).toBeNull()
    })
  })

  // WS02-S5: the live stream is a transport; the page keeps the listing as the source of truth and
  // falls back to the 6 s poll whenever the stream is not live. The controller itself is covered in
  // lib/space-stream.test.ts; here it is the mock above, driven by hand.
  describe('the live stream through the page (WS02-S5)', () => {
    const LISTING_POLL_MS = 6_000
    const PRESENCE_BEAT_MS = 6_000
    const RECHECK_MS = 30_000

    // Records every interval the page starts and every one it clears, and never runs them: a test
    // runs a handler by hand, so "60 s idle" is ten calls rather than a minute of fake clock.
    function captureIntervals() {
      const created: { id: number; timeout: number | undefined; handler: () => void }[] = []
      const cleared = new Set<number>()
      let next = 100
      vi.spyOn(window, 'setInterval').mockImplementation(((handler: TimerHandler, timeout?: number) => {
        const id = next++
        created.push({ id, timeout, handler: handler as () => void })
        return id as unknown as number
      }) as typeof window.setInterval)
      vi.spyOn(window, 'clearInterval').mockImplementation(((id?: number) => {
        if (id !== undefined) cleared.add(id)
      }) as typeof window.clearInterval)
      return {
        created,
        active: (timeout: number) => created.filter((entry) => entry.timeout === timeout && !cleared.has(entry.id)),
      }
    }

    const presenceCalls = (status: 'online' | 'offline') =>
      spacesClient.recordSpacePresence.mock.calls.filter((call) => call[1] === status)

    beforeEach(() => {
      streamMock.instances.length = 0
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    })

    const controller = async () => {
      await waitFor(() => expect(streamMock.instances.length).toBeGreaterThan(0))
      return streamMock.instances[streamMock.instances.length - 1] as (typeof streamMock.instances)[number]
    }

    it('starts one stream for the room once membership is confirmed, on this room’s ref', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(streamMock.instances).toHaveLength(1)
      expect(streamMock.instances[0]?.options.spaceRef).toBe('space_personal_1')
      expect(streamMock.instances[0]?.start).toHaveBeenCalledTimes(1)
    })

    it('re-reads the listing once per stream change, and never beats presence or says offline for it', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      await waitFor(() => expect(presenceCalls('online').length).toBeGreaterThan(0))
      const threadReads = spacesClient.getSpaceThreads.mock.calls.length
      const beats = presenceCalls('online').length

      const live = await controller()
      live.options.onChange()
      live.options.onChange()
      await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(threadReads))
      flush()

      // The listing refetched; presence did not move with it, and nobody "left the room".
      expect(presenceCalls('online')).toHaveLength(beats)
      expect(presenceCalls('offline')).toHaveLength(0)
    })

    it('keeps the 6 s presence beat running on its own timer while the stream is live', async () => {
      const intervals = captureIntervals()
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const live = await controller()
      live.options.onState?.('live')
      flush()

      // Presence expires at 30 s server-side; with the listing quiet it must keep its own beat.
      expect(PRESENCE_BEAT_MS).toBeLessThan(30_000)
      const beating = intervals.active(PRESENCE_BEAT_MS)
      expect(beating).toHaveLength(1)
      const before = presenceCalls('online').length
      for (let minute = 0; minute < 10; minute += 1) beating[0]?.handler()
      expect(presenceCalls('online').length).toBe(before + 10)
      expect(presenceCalls('offline')).toHaveLength(0)
    })

    it('polls the listing every 6 s only while the stream is not live', async () => {
      const intervals = captureIntervals()
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()

      // Idle: the presence beat and the fallback poll.
      expect(intervals.active(LISTING_POLL_MS)).toHaveLength(2)

      stream.options.onState?.('live')
      flush()
      await waitFor(() => expect(intervals.active(LISTING_POLL_MS)).toHaveLength(1))

      stream.options.onState?.('degraded')
      flush()
      await waitFor(() => expect(intervals.active(LISTING_POLL_MS)).toHaveLength(2))

      stream.options.onState?.('unavailable')
      flush()
      expect(intervals.active(LISTING_POLL_MS)).toHaveLength(2)
    })

    it('the fallback poll reads the listing, and the page still refetches when the tab returns while live', async () => {
      const intervals = captureIntervals()
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()
      const reads = spacesClient.getSpaceThreads.mock.calls.length
      // Run every active 6 s timer: the presence one ignores the listing, the poll reads it.
      for (const entry of intervals.active(LISTING_POLL_MS)) entry.handler()
      await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(reads))

      stream.options.onState?.('live')
      flush()
      const liveReads = spacesClient.getSpaceThreads.mock.calls.length
      document.dispatchEvent(new Event('visibilitychange'))
      await waitFor(() => expect(spacesClient.getSpaceThreads.mock.calls.length).toBeGreaterThan(liveReads))
    })

    it('a successful membership recheck gives a stream that ended forbidden one new attempt', async () => {
      const intervals = captureIntervals()
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()
      expect(stream.rearm).not.toHaveBeenCalled()

      intervals.active(RECHECK_MS)[0]?.handler()
      await waitFor(() => expect(spacesClient.getSpaceContext).toHaveBeenCalledTimes(2))
      await waitFor(() => expect(stream.rearm).toHaveBeenCalledTimes(1))
    })

    it('a failed recheck keeps the page fail-closed, stops the stream and never rearms it', async () => {
      const intervals = captureIntervals()
      spacesClient.getSpaceContext
        .mockResolvedValueOnce(personalContext)
        .mockRejectedValueOnce(new Error('membership revoked'))
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()

      intervals.active(RECHECK_MS)[0]?.handler()
      expect(await screen.findByRole('heading', { name: 'Rom utilgjengelig' })).toBeTruthy()
      await waitFor(() => expect(stream.stop).toHaveBeenCalled())
      expect(stream.rearm).not.toHaveBeenCalled()
      expect(screen.queryByText('Live')).toBeNull()
    })

    it('says Live in the meta row for a live stream, and nothing for an idle or unavailable one', async () => {
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()
      expect(document.querySelector('.verevon-space-live')).toBeNull()

      stream.options.onState?.('live')
      flush()
      await waitFor(() => expect(document.querySelector('.verevon-space-live')?.textContent).toBe('Live'))

      stream.options.onState?.('degraded')
      flush()
      await waitFor(() =>
        expect(document.querySelector('.verevon-space-live')?.textContent).toBe('Oppdateres hvert par sekunder'),
      )

      stream.options.onState?.('unavailable')
      flush()
      await waitFor(() => expect(document.querySelector('.verevon-space-live')).toBeNull())
    })

    it('stops the stream when the page unmounts', async () => {
      const view = renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      const stream = await controller()
      view.unmount()
      expect(stream.stop).toHaveBeenCalled()
    })
  })

  // WS01-S7b: with human posts on and the pane available, Reply opens the pane and the pane holds
  // the reply composer, fixed to that thread. With human posts off nothing changes.
  describe('replying in the thread pane (WS01-S7b)', () => {
    const thread = {
      thread_id: 'thread_1',
      space_id: 'space_personal_1',
      title: 'Innkjøp',
      preview: 'Forhåndsvisning',
      latest_run_status: 'completed',
      updated_at: '2026-10-27T10:00:00Z',
    }
    const postsContext = { ...personalContext, features: { human_posts: true } }

    beforeEach(() => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
      spacesClient.getSpaceThreads.mockResolvedValue({ ...personalContext, threads: [thread] })
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        threadId: 'thread_1',
        turns: [{ role: 'user', content: 'Hva koster det?', authorSubjectId: 'user_1' }],
      })
    })

    it('with human posts on, the pane shows a composer aimed at that thread with no cancel control', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext)
      renderSpacePage('/spaces/space_personal_1?thread_id=thread_1')
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      await within(pane).findByText('Hva koster det?')
      expect(within(pane).getByRole('textbox')).toBeTruthy()
      expect(within(pane).getByText(/Svarer i/).textContent).toContain('Innkjøp')
      expect(within(pane).queryByRole('button', { name: 'Avbryt svar' })).toBeNull()
    })

    it('Reply opens the pane (?thread_id) instead of pointing the room composer', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext)
      renderSpacePage()
      const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
      expect(document.querySelector('.verevon-space-auxpane')).toBeNull()
      fireEvent.click(timeline.getByRole('button', { name: 'Svar' }))
      flush()
      const pane = await screen.findByRole('complementary', { name: 'Innkjøp' })
      expect(within(pane).getByText(/Svarer i/)).toBeTruthy()
      // The room's own composer is not the one aimed at the thread: exactly one "Svarer i" on the page.
      expect(screen.getAllByText(/Svarer i/)).toHaveLength(1)
    })

    it('with human posts off the pane has no composer and Reply points the room composer as before', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      renderSpacePage()
      const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
      fireEvent.click(timeline.getByRole('button', { name: 'Svar' }))
      flush()
      expect(await screen.findByText(/Svarer i/)).toBeTruthy()
      expect(screen.getByRole('button', { name: 'Avbryt svar' })).toBeTruthy()
      expect(document.querySelector('.verevon-space-auxpane')).toBeNull()
    })

    it('with human posts on but the pane switched off, Reply still points the room composer', async () => {
      vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'false')
      spacesClient.getSpaceContext.mockResolvedValue(postsContext)
      renderSpacePage()
      const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
      fireEvent.click(timeline.getByRole('button', { name: 'Svar' }))
      flush()
      expect(await screen.findByText(/Svarer i/)).toBeTruthy()
      expect(document.querySelector('.verevon-space-auxpane')).toBeNull()
    })

    it('draws a long thread collapsed in the room only with human posts on', async () => {
      const turns = Array.from({ length: 6 }, (_, i) => ({
        role: 'user', content: `Melding ${i + 1}`, authorSubjectId: 'user_1',
      }))
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({ threadId: 'thread_1', turns })
      spacesClient.getSpaceContext.mockResolvedValue(postsContext)
      const view = renderSpacePage()
      expect(await screen.findByRole('button', { name: '2 meldinger til i tråden' })).toBeTruthy()
      view.unmount()
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      renderSpacePage()
      expect(await screen.findByText('Melding 6')).toBeTruthy()
      expect(screen.getByText('Melding 2')).toBeTruthy()
      expect(document.querySelector('.verevon-room-post__collapsed')).toBeNull()
    })
  })

  // WS01-S6: the room context's flags, limits and role reach the composer, and the page's send
  // journal is settled by the room's own projection.
  describe('human posts through the page (WS01-S6)', () => {
    const postsContext = (role: string, limit?: number) => ({
      ...personalContext,
      membership: { ...personalContext.membership, role },
      features: { human_posts: true },
      ...(limit === undefined ? {} : { limits: { post_max_chars: limit } }),
    })

    it('leaves the composer as the single ask box when features.human_posts is absent', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(personalContext)
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
    })

    it('offers Send and Ask when features.human_posts is on and the role may post', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext('editor'))
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(screen.getAllByRole('radio').map((radio) => radio.textContent)).toEqual(['Send til rommet', 'Spør Verevon'])
    })

    it('passes membership.role through: a viewer gets a disabled box and the reason', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext('viewer'))
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      expect(await screen.findByText(/lesetilgang i dette rommet/)).toBeTruthy()
      expect((screen.getByPlaceholderText(/Skriv/) as HTMLTextAreaElement).disabled).toBe(true)
    })

    it('passes limits.post_max_chars through to the counter', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext('owner', 10))
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      fireEvent.input(screen.getByPlaceholderText(/Skriv/), { target: { value: 'x'.repeat(11) } })
      flush()
      expect((await screen.findByRole('alert')).textContent).toContain('11 / 10')
    })

    // DEPTH-SPA-1: the page's own journal is the unflushed one, so a post never leaves the composer
    // and there is no accepted entry to reconcile. Left written for the live tier.
    it.skip('DEPTH-SPA-1: an accepted post is dropped from the pending rows once the projection lists its thread', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(postsContext('editor'))
      spacesClient.createSpacePost.mockResolvedValue({
        threadId: 'thread_new', messageId: 'm1', sequence: 1, deduplicated: false,
      })
      renderSpacePage()
      await screen.findByRole('heading', { name: 'Personlig rom', level: 1 })
      fireEvent.input(screen.getByPlaceholderText(/Skriv/), { target: { value: 'Hei alle' } })
      flush()
      fireEvent.click(screen.getByRole('button', { name: 'Send' }))
      await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
      expect(await screen.findByText('Sendt')).toBeTruthy()
      spacesClient.getSpaceThreads.mockResolvedValue({
        ...personalContext,
        threads: [{ thread_id: 'thread_new', space_id: 'space_personal_1', title: 'Hei alle', preview: 'Hei alle' }],
      })
      await waitFor(() => expect(screen.queryByText('Sendt')).toBeNull())
    })
  })

  // WS07-S13 (decision WS07-D3, option A): the artifact-first rule is an opt-in snippet an editor
  // appends to the room's own instructions. The page never composes it by default.
  describe('artifact rule snippet in the Space instructions', () => {
    const RULE_NO_START = 'Når du blir bedt om en analyse, rapport, plan, dokument eller kode'
    const RULE_EN_START = 'When asked for an analysis, report, plan, document or code'
    const ADD = 'Legg til artefaktregelen'

    function contextFor(role: string) {
      return { ...personalContext, membership: { ...personalContext.membership, role } }
    }

    async function openInstructions() {
      const panel = await openAgentTab()
      const field = (await panel.findByRole('textbox', { name: 'Instrukser for rommet' })) as HTMLTextAreaElement
      return { panel, field }
    }

    it.each(['editor', 'manager', 'owner'])('shows the add button to a %s', async (role) => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor(role))
      renderSpacePage()
      const { panel } = await openInstructions()
      expect(panel.getByRole('button', { name: ADD })).toBeTruthy()
    })

    it.each(['viewer', 'member'])('hides the add button from a %s', async (role) => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor(role))
      renderSpacePage()
      const panel = await openAgentTab()
      await panel.findByRole('heading', { name: 'Instrukser for rommet' })
      expect(panel.queryByRole('button', { name: ADD })).toBeNull()
      expect(panel.queryByRole('textbox', { name: 'Instrukser for rommet' })).toBeNull()
    })

    it('appends the rule once, marks the draft dirty, and does not save by itself', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('editor'))
      spacesClient.getSpaceInstructions.mockResolvedValue('Svar kort.')
      renderSpacePage()
      const { panel, field } = await openInstructions()
      await waitFor(() => expect(field.value).toBe('Svar kort.'))
      const save = panel.getByRole('button', { name: /Lagre instrukser/ }) as HTMLButtonElement
      expect(save.disabled).toBe(true)

      const add = panel.getByRole('button', { name: ADD }) as HTMLButtonElement
      expect(add.disabled).toBe(false)
      fireEvent.click(add)
      flush()

      await waitFor(() => expect(field.value).toContain(RULE_NO_START))
      expect(field.value.startsWith('Svar kort.\n\n')).toBe(true)
      expect(field.value.split(RULE_NO_START)).toHaveLength(2)
      expect(save.disabled).toBe(false)
      expect(spacesClient.updateSpaceInstructions).not.toHaveBeenCalled()

      // Added once: the button now says so and a second click changes nothing.
      expect(add.disabled).toBe(true)
      expect(panel.getByText('Artefaktregelen er allerede lagt til.')).toBeTruthy()
      const afterFirst = field.value
      fireEvent.click(add)
      flush()
      expect(field.value).toBe(afterFirst)
    })

    it('uses the rule alone when the instructions are empty', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('owner'))
      renderSpacePage()
      const { panel, field } = await openInstructions()
      fireEvent.click(panel.getByRole('button', { name: ADD }))
      flush()
      await waitFor(() => expect(field.value.startsWith(RULE_NO_START)).toBe(true))
    })

    it('treats the English wording as already present, so a language switch cannot add a second copy', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('manager'))
      spacesClient.getSpaceInstructions.mockResolvedValue(
        `Be concise.\n\n${RULE_EN_START} longer than a short answer, create it as an artifact and reply with one or two sentences saying what it is and how it changed. Revise the same artifact instead of creating a new one.`,
      )
      renderSpacePage()
      const { panel, field } = await openInstructions()
      await waitFor(() => expect(field.value).toContain(RULE_EN_START))
      expect((panel.getByRole('button', { name: ADD }) as HTMLButtonElement).disabled).toBe(true)
      expect(panel.getByText('Artefaktregelen er allerede lagt til.')).toBeTruthy()
    })

    it('is not fooled by whitespace differences in the saved rule', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('manager'))
      spacesClient.getSpaceInstructions.mockResolvedValue(
        'Når du blir bedt om en analyse,   rapport, plan,\ndokument eller kode som er lengre enn et kort svar, lag det som en artefakt og svar med én eller to setninger om hva det er og hva som er endret. Revider den samme artefakten i stedet for å lage en ny.',
      )
      renderSpacePage()
      const { panel, field } = await openInstructions()
      await waitFor(() => expect(field.value).toContain('Når du blir bedt'))
      expect((panel.getByRole('button', { name: ADD }) as HTMLButtonElement).disabled).toBe(true)
    })

    it('disables the button with a hint when appending would pass 4000 characters', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('editor'))
      spacesClient.getSpaceInstructions.mockResolvedValue('x'.repeat(3900))
      renderSpacePage()
      const { panel, field } = await openInstructions()
      await waitFor(() => expect(field.value).toHaveLength(3900))
      expect((panel.getByRole('button', { name: ADD }) as HTMLButtonElement).disabled).toBe(true)
      expect(panel.getByText(/har ikke plass til artefaktregelen/)).toBeTruthy()
      fireEvent.click(panel.getByRole('button', { name: ADD }))
      flush()
      expect(field.value).toHaveLength(3900)
    })

    it('saves the appended draft through updateSpaceInstructions', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('editor'))
      spacesClient.getSpaceInstructions.mockResolvedValue('Svar kort.')
      renderSpacePage()
      const { panel, field } = await openInstructions()
      await waitFor(() => expect(field.value).toBe('Svar kort.'))
      fireEvent.click(panel.getByRole('button', { name: ADD }))
      flush()
      await waitFor(() => expect(field.value).toContain(RULE_NO_START))
      const draft = field.value

      fireEvent.click(panel.getByRole('button', { name: /Lagre instrukser/ }))
      await waitFor(() => expect(spacesClient.updateSpaceInstructions).toHaveBeenCalledTimes(1))
      expect(spacesClient.updateSpaceInstructions).toHaveBeenCalledWith('space_personal_1', draft)
    })

    it('never appends the rule on its own when the instructions load', async () => {
      spacesClient.getSpaceContext.mockResolvedValue(contextFor('owner'))
      spacesClient.getSpaceInstructions.mockResolvedValue('Svar kort.')
      renderSpacePage()
      const { field } = await openInstructions()
      await waitFor(() => expect(field.value).toBe('Svar kort.'))
      expect(field.value).not.toContain('artefakt')
    })
  })
})
