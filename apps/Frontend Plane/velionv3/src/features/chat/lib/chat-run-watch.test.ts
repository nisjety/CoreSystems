import { beforeEach, describe, expect, it } from 'vitest'
import {
  CHAT_RUN_PANEL_COLLAPSED_KEY,
  applyBrowserAction,
  applyBrowserObservation,
  browserSessionFromChatRun,
  closeChatRunWatch,
  emptyChatRunWatch,
  hasBrowserFrames,
  isFetchableArtifactRef,
  markScreenshotFailed,
  readChatRunPanelCollapsed,
  runArtifactUrl,
  screenshotStateFor,
  writeChatRunPanelCollapsed,
  type ChatRunWatchState,
} from './chat-run-watch'

function watched(): ChatRunWatchState {
  return emptyChatRunWatch('run_abc123', false)
}

describe('runArtifactUrl', () => {
  it('points a screenshot reference at the BFF browser-artifact route', () => {
    expect(runArtifactUrl('run_abc123', 'art_shot_1'))
      .toBe('/api/v1/browser/sessions/run_abc123/artifacts/art_shot_1')
  })

  it('refuses references the gateway would reject rather than firing a doomed request', () => {
    expect(isFetchableArtifactRef('art_shot_1')).toBe(true)
    expect(isFetchableArtifactRef('shot_1')).toBe(false)
    expect(isFetchableArtifactRef('art_')).toBe(false)
    expect(isFetchableArtifactRef('art_../../etc/passwd')).toBe(false)
    expect(runArtifactUrl('run_abc123', 'shot_1')).toBeUndefined()
    expect(runArtifactUrl('', 'art_shot_1')).toBeUndefined()
  })
})

describe('chat run reduction', () => {
  it('numbers browser steps by arrival and merges the observation into its action', () => {
    let state = applyBrowserAction(watched(), {
      actionId: 'act_0001',
      actionType: 'goto',
      reason: 'Åpne forsiden',
      url: 'https://example.com',
    })
    state = applyBrowserAction(state, { actionId: 'act_0002', actionType: 'click' })
    state = applyBrowserObservation(state, {
      actionId: 'act_0001',
      pageTitle: 'Example Domain',
      pageUrl: 'https://example.com/',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })

    expect(state.steps.map((step) => step.step)).toEqual([1, 2])
    expect(state.steps[0]).toMatchObject({
      actionType: 'goto',
      observed: true,
      pageTitle: 'Example Domain',
      reason: 'Åpne forsiden',
      screenshotUrl: '/api/v1/browser/sessions/run_abc123/artifacts/art_shot_1',
      status: 'success',
    })
    expect(state.steps[1]?.observed).toBe(false)
    expect(hasBrowserFrames(state)).toBe(true)
  })

  it('creates a step for an observation whose action was never seen', () => {
    const state = applyBrowserObservation(watched(), {
      actionId: 'act_0009',
      pageUrl: 'https://example.com/late',
      status: 'success',
    })
    expect(state.steps).toHaveLength(1)
    expect(state.steps[0]).toMatchObject({ observed: true, step: 1 })
  })
})

describe('screenshotStateFor', () => {
  const dispatched = () => applyBrowserAction(watched(), { actionId: 'act_0001', actionType: 'goto' })

  it('reports "pending" only while the run is still open', () => {
    const state = dispatched()
    const step = state.steps[0]!
    expect(screenshotStateFor(step, { live: true, zdr: false })).toBe('pending')
    expect(screenshotStateFor(step, { live: false, zdr: false })).toBe('unavailable')
  })

  it('reports "withheld" for a ZDR turn — the capture is gated server-side, so none is coming', () => {
    const state = applyBrowserObservation(emptyChatRunWatch('run_abc123', true), {
      actionId: 'act_0001',
      pageTitle: 'Intern side',
      pageUrl: 'https://example.com/',
      status: 'success',
    })
    expect(screenshotStateFor(state.steps[0]!, { live: true, zdr: true })).toBe('withheld')
  })

  it('reports "unavailable" when the observation itself carried no reference', () => {
    const state = applyBrowserObservation(dispatched(), { actionId: 'act_0001', status: 'success' })
    expect(screenshotStateFor(state.steps[0]!, { live: true, zdr: false })).toBe('unavailable')
  })

  it('reports "failed" once a referenced screenshot could not be loaded', () => {
    const observed = applyBrowserObservation(dispatched(), {
      actionId: 'act_0001',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    expect(screenshotStateFor(observed.steps[0]!, { live: true, zdr: false })).toBe('ready')

    const failed = markScreenshotFailed(observed, 'act_0001')
    expect(screenshotStateFor(failed.steps[0]!, { live: true, zdr: false })).toBe('failed')
    expect(hasBrowserFrames(failed)).toBe(false)
  })

  it('reports "failed" for a reference the gateway would reject', () => {
    const state = applyBrowserObservation(dispatched(), {
      actionId: 'act_0001',
      screenshotRef: 'not-an-artifact',
      status: 'success',
    })
    expect(state.steps[0]?.screenshotUrl).toBeUndefined()
    expect(screenshotStateFor(state.steps[0]!, { live: true, zdr: false })).toBe('failed')
  })
})

describe('browserSessionFromChatRun', () => {
  it('feeds BrowserChrome the latest artifact frame and a per-step timeline', () => {
    let state = applyBrowserObservation(watched(), {
      actionId: 'act_0001',
      pageTitle: 'First',
      pageUrl: 'https://example.com/one',
      screenshotRef: 'art_shot_1',
      status: 'success',
    })
    state = applyBrowserObservation(state, {
      actionId: 'act_0002',
      pageTitle: 'Second',
      pageUrl: 'https://example.com/two',
      screenshotRef: 'art_shot_2',
      status: 'success',
    })

    const session = browserSessionFromChatRun(state)
    expect(session.frameUrl).toBe('/api/v1/browser/sessions/run_abc123/artifacts/art_shot_2')
    expect(session.timeline.map((entry) => entry.step)).toEqual([1, 2])
    expect(session.timeline[0]?.screenshotUrl)
      .toBe('/api/v1/browser/sessions/run_abc123/artifacts/art_shot_1')
    expect(session.host).toBe('example.com')
    expect(session.renderMode).toBe('chromium')
  })

  it('leaves every live-frame transport unset so none of BrowserChrome\'s tiers arm', () => {
    // A chat turn's browser runs inside execution-core, not behind a
    // gateway-registered live-frame lease. `BrowserChrome` selects WebSocket →
    // SSE → polling purely from these three urls; all-null means it stays on
    // the honest per-step artifact frame instead of opening sockets that would
    // fail. This guards that contract, without modifying the selection logic.
    const session = browserSessionFromChatRun(watched())
    expect(session.liveFrameWsUrl).toBeNull()
    expect(session.liveFrameStreamUrl).toBeNull()
    expect(session.liveFrameUrl).toBeNull()
    // No gateway browser session backs a chat run, so manual controls stay off.
    expect(session.sessionId).toBeUndefined()
  })

  it('propagates ZDR and the stream lifecycle into the session status', () => {
    const zdrState = emptyChatRunWatch('run_abc123', true)
    expect(browserSessionFromChatRun(zdrState).zdr).toBe(true)
    expect(browserSessionFromChatRun(zdrState).status).toBe('live')
    expect(browserSessionFromChatRun(closeChatRunWatch(zdrState)).status).toBe('closed')
    expect(browserSessionFromChatRun(closeChatRunWatch(zdrState, 'boom')).status).toBe('degraded')
  })
})

describe('run panel collapse persistence', () => {
  beforeEach(() => {
    window.localStorage.removeItem(CHAT_RUN_PANEL_COLLAPSED_KEY)
    window.sessionStorage.removeItem(CHAT_RUN_PANEL_COLLAPSED_KEY)
  })

  it('defaults to expanded and round-trips the collapsed choice', () => {
    expect(readChatRunPanelCollapsed()).toBe(false)
    writeChatRunPanelCollapsed(true)
    expect(readChatRunPanelCollapsed()).toBe(true)
    writeChatRunPanelCollapsed(false)
    expect(readChatRunPanelCollapsed()).toBe(false)
  })
})
