import { beforeEach, vi } from 'vitest'

Object.defineProperty(window, 'scrollTo', {
  configurable: true,
  value: vi.fn(),
  writable: true,
})

// Default global fetch for the test environment. Any request a test does not
// explicitly stub returns an empty 200 JSON body. This keeps stray / relative-URL
// calls — e.g. PR-6's `/api/v1/ownership/status` honesty-gate probe, which can
// fire outside a test's own fetch stub — from reaching Node's real undici fetch,
// which throws "Failed to parse URL" on a relative path (and would otherwise make
// a data-load `catch` render an empty page). Tests that need real payloads still
// override this via `vi.stubGlobal('fetch', ...)`.
const defaultFetch = vi.fn(
  async () => new Response('{}', { headers: { 'Content-Type': 'application/json' }, status: 200 }),
) as unknown as typeof fetch

globalThis.fetch = defaultFetch

beforeEach(() => {
  globalThis.fetch = defaultFetch
})
