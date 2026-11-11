import { createSignal } from 'solid-js'

import { reloadOnceForStaleBuild } from '@/app/chunk-recovery'
import { isDevBuild } from '@/shared/build/build-info'
import { hasUnsavedWork, hasUnsavedWorkProbes } from '@/shared/build/unsaved-work'

/**
 * Client contract (WS07-S3). The gateway publishes the contract it speaks and the
 * oldest client contract it still serves at `GET /api/v1/client-config`. When this
 * bundle is older than the minimum, a banner says so.
 *
 * Parity: these two numbers mirror `CLIENT_CONTRACT` / `CLIENT_MIN_CONTRACT` in
 * `apps/gateway/src/client_contract.rs` and `clientContract` in `vite.config.ts`
 * (written into `version.json`). `apps/gateway/scripts/client-contract-parity.test.mjs`
 * fails when they disagree. Change all three together.
 */
export const CLIENT_CONTRACT = 1
export const CLIENT_MIN_CONTRACT = 1

export const CLIENT_CONFIG_URL = '/api/v1/client-config'
/** Two checks closer than this collapse into one. */
export const CONTRACT_CHECK_GAP_MS = 30_000

export type ClientVersionPolicy = 'off' | 'banner' | 'enforce'

/**
 * Where the gateway runs, as `CLIENT_ENV_IDENTITY` reports it (F-t3-098): a closed set, never the
 * raw `APP_ENV`. The gateway leaves the field out while its switch is off.
 */
export type ClientEnvironment = 'production' | 'staging' | 'development' | 'other'

const CLIENT_ENVIRONMENTS: readonly ClientEnvironment[] = ['production', 'staging', 'development', 'other']

export interface ClientConfig {
  contract: number
  minContract: number
  policy: ClientVersionPolicy
  revision: string
  buildDate: string
  environment?: ClientEnvironment
}

const [mismatch, setMismatch] = createSignal(false)

/** True when the gateway's minimum contract is above this bundle's and the policy allows a notice. */
export const clientContractMismatch = mismatch

const [revision, setRevision] = createSignal<string | null>(null)

/**
 * The revision the gateway last reported in its client config, or null before the first check, when it reports none,
 * and in a dev build. A problem report's "app build" section sends this value and nothing else about the build.
 */
export const clientRevision = revision

/** A revision a report may carry: a short token such as a commit id or a build label. Anything else is not reported. */
const REVISION_PATTERN = /^[A-Za-z0-9._+-]{1,64}$/

const [environment, setEnvironment] = createSignal<ClientEnvironment | null>(null)

/** The environment the gateway last reported, or null when it reports none (switch off, or not asked yet). */
export const clientEnvironment = environment

/** Parse the `{data}` envelope; null for anything else (an HTML fallback, an error envelope). */
export function clientConfigFrom(body: unknown): ClientConfig | null {
  if (typeof body !== 'object' || body === null) return null
  const data = (body as { data?: unknown }).data
  if (typeof data !== 'object' || data === null) return null
  const d = data as Record<string, unknown>
  const policy = d.policy
  if (
    typeof d.contract !== 'number' ||
    typeof d.min_contract !== 'number' ||
    (policy !== 'off' && policy !== 'banner' && policy !== 'enforce')
  ) {
    return null
  }
  const reported = CLIENT_ENVIRONMENTS.find((known) => known === d.environment)
  return {
    contract: d.contract,
    minContract: d.min_contract,
    policy,
    revision: typeof d.revision === 'string' ? d.revision : '',
    buildDate: typeof d.build_date === 'string' ? d.build_date : '',
    ...(reported === undefined ? {} : { environment: reported }),
  }
}

/** True when `config` says this bundle is too old to keep running unnoticed. */
export const isContractStale = (config: ClientConfig, ownContract: number = CLIENT_CONTRACT): boolean =>
  config.policy !== 'off' && ownContract < config.minContract

/**
 * What to do about a stale contract. `enforce` reloads only when at least one
 * unsaved-work probe is registered AND none reports work; with no probe the page
 * cannot tell a reload is safe, so `enforce` behaves as `banner`.
 */
export function contractAction(
  config: ClientConfig,
  probes: { registered: boolean; dirty: boolean },
  ownContract: number = CLIENT_CONTRACT,
): 'none' | 'banner' | 'reload' {
  if (!isContractStale(config, ownContract)) return 'none'
  if (config.policy === 'enforce' && probes.registered && !probes.dirty) return 'reload'
  return 'banner'
}

let lastCheckAt = 0
let inFlight: Promise<void> | null = null

/**
 * Fetch the client config and update the banner state. Never throws. Called from the
 * stale-tab check (`checkClientVersion`), so it runs on the same triggers.
 */
export function checkClientContract(options: { force?: boolean } = {}): Promise<void> {
  if (isDevBuild()) return Promise.resolve()
  if (inFlight) return inFlight
  const now = Date.now()
  if (options.force !== true && now - lastCheckAt < CONTRACT_CHECK_GAP_MS) return Promise.resolve()
  lastCheckAt = now
  inFlight = (async () => {
    try {
      const res = await fetch(CLIENT_CONFIG_URL, { cache: 'no-store', credentials: 'omit' })
      const type = res.headers.get('content-type') ?? ''
      if (!res.ok || !type.includes('json')) return
      const config = clientConfigFrom(await res.json())
      if (config === null) return
      setEnvironment(config.environment ?? null)
      setRevision(REVISION_PATTERN.test(config.revision) ? config.revision : null)
      const action = contractAction(config, {
        registered: hasUnsavedWorkProbes(),
        dirty: hasUnsavedWork(),
      })
      setMismatch(action !== 'none')
      // The reload that `enforce` allows is the same one the banner offers, taken for
      // the user because nothing would be lost. The guarded reload lives with the
      // chunk-recovery marker so the two paths can never loop.
      if (action === 'reload') {
        reloadOnceForStaleBuild()
      }
    } catch {
      // Offline or blocked: the next trigger tries again.
    } finally {
      inFlight = null
    }
  })()
  return inFlight
}
