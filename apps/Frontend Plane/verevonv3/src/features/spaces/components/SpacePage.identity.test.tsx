import { createRouter, memoryHistory } from '@solidjs/router'
import { render, screen, within } from '@solidjs/testing-library'
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

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  searchSpaceMessages: () => new Promise(() => {}),
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

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/orchestration-client')>()),
  ...orchestration,
}))

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
describe('SpacePage identity (WS07-S7, WS07-S8)', () => {
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

  const BLUE = 'rgb(37, 99, 235)'

  const driftAgent = (overrides: Record<string, unknown> = {}) => ({
    subject_id: 'agent-drift', role: 'editor', revision: 1, identity_published: true,
    binding_ref: 'sab_drift', name: 'Driftsassistent', status: 'active', definition_status: 'active',
    avatar_color: '#2563eb', delivery_targets: [], ...overrides,
  })

  const seedRoom = (agents: readonly unknown[], turns: readonly unknown[] = []) => {
    spacesClient.getSpaceContext.mockResolvedValue(personalContext)
    spacesClient.getSpaceRoster.mockResolvedValue([
      { subject_type: 'user', subject_id: 'user_1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
    ])
    spacesClient.getSpaceAgents.mockResolvedValue(agents)
    spacesClient.getSpaceThreads.mockResolvedValue({
      ...personalContext,
      threads: [{
        thread_id: 'thread_drift',
        space_id: 'space_personal_1',
        owner_subject_id: 'user_1',
        title: 'Driftsstatus',
        preview: 'Hva er status?',
        latest_run_status: 'completed',
        updated_at: '2026-08-16T10:00:00Z',
      }],
    })
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ threadId: 'thread_drift', turns })
  }

  async function openAgentTab() {
    const tab = await screen.findByRole('tab', { name: 'Agent' })
    tab.click()
    return within(await screen.findByRole('tabpanel', { name: 'Agent' }))
  }

  it('the agent card renders an AgentAvatar in the agent colour and its name is wrapped by the hover card', async () => {
    seedRoom([driftAgent()])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    const card = (await agentPanel.findByText('Driftsassistent')).closest('.verevon-space-agent') as HTMLElement
    const avatar = card.querySelector('.verevon-agent-avatar') as HTMLElement
    expect(avatar.getAttribute('data-kind')).toBe('agent')
    expect(avatar.style.background).toBe(BLUE)
    const trigger = card.querySelector('.verevon-space-agent__name .verevon-identity-trigger') as HTMLElement
    expect(trigger?.textContent).toBe('Driftsassistent')
  })

  it('an agent with an invalid stored colour renders a neutral avatar on its card', async () => {
    seedRoom([driftAgent({ avatar_color: 'red' })])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    const card = (await agentPanel.findByText('Driftsassistent')).closest('.verevon-space-agent') as HTMLElement
    const avatar = card.querySelector('.verevon-agent-avatar') as HTMLElement
    expect(avatar.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
    expect(avatar.style.background).toBe('')
  })

  it('an agent with no published identity keeps a plain, unwrapped name and a neutral avatar', async () => {
    seedRoom([
      { subject_id: 'agent-orphan', role: 'editor', revision: 1, identity_published: false, delivery_targets: [] },
    ])
    renderSpacePage()

    const agentPanel = await openAgentTab()
    const card = (await agentPanel.findByText('Agent uten publisert navn')).closest('.verevon-space-agent') as HTMLElement
    expect(card.querySelector('.verevon-identity-trigger')).toBeNull()
    expect(card.querySelector('.verevon-agent-avatar')?.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
  })

  it('the room timeline wraps the post author and the agent name in the hover card and colours the agent turn', async () => {
    seedRoom([driftAgent(), driftAgent({ subject_id: 'agent-status', name: 'Statusagent', avatar_color: '#dc2626' })], [
      { role: 'user', content: 'Hva er status?', authorSubjectId: 'user_1' },
      { role: 'assistant', content: 'Alt er grønt.', agentName: 'Driftsassistent' },
    ])
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    const agentTurn = (await timeline.findByText('Alt er grønt.')).closest('.verevon-room-turn') as HTMLElement
    expect(agentTurn.querySelector('.verevon-room-turn__author .verevon-identity-trigger')?.textContent).toBe('Driftsassistent')
    expect((agentTurn.querySelector('.verevon-agent-avatar') as HTMLElement).style.background).toBe(BLUE)

    const humanTurn = timeline.getAllByText('Hva er status?')
      .map((node) => node.closest('.verevon-room-turn') as HTMLElement)
      .find((row) => row?.querySelector('.verevon-room-turn__author .verevon-identity-trigger'))
    expect(humanTurn?.querySelector('.verevon-identity-trigger')?.textContent).toBe('Kari Nordmann (deg)')
  })

  it('two same-named agents render neutral in the timeline and get no hover card there', async () => {
    seedRoom([driftAgent(), driftAgent({ subject_id: 'agent-twin', avatar_color: '#dc2626' })], [
      { role: 'assistant', content: 'Alt er grønt.', agentName: 'Driftsassistent' },
    ])
    renderSpacePage()

    const timeline = within(await screen.findByRole('list', { name: 'Samtaler i rommet' }))
    const agentTurn = (await timeline.findByText('Alt er grønt.')).closest('.verevon-room-turn') as HTMLElement
    const avatar = agentTurn.querySelector('.verevon-agent-avatar') as HTMLElement
    expect(avatar.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
    expect(avatar.style.background).toBe('')
    expect(agentTurn.querySelector('.verevon-identity-trigger')).toBeNull()
  })
})
