import { expect, test } from '@playwright/test'

type BrowserSession = {
  capabilities?: string[]
  control?: { mode?: string } | null
  devtools?: { eventCount?: number; events?: Array<Record<string, unknown>>; lastSequence?: number | null } | null
  devtoolsUrl?: string | null
  frame?: { url?: string | null } | null
  id: string
  liveFrameUrl?: string | null
  liveFrameWsUrl?: string | null
  replay?: { eventCount?: number; events?: Array<Record<string, unknown>> } | null
  tabs?: Array<{ active: boolean; tabId: string; title?: string | null; url?: string | null }>
  tabsUrl?: string | null
  timeline?: Array<Record<string, unknown>>
  title?: string
  url?: string
}

type BrowserSessionResponse = {
  observation?: Record<string, unknown> | null
  session: BrowserSession
}

type BrowserTabsResponse = {
  session?: BrowserSession
  tabs: BrowserSession['tabs']
}

type BrowserDevtoolsResponse = {
  events: Array<Record<string, unknown>>
  session?: BrowserSession
  zdr?: boolean
}

function websocketUrl(baseUrl: string, rawUrl: string): string {
  const url = new URL(rawUrl, baseUrl)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  url.searchParams.set('format', 'jpeg')
  url.searchParams.set('quality', '55')
  url.searchParams.set('intervalMs', '150')
  url.searchParams.set('maxFrames', '8')
  return url.toString()
}

test('live browser session streams frames, tabs, WS actions, DevTools, and replay', async ({ page, baseURL }) => {
  test.setTimeout(120_000)
  const origin = baseURL ?? 'http://localhost:5173'
  const api = page.request

  const created = await api.post('/api/v1/browser/sessions', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      url: 'https://example.com/',
      viewport: { width: 960, height: 640 },
    },
  })
  expect(created.status(), await created.text()).toBe(200)
  const createdBody = await created.json() as { data: BrowserSessionResponse }
  const session = createdBody.data.session

  try {
    expect(session.status ?? 'live').toBe('live')
    expect(session.renderMode ?? 'chromium').toBe('chromium')
    expect(session.id).toBeTruthy()
    expect(session.liveFrameUrl).toBeTruthy()
    expect(session.liveFrameWsUrl).toBeTruthy()
    expect(session.tabsUrl).toBeTruthy()
    expect(session.devtoolsUrl).toBeTruthy()
    expect(session.capabilities ?? []).toEqual(expect.arrayContaining([
      'tabs',
      'live_frame',
      'live_frame_ws',
      'devtools_events',
      'devtools_stream',
      'human_takeover',
      'agent_control',
      'replay_timeline',
    ]))

    const frame = await api.get(session.liveFrameUrl!)
    expect(frame.status(), await frame.text()).toBe(200)
    expect(frame.headers()['content-type']).toMatch(/^image\/(jpeg|png)/)

    const newTab = await api.post(session.tabsUrl!, {
      headers: { 'content-type': 'application/json', origin },
      data: {},
    })
    expect(newTab.status(), await newTab.text()).toBe(200)
    const tabBody = await newTab.json() as { data: BrowserTabsResponse }
    expect(tabBody.data.tabs?.length ?? 0).toBeGreaterThanOrEqual(2)
    const activeTab = tabBody.data.tabs?.find((tab) => tab.active)
    expect(activeTab?.tabId).toBeTruthy()
    const backgroundTab = tabBody.data.tabs?.find((tab) => !tab.active)
    expect(backgroundTab?.tabId).toBeTruthy()

    const selectedTab = await api.post(`${session.tabsUrl!}/${encodeURIComponent(backgroundTab!.tabId)}/select`, {
      headers: { origin },
    })
    expect(selectedTab.status(), await selectedTab.text()).toBe(200)
    const selectedTabBody = await selectedTab.json() as { data: BrowserTabsResponse }
    expect(selectedTabBody.data.tabs?.find((tab) => tab.tabId === backgroundTab!.tabId)?.active).toBe(true)

    const closedTab = await api.delete(`${session.tabsUrl!}/${encodeURIComponent(activeTab!.tabId)}`, {
      headers: { origin },
    })
    expect(closedTab.status(), await closedTab.text()).toBe(200)
    const closedTabBody = await closedTab.json() as { data: BrowserTabsResponse }
    expect(closedTabBody.data.tabs?.some((tab) => tab.tabId === activeTab!.tabId)).toBe(false)
    expect(closedTabBody.data.tabs?.find((tab) => tab.tabId === backgroundTab!.tabId)?.active).toBe(true)
    expect(closedTabBody.data.session?.replay?.events?.map((event) => event.kind)).toEqual(expect.arrayContaining(['tab']))

    await page.goto(origin, { waitUntil: 'domcontentloaded' })

    const wsResult = await page.evaluate(async ({ wsUrl }) => {
      return await new Promise<{
        agentObservationMode: string | null
        controlReleaseMode: string | null
        devtoolsEvents: number
        devtoolsSessionEvents: number
        errors: string[]
        frameReplayPersisted: boolean | null
        frames: number
        observations: number
        observationStep: number | null
        replayEvents: number
        replayKinds: string[]
        sessionControlMode: string | null
        wsUrl: string
      }>((resolve) => {
        const socket = new WebSocket(wsUrl)
        const startedAt = Date.now()
        const errors: string[] = []
        let actionSent = false
        let agentActionSent = false
        let controlReleaseSent = false
        let agentObservationMode: string | null = null
        let controlReleaseMode: string | null = null
        let devtoolsEvents = 0
        let devtoolsSessionEvents = 0
        let frameReplayPersisted: boolean | null = null
        let frames = 0
        let observations = 0
        let observationStep: number | null = null
        let replayEvents = 0
        let replayKinds: string[] = []
        let sessionControlMode: string | null = null

        const finish = () => {
          if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
            socket.close()
          }
          resolve({
            agentObservationMode,
            controlReleaseMode,
            devtoolsEvents,
            devtoolsSessionEvents,
            errors,
            frameReplayPersisted,
            frames,
            observations,
            observationStep,
            replayEvents,
            replayKinds,
            sessionControlMode,
            wsUrl,
          })
        }

        const timeout = window.setTimeout(() => {
          errors.push(`timeout after ${Date.now() - startedAt}ms`)
          finish()
        }, 45_000)

        socket.onerror = () => errors.push('websocket error')
        socket.onclose = (event) => {
          errors.push(`websocket closed code=${event.code} reason=${event.reason || '<empty>'}`)
          window.clearTimeout(timeout)
          finish()
        }
        socket.onopen = () => {
          socket.send(JSON.stringify({ type: 'ping' }))
        }
        socket.onmessage = (event) => {
          const message = JSON.parse(String(event.data)) as {
            dataBase64?: string
            events?: Array<Record<string, unknown>>
            message?: string
            observation?: { step?: number }
            session?: {
              control?: { mode?: string }
            replay?: { eventCount?: number; events?: Array<{ kind?: string }> }
            devtools?: { eventCount?: number; events?: Array<unknown> }
            }
            type?: string
          }
          if (message.type === 'error') {
            errors.push(message.message ?? 'unknown websocket error')
          }
          if (message.type === 'frame' && message.dataBase64) {
            frames += 1
            if (message.session?.replay?.events?.length) {
              replayEvents = message.session.replay.eventCount ?? message.session.replay.events.length
              replayKinds = message.session.replay.events.map((entry) => String(entry.kind ?? ''))
              const frameEvent = message.session.replay.events.find((entry) => entry.kind === 'frame') as { persisted?: boolean } | undefined
              frameReplayPersisted = frameEvent?.persisted ?? frameReplayPersisted
            }
            if (!actionSent) {
              actionSent = true
              socket.send(JSON.stringify({
                type: 'action',
                actor: 'human',
                action: { type: 'wait', ms: 50 },
                instruction: 'Playwright live browser smoke wait.',
              }))
            }
          }
          if (message.type === 'devtools') {
            devtoolsEvents += message.events?.length ?? 0
            devtoolsSessionEvents = message.session?.devtools?.eventCount ?? message.session?.devtools?.events?.length ?? devtoolsSessionEvents
            replayEvents = message.session?.replay?.eventCount ?? message.session?.replay?.events?.length ?? replayEvents
            replayKinds = message.session?.replay?.events?.map((entry) => String(entry.kind ?? '')) ?? replayKinds
          }
          if (message.type === 'control') {
            controlReleaseMode = message.session?.control?.mode ?? null
            replayEvents = message.session?.replay?.eventCount ?? message.session?.replay?.events?.length ?? replayEvents
            replayKinds = message.session?.replay?.events?.map((entry) => String(entry.kind ?? '')) ?? replayKinds
            if (controlReleaseMode === 'agent_control' && !agentActionSent) {
              agentActionSent = true
              socket.send(JSON.stringify({
                type: 'action',
                actor: 'agent',
                action: { type: 'wait', ms: 25 },
                instruction: 'Agent action after human release.',
              }))
            }
          }
          if (message.type === 'observation' && message.observation) {
            observations += 1
            observationStep = message.observation.step ?? null
            sessionControlMode = message.session?.control?.mode ?? null
            replayEvents = message.session?.replay?.eventCount ?? message.session?.replay?.events?.length ?? 0
            replayKinds = message.session?.replay?.events?.map((entry) => String(entry.kind ?? '')) ?? []
            if (sessionControlMode === 'human_takeover' && !controlReleaseSent) {
              controlReleaseSent = true
              socket.send(JSON.stringify({
                type: 'control',
                actor: 'human',
                mode: 'agent_control',
              }))
            } else if (agentActionSent && sessionControlMode === 'agent_control') {
              agentObservationMode = sessionControlMode
            }
          }
          if (frames >= 2 && observations >= 2 && agentObservationMode === 'agent_control' && replayEvents > 1) {
            window.clearTimeout(timeout)
            finish()
          }
        }
      })
    }, { wsUrl: websocketUrl(process.env.E2E_BROWSER_WS_BASE_URL ?? origin, session.liveFrameWsUrl!) })

    expect(wsResult.errors).toEqual([])
    expect(wsResult.frames).toBeGreaterThanOrEqual(2)
    expect(wsResult.observations).toBeGreaterThanOrEqual(2)
    expect(wsResult.observationStep).not.toBeNull()
    expect(wsResult.controlReleaseMode).toBe('agent_control')
    expect(wsResult.agentObservationMode).toBe('agent_control')
    expect(wsResult.sessionControlMode).toBe('agent_control')
    expect(wsResult.replayEvents).toBeGreaterThan(0)
    expect(wsResult.replayKinds).toEqual(expect.arrayContaining(['control', 'devtools', 'frame', 'observation']))
    expect(wsResult.frameReplayPersisted).toBe(false)
    expect(wsResult.devtoolsEvents).toBeGreaterThan(0)
    expect(wsResult.devtoolsSessionEvents).toBeGreaterThan(0)

    const devtools = await api.get(`${session.devtoolsUrl!}?afterSequence=0&limit=64`)
    expect(devtools.status(), await devtools.text()).toBe(200)
    const devtoolsBody = await devtools.json() as { data: BrowserDevtoolsResponse }
    expect(devtoolsBody.data.events.length).toBeGreaterThan(0)
    expect(devtoolsBody.data.session?.devtools?.eventCount ?? 0).toBeGreaterThan(0)
    expect(devtoolsBody.data.session?.replay?.events?.map((event) => event.kind)).toEqual(expect.arrayContaining(['devtools']))
  } finally {
    await api.delete(`/api/v1/browser/sessions/${encodeURIComponent(session.id)}`).catch(() => undefined)
  }
})
