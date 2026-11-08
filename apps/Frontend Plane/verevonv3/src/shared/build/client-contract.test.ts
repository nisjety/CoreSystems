// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S3: the client-contract check. `isDevBuild` and the guarded reload are controlled
// here; the unsaved-work registry and chunk-recovery guard set are the real ones.
const build = vi.hoisted(() => ({ dev: false }))
const reloadOnce = vi.hoisted(() => vi.fn(() => true))

vi.mock('@/shared/build/build-info', () => ({
  BUILD_ID: 'abc123def456',
  isDevBuild: () => build.dev,
}))

vi.mock('@/app/chunk-recovery', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/app/chunk-recovery')>()
  return { ...actual, reloadOnceForStaleBuild: reloadOnce }
})

type ContractModule = typeof import('./client-contract')
type UnsavedModule = typeof import('./unsaved-work')

// The module keeps its last-check time, in-flight promise and banner signal at module level,
// so each test loads a fresh copy.
async function load(): Promise<{ contract: ContractModule; unsaved: UnsavedModule }> {
  vi.resetModules()
  const contract = await import('./client-contract')
  const unsaved = await import('./unsaved-work')
  return { contract, unsaved }
}

const envelope = (over: Record<string, unknown> = {}) => ({
  data: {
    contract: 1,
    min_contract: 1,
    policy: 'banner',
    revision: 'abc123def456',
    build_date: '2026-10-20',
    ...over,
  },
})

const jsonResponse = (body: unknown, init: { status?: number; type?: string } = {}) =>
  new Response(JSON.stringify(body), {
    status: init.status ?? 200,
    headers: { 'content-type': init.type ?? 'application/json' },
  })

let fetchMock: ReturnType<typeof vi.fn>

beforeEach(() => {
  build.dev = false
  reloadOnce.mockClear()
  fetchMock = vi.fn()
  vi.stubGlobal('fetch', fetchMock)
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('clientConfigFrom', () => {
  it('rejects anything that is not the {data} envelope', async () => {
    const { contract } = await load()
    for (const body of [
      null,
      undefined,
      'text',
      42,
      [],
      {},
      { data: null },
      { data: 'x' },
      { error: { code: 'x', message: 'y', details: {} } },
      { data: { contract: 1 } },
      { data: { contract: '1', min_contract: 1, policy: 'banner' } },
      { data: { contract: 1, min_contract: 1, policy: 'sometimes' } },
      { data: { contract: 1, min_contract: 1 } },
    ]) {
      expect(contract.clientConfigFrom(body), JSON.stringify(body)).toBeNull()
    }
  })

  it('maps a valid envelope and tolerates a missing revision or build date', async () => {
    const { contract } = await load()
    expect(contract.clientConfigFrom(envelope({ min_contract: 3, policy: 'enforce' }))).toEqual({
      contract: 1,
      minContract: 3,
      policy: 'enforce',
      revision: 'abc123def456',
      buildDate: '2026-10-20',
    })
    expect(
      contract.clientConfigFrom({ data: { contract: 1, min_contract: 1, policy: 'off' } }),
    ).toEqual({ contract: 1, minContract: 1, policy: 'off', revision: '', buildDate: '' })
  })
})

describe('contractAction', () => {
  const config = (policy: 'off' | 'banner' | 'enforce', minContract: number) => ({
    contract: 1,
    minContract,
    policy,
    revision: '',
    buildDate: '',
  })
  const none = { registered: false, dirty: false }

  it('is none when this bundle meets the minimum', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('banner', contract.CLIENT_CONTRACT), none)).toBe('none')
    expect(contract.contractAction(config('enforce', 1), { registered: true, dirty: false })).toBe(
      'none',
    )
  })

  it('is none when the policy is off, however stale the bundle is', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('off', 99), none)).toBe('none')
    expect(contract.contractAction(config('off', 99), { registered: true, dirty: false })).toBe(
      'none',
    )
  })

  it('is a banner for a stale bundle under the banner policy', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('banner', 99), none)).toBe('banner')
    expect(contract.contractAction(config('banner', 99), { registered: true, dirty: false })).toBe(
      'banner',
    )
  })

  it('degrades enforce to a banner when no probe is registered', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('enforce', 99), none)).toBe('banner')
  })

  it('degrades enforce to a banner while a probe reports unsaved work', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('enforce', 99), { registered: true, dirty: true })).toBe(
      'banner',
    )
  })

  it('reloads under enforce only with a registered, clean probe', async () => {
    const { contract } = await load()
    expect(contract.contractAction(config('enforce', 99), { registered: true, dirty: false })).toBe(
      'reload',
    )
  })

  it('uses the supplied own contract', async () => {
    const { contract } = await load()
    expect(contract.isContractStale(config('banner', 5), 4)).toBe(true)
    expect(contract.isContractStale(config('banner', 5), 5)).toBe(false)
  })
})

describe('checkClientContract', () => {
  it('never fetches in a dev build', async () => {
    build.dev = true
    const { contract } = await load()
    await contract.checkClientContract({ force: true })
    expect(fetchMock).not.toHaveBeenCalled()
    expect(contract.clientContractMismatch()).toBe(false)
  })

  it('asks the gateway uncached and without credentials, then shows the banner for a stale bundle', async () => {
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 99, policy: 'banner' })))
    const { contract } = await load()
    await contract.checkClientContract({ force: true })
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/client-config', {
      cache: 'no-store',
      credentials: 'omit',
    })
    expect(contract.clientContractMismatch()).toBe(true)
    expect(reloadOnce).not.toHaveBeenCalled()
  })

  it('shows nothing when the policy is off or the bundle is current', async () => {
    const { contract } = await load()
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 99, policy: 'off' })))
    await contract.checkClientContract({ force: true })
    expect(contract.clientContractMismatch()).toBe(false)
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 1 })))
    await contract.checkClientContract({ force: true })
    expect(contract.clientContractMismatch()).toBe(false)
  })

  it('under enforce with no probe, shows the banner and does not reload', async () => {
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 99, policy: 'enforce' })))
    const { contract } = await load()
    await contract.checkClientContract({ force: true })
    expect(contract.clientContractMismatch()).toBe(true)
    expect(reloadOnce).not.toHaveBeenCalled()
  })

  it('under enforce with a dirty probe, shows the banner and does not reload', async () => {
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 99, policy: 'enforce' })))
    const { contract, unsaved } = await load()
    const release = unsaved.registerUnsavedWork('composer', () => true)
    await contract.checkClientContract({ force: true })
    release()
    expect(contract.clientContractMismatch()).toBe(true)
    expect(reloadOnce).not.toHaveBeenCalled()
  })

  it('under enforce with a clean probe, takes the guarded reload', async () => {
    fetchMock.mockResolvedValue(jsonResponse(envelope({ min_contract: 99, policy: 'enforce' })))
    const { contract, unsaved } = await load()
    const release = unsaved.registerUnsavedWork('composer', () => false)
    await contract.checkClientContract({ force: true })
    release()
    expect(reloadOnce).toHaveBeenCalledTimes(1)
  })

  it('ignores a non-JSON answer, an error status, a bad envelope and a network failure', async () => {
    const { contract } = await load()
    fetchMock.mockResolvedValueOnce(
      new Response('<html></html>', { status: 200, headers: { 'content-type': 'text/html' } }),
    )
    await contract.checkClientContract({ force: true })
    fetchMock.mockResolvedValueOnce(jsonResponse(envelope({ min_contract: 99 }), { status: 503 }))
    await contract.checkClientContract({ force: true })
    fetchMock.mockResolvedValueOnce(jsonResponse({ error: { code: 'x', message: 'y', details: {} } }))
    await contract.checkClientContract({ force: true })
    fetchMock.mockRejectedValueOnce(new TypeError('offline'))
    await expect(contract.checkClientContract({ force: true })).resolves.toBeUndefined()
    expect(contract.clientContractMismatch()).toBe(false)
    expect(reloadOnce).not.toHaveBeenCalled()
  })

  it('collapses two checks closer than the gap into one unless forced', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(1_800_000_000_000)
    fetchMock.mockImplementation(async () => jsonResponse(envelope()))
    const { contract } = await load()
    await contract.checkClientContract()
    await contract.checkClientContract()
    expect(fetchMock).toHaveBeenCalledTimes(1)
    await contract.checkClientContract({ force: true })
    expect(fetchMock).toHaveBeenCalledTimes(2)
    vi.setSystemTime(1_800_000_000_000 + contract.CONTRACT_CHECK_GAP_MS + 1)
    await contract.checkClientContract()
    expect(fetchMock).toHaveBeenCalledTimes(3)
  })

  it('shares one request between concurrent checks', async () => {
    fetchMock.mockImplementation(async () => jsonResponse(envelope()))
    const { contract } = await load()
    await Promise.all([
      contract.checkClientContract({ force: true }),
      contract.checkClientContract({ force: true }),
    ])
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})
