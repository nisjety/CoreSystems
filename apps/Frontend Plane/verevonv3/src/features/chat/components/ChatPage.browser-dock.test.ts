/**
 * The chat page's browser-dock wiring (WS9.G), pinned against the source.
 *
 * The full page has no cheap unit harness (see ChatPage.voice-first-turn.test.tsx), so the
 * class list, the mounts and the controller's record are read as text, the way the rail
 * geometry and work-content guards read them. Each assertion names a rule the dock relies on:
 * one class per visible layout, no class while hidden or while no run is open, the picker
 * mounted once through a memo, the wide-research cards still mounted, the chords wired to the
 * page, and the controller writing the dock record instead of the old collapse key.
 */

import { describe, expect, it } from 'vitest'

async function read(relative: string): Promise<string> {
  const { readFileSync } = await import('node:fs')
  const { resolve } = await import('node:path')
  return readFileSync(resolve(process.cwd(), relative), 'utf8')
}

const PAGE = 'src/features/chat/components/ChatPage.tsx'
const CONTROLLER = 'src/features/chat/components/use-chat-controller.ts'
const RUN_WATCH = 'src/features/chat/lib/chat-run-watch.ts'

describe('ChatPage browser dock class list', () => {
  it('takes one class per visible layout beside the existing rail classes', async () => {
    const page = await read(PAGE)
    expect(page).toContain("'verevon-chat-page--dock-right': dockLayout() === 'right',")
    expect(page).toContain("'verevon-chat-page--dock-bottom': dockLayout() === 'bottom',")
    expect(page).toContain("'verevon-chat-page--dock-maximised': dockLayout() === 'maximised',")
    expect(page).toContain("'verevon-chat-page--split': (Boolean(liveRunId()) && !runPanelCollapsed()) || browserBodyShown(),")
    expect(page).toContain("'verevon-chat-page--railed': Boolean(liveRunId()) && runPanelCollapsed(),")
    expect(page).toContain("'verevon-chat-page--canvas': activeTab() !== 'chat',")
  })

  it('derives the layout from the stored record, behind the flag and a live run', async () => {
    const page = await read(PAGE)
    expect(page).toContain('const dockLayout = createMemo<BrowserDockVisiblePosition | null>(() => {')
    // Hidden, flag off and no live run each give no dock class.
    expect(page).toContain("if (!isChatBrowserDockEnabled() || !liveRunId() || position === 'hidden') return null")
  })

  it('sizes the bottom dock from the stored share of the page, only while a dock applies', async () => {
    const page = await read(PAGE)
    expect(page).toContain(
      "style={dockLayout() ? { '--browser-dock-size': browserDockSizeCss(browserDock()) } : undefined}",
    )
  })
})

describe('ChatPage browser dock mounts', () => {
  it('mounts the picker through one flag-gated memo, never an inline prop', async () => {
    const page = await read(PAGE)
    expect(page).toContain('const liveRailDockControls = createMemo(() => (')
    expect(page).toContain('isChatBrowserDockEnabled() && liveRunId()')
    expect(page).toContain('dockControls={liveRailDockControls()}')
    expect(page).not.toContain('dockControls={isChatBrowserDockEnabled()')
  })

  it('keeps the wide-research cards mounted in the rail', async () => {
    const page = await read(PAGE)
    expect(page).toContain('const liveRailWideResearch = createMemo(() => <ChatWideResearchPanel runId={liveRunId()} threadId={state.threadId ?? null} onRetryItem={wideResearchOffered() ? retryWideResearchItem : undefined} />)')
    expect(page).toContain('wideResearch={liveRailWideResearch()}')
  })

  it('wires the two dock chords to the page, each gated on a live run', async () => {
    const page = await read(PAGE)
    expect(page).toContain('toggleBrowserDock: () => {')
    expect(page).toContain('cycleBrowserDock: () => {')
    expect(page).toContain('if (liveRunId()) toggleRunPanel()')
    expect(page).toContain('setBrowserDockPosition(nextBrowserDockPosition(browserDock().position))')
  })
})

describe('chat controller dock record', () => {
  it('writes the dock record and no longer the old collapse key', async () => {
    const controller = await read(CONTROLLER)
    expect(controller).toContain('const [browserDock, updateBrowserDock] = useBrowserDock()')
    expect(controller).toContain("const runPanelCollapsed = () => browserDock().position === 'hidden'")
    expect(controller).toContain('const toggleRunPanel = () => {')
    expect(controller).not.toContain('writeChatRunPanelCollapsed')
    expect(controller).not.toContain('readChatRunPanelCollapsed')
  })

  it('keeps the old collapse key exported and read-only for the one-time migration', async () => {
    const watch = await read(RUN_WATCH)
    expect(watch).toContain("export const CHAT_RUN_PANEL_COLLAPSED_KEY = 'verevon.chat.runPanel.collapsed.v1'")
    expect(watch).toContain('export function readChatRunPanelCollapsed(): boolean {')
  })
})
