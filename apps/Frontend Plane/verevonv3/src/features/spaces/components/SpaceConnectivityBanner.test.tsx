import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { noteRoomFresh, noteSendTransportLost, noteSendTransportOk, spaceConnectivity } from '../lib/space-connectivity'
import { formatTurnTime } from '../lib/space-thread-presentation'

const { SpaceConnectivityBanner } = await import('./SpaceConnectivityBanner')

// The banner is presentation only: no model, no network. The browser's own events and the send
// pipeline's transport note are the two facts it reads.
const setNavigatorOnline = (online: boolean) => {
  Object.defineProperty(window.navigator, 'onLine', { configurable: true, get: () => online })
}

beforeEach(() => {
  vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'true')
  setNavigatorOnline(true)
  noteSendTransportOk()
  window.dispatchEvent(new Event('online'))
  flush()
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  setNavigatorOnline(true)
  window.dispatchEvent(new Event('online'))
  noteSendTransportOk()
  flush()
})

// DEPTH-SPA-2 (fixed): the banner starts the watcher in onMount, and the watcher writes only on a
// change, so mounting it no longer writes a signal inside the component body (Solid 2 refuses that
// with REACTIVE_WRITE_IN_OWNED_SCOPE). The connectivity state itself is covered in
// lib/space-connectivity.test.ts.
describe('SpaceConnectivityBanner (WS01-S8)', () => {
  it('DEPTH-SPA-2: renders nothing while online', () => {
    render(() => <SpaceConnectivityBanner />)
    expect(screen.queryByRole('status')).toBeNull()
    expect(document.querySelector('.verevon-space-connectivity')).toBeNull()
  })

  it('DEPTH-SPA-2: says the user is offline after the browser goes offline', async () => {
    render(() => <SpaceConnectivityBanner />)
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    const banner = await screen.findByRole('status')
    expect(banner.textContent).toContain('Du er frakoblet')
    expect(banner.getAttribute('data-state')).toBe('offline')
    expect(banner.getAttribute('aria-live')).toBe('polite')
  })

  it('DEPTH-SPA-2: says the room is unreachable, with the retry sentence, after an unresumable send', async () => {
    render(() => <SpaceConnectivityBanner />)
    noteSendTransportLost()
    flush()
    const banner = await screen.findByRole('status')
    expect(banner.textContent).toContain('Rommet kan ikke nås')
    expect(banner.textContent).toContain('Prøv igjen')
    expect(banner.getAttribute('data-state')).toBe('unreachable')
  })

  it('DEPTH-SPA-2: offline wins over unreachable', async () => {
    render(() => <SpaceConnectivityBanner />)
    noteSendTransportLost()
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect((await screen.findByRole('status')).getAttribute('data-state')).toBe('offline')
    expect(spaceConnectivity()).toBe('offline')
  })

  it('DEPTH-SPA-2: goes away on the online event, which also clears an unreachable room', async () => {
    render(() => <SpaceConnectivityBanner />)
    noteSendTransportLost()
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    await screen.findByRole('status')
    setNavigatorOnline(true)
    window.dispatchEvent(new Event('online'))
    flush()
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
    expect(spaceConnectivity()).toBe('online')
  })

  it('DEPTH-SPA-2: goes away when a later send reaches the server', async () => {
    render(() => <SpaceConnectivityBanner />)
    noteSendTransportLost()
    flush()
    await screen.findByRole('status')
    noteSendTransportOk()
    flush()
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
  })

  it('renders nothing with the flag off, whatever the connection', () => {
    vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'false')
    noteSendTransportLost()
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    render(() => <SpaceConnectivityBanner />)
    expect(screen.queryByRole('status')).toBeNull()
    expect(document.querySelector('.verevon-space-connectivity')).toBeNull()
  })

  it('DEPTH-SPA-2: stops following browser events once it is removed', async () => {
    const view = render(() => <SpaceConnectivityBanner />)
    // The watcher starts once the component has settled, so a flush is what attaches it.
    flush()
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect(spaceConnectivity()).toBe('offline')
    setNavigatorOnline(true)
    window.dispatchEvent(new Event('online'))
    flush()
    expect(spaceConnectivity()).toBe('online')

    view.unmount()
    // The unmount runs the watcher's stop function, so the browser's offline event no longer reaches the state.
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    expect(spaceConnectivity()).toBe('online')
    expect(document.querySelector('.verevon-space-connectivity')).toBeNull()
  })
})

// WS01-S8: the sentence also dates what is on screen, by the last moment this room heard from its server.
describe('SpaceConnectivityBanner — last fresh time (WS01-S8)', () => {
  const at = Date.parse('2026-11-23T13:05:00Z')
  const iso = new Date(at).toISOString()
  const shortTime = formatTurnTime(iso, { locale: 'no' })?.short
  const fresh = () => document.querySelector('.verevon-space-connectivity__fresh')

  it('says when the room last heard from its server, beside an offline sentence', async () => {
    noteRoomFresh('banner-room-offline', at)
    flush()
    render(() => <SpaceConnectivityBanner spaceRef="banner-room-offline" />)
    setNavigatorOnline(false)
    window.dispatchEvent(new Event('offline'))
    flush()
    const banner = await screen.findByRole('status')
    expect(banner.textContent).toContain('Du er frakoblet')
    // The time sits beside the live region, not in it, so a new minute is not announced again.
    expect(banner.textContent).not.toContain('Sist oppdatert')
    expect(banner.querySelector('time')).toBeNull()
    expect(fresh()?.textContent).toContain('Sist oppdatert kl.')
    expect(fresh()?.closest('[aria-live]')).toBeNull()
    expect(fresh()?.querySelector('time')?.getAttribute('datetime')).toBe(iso)
    expect(fresh()?.querySelector('time')?.textContent).toBe(shortTime)
  })

  it('says the same for an unreachable room', async () => {
    noteRoomFresh('banner-room-unreachable', at)
    flush()
    render(() => <SpaceConnectivityBanner spaceRef="banner-room-unreachable" />)
    noteSendTransportLost()
    flush()
    const banner = await screen.findByRole('status')
    expect(banner.getAttribute('data-state')).toBe('unreachable')
    expect(fresh()?.querySelector('time')?.textContent).toBe(shortTime)
  })

  it('a room read after a failed send clears the unreachable sentence, so a newer time never sits beside it', async () => {
    render(() => <SpaceConnectivityBanner spaceRef="banner-room-recovered" />)
    noteSendTransportLost()
    flush()
    expect((await screen.findByRole('status')).getAttribute('data-state')).toBe('unreachable')
    noteRoomFresh('banner-room-recovered', at + 60_000)
    flush()
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
    expect(fresh()).toBeNull()
    expect(spaceConnectivity()).toBe('online')
  })

  it('shows no time for another room', async () => {
    noteRoomFresh('banner-room-other', at)
    flush()
    render(() => <SpaceConnectivityBanner spaceRef="banner-room-unknown" />)
    noteSendTransportLost()
    flush()
    const banner = await screen.findByRole('status')
    expect(banner.textContent).toContain('Rommet kan ikke nås')
    expect(fresh()).toBeNull()
  })

  it('shows no time when no room is named', async () => {
    noteRoomFresh('banner-room-unnamed', at)
    flush()
    render(() => <SpaceConnectivityBanner />)
    noteSendTransportLost()
    flush()
    await screen.findByRole('status')
    expect(fresh()).toBeNull()
  })

  it('keeps each room its own time, so another room heard from later does not blank this one', async () => {
    noteRoomFresh('banner-room-kept', at)
    noteRoomFresh('banner-room-later', at + 120_000)
    flush()
    render(() => <SpaceConnectivityBanner spaceRef="banner-room-kept" />)
    noteSendTransportLost()
    flush()
    await screen.findByRole('status')
    expect(fresh()?.querySelector('time')?.getAttribute('datetime')).toBe(iso)
  })
})
