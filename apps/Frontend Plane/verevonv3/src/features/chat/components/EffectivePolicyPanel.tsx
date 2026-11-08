import { createMemo, createSignal, For, Show } from 'solid-js'
import { ApiError } from '@/shared/api/api-error'
import { requestJson } from '@/shared/api/http'
import { Lock } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { en } from '@/shared/i18n/en'
import { no } from '@/shared/i18n/no'
import { createResource } from '@/shared/lib/create-resource-compat'
import './effective-policy-panel.css'

/**
 * What the policy resolves to for one thread or Space, and the layer that set each value (EP-9 T3, over
 * T2's read). A row says the value, who set it and whether a higher layer fixes it. The server decides what
 * a member may see; the panel shows what it sends and never names an origin it was not sent.
 */

/**
 * Off, the panel sends no request, so a build without the feature adds no traffic. On, the server's own
 * switch still decides: the read answers 404 while it is off, and the panel then renders nothing. Read
 * strictly: only the literal "true" turns it on.
 */
export const policyEffectiveViewEnabled = () => import.meta.env.VITE_POLICY_EFFECTIVE_VIEW_ENABLED === 'true'

const POLICY_PATH = '/api/v1/policy/effective'

/** The layers the read may name. Any other origin is shown as "source not reported". */
const POLICY_ORIGINS = ['platform', 'org', 'space', 'user', 'request'] as const
export type PolicyOrigin = (typeof POLICY_ORIGINS)[number]

/** A display-ready value: a boolean, a number or a string, or null when nothing is set. */
export type PolicyValue = boolean | number | string | null

/** One effective setting, as the server sends it after its member redaction. */
export type EffectivePolicyItem = {
  key: string
  value: PolicyValue
  origin: PolicyOrigin | null
  locked: boolean
  revision?: number
  /** The server withheld the value from this caller; the origin may still be shown. */
  redacted: boolean
  /** For a withheld list: how many entries it holds. */
  count?: number
  /** The owner did not answer for this setting, so neither its value nor its origin is shown. */
  unavailable: boolean
}

export type EffectivePolicyScope = { kind: 'thread'; threadId: string } | { kind: 'space'; spaceId: string }

/** A 404 means "not served here" and hides the panel; any other failure is an error the panel says so about. */
type PolicyLoad = { kind: 'off' } | { kind: 'ready'; items: EffectivePolicyItem[] }

/** The panel's copy in one locale. en.ts and no.ts keep the same keys, so either one fits this shape. */
type PolicyCopy = {
  title: string
  intro: string
  empty: string
  loadFailed: string
  sourceUnknown: string
  locked: string
  unavailable: string
  notSet: string
  redacted: string
  on: string
  off: string
  origin: Record<PolicyOrigin, string>
  keys: Record<string, string>
  setBy: (origin: string) => string
  redactedCount: (count: number) => string
  revision: (revision: number) => string
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isPolicyOrigin(value: unknown): value is PolicyOrigin {
  return typeof value === 'string' && (POLICY_ORIGINS as readonly string[]).includes(value)
}

function isPolicyScalar(value: unknown): value is Exclude<PolicyValue, null> {
  return (
    typeof value === 'boolean' || typeof value === 'string' || (typeof value === 'number' && Number.isFinite(value))
  )
}

/**
 * One setting as sent, or null when it has no key. A value this panel cannot show makes the row unavailable
 * rather than being rendered as something else.
 */
function parseItem(raw: unknown): EffectivePolicyItem | null {
  if (!isRecord(raw) || typeof raw.key !== 'string' || raw.key.trim() === '') return null
  const sent = raw.value === undefined ? null : raw.value
  const unavailable = raw.unavailable === true || (sent !== null && !isPolicyScalar(sent))
  const item: EffectivePolicyItem = {
    key: raw.key,
    value: !unavailable && isPolicyScalar(sent) ? sent : null,
    origin: !unavailable && isPolicyOrigin(raw.origin) ? raw.origin : null,
    locked: raw.locked === true,
    redacted: raw.redacted === true,
    unavailable,
  }
  if (typeof raw.revision === 'number' && Number.isInteger(raw.revision)) item.revision = raw.revision
  if (typeof raw.count === 'number' && Number.isInteger(raw.count) && raw.count >= 0) item.count = raw.count
  return item
}

/**
 * The settings in a T2 response. A response of any other shape is an error, never an empty policy: an empty
 * list would read as "nothing applies here".
 */
export function parseEffectivePolicy(payload: unknown): EffectivePolicyItem[] {
  const items = isRecord(payload) ? payload.items : undefined
  if (!Array.isArray(items)) throw new Error('The effective policy response has no items list.')
  return items.flatMap((raw: unknown) => {
    const item = parseItem(raw)
    return item ? [item] : []
  })
}

/** The query that names the thread or the Space to read. Null when there is no id, so nothing is read. */
function scopeQuery(scope: EffectivePolicyScope): string | null {
  if (scope.kind === 'thread') {
    return scope.threadId ? new URLSearchParams({ thread_id: scope.threadId }).toString() : null
  }
  return scope.spaceId ? new URLSearchParams({ space_id: scope.spaceId }).toString() : null
}

/** Reads the policy for one scope. A 404 is "not served here" (switch off, or no such scope), so nothing shows. */
async function loadEffectivePolicy(query: string): Promise<PolicyLoad> {
  try {
    return { kind: 'ready', items: parseEffectivePolicy(await requestJson<unknown>(`${POLICY_PATH}?${query}`)) }
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return { kind: 'off' }
    throw error
  }
}

function keyLabel(text: PolicyCopy, key: string): string {
  return Object.hasOwn(text.keys, key) ? (text.keys[key] ?? key) : key
}

function valueText(item: EffectivePolicyItem, text: PolicyCopy): string {
  if (item.unavailable) return text.unavailable
  if (item.redacted) return item.count === undefined ? text.redacted : text.redactedCount(item.count)
  if (item.value === null) return text.notSet
  if (typeof item.value === 'boolean') return item.value ? text.on : text.off
  return String(item.value)
}

/** The layer that set the value. Only the layers the server named are claimed; a row that could not be read claims none. */
function originText(item: EffectivePolicyItem, text: PolicyCopy): string {
  if (item.unavailable) return ''
  return item.origin === null ? text.sourceUnknown : text.setBy(text.origin[item.origin])
}

function revisionText(item: EffectivePolicyItem, text: PolicyCopy): string {
  if (item.unavailable || item.revision === undefined) return ''
  return text.revision(item.revision)
}

// Keeps aria-controls unique when the panel is on a page twice (the thread header and the room details).
let panelSequence = 0

export function EffectivePolicyPanel(props: { scope: EffectivePolicyScope }) {
  const i18n = useI18n()
  const copy = createMemo<PolicyCopy>(() => (i18n.locale() === 'no' ? no.effectivePolicy : en.effectivePolicy))
  const [open, setOpen] = createSignal(false)
  const regionId = `effective-policy-${(panelSequence += 1)}`

  // The source is null while the switch is off, so no request is made and the resource stays empty.
  const [view] = createResource(
    () => (policyEffectiveViewEnabled() ? scopeQuery(props.scope) : null),
    (query: string) => loadEffectivePolicy(query),
  )

  // A read still in flight shows nothing, so a panel that has moved to another thread never shows the old values.
  const settled = createMemo(() => (view.loading ? undefined : view()))
  const items = createMemo(() => {
    const loaded = settled()
    return loaded?.kind === 'ready' ? loaded.items : []
  })
  // Shown once the read settles: a 404 leaves nothing on the page, and a failure is said in place.
  const visible = createMemo(
    () => !view.loading && (view.error !== undefined || settled()?.kind === 'ready'),
  )

  return (
    <Show when={visible()}>
      <section class="verevon-effective-policy">
        <button
          type="button"
          class="verevon-effective-policy__toggle"
          aria-expanded={open() ? 'true' : 'false'}
          aria-controls={open() ? regionId : undefined}
          onClick={() => setOpen(!open())}
        >
          {copy().title}
        </button>
        <Show when={open()}>
          <div id={regionId} class="verevon-effective-policy__body">
            <Show
              when={view.error === undefined}
              fallback={<p class="verevon-effective-policy__alert" role="alert">{copy().loadFailed}</p>}
            >
              <p class="verevon-effective-policy__intro">{copy().intro}</p>
              <Show when={items().length > 0} fallback={<p class="verevon-effective-policy__intro">{copy().empty}</p>}>
                <dl class="verevon-effective-policy__rows">
                  <For each={items()}>
                    {(item) => (
                      <div class="verevon-effective-policy__row">
                        <dt>{keyLabel(copy(), item.key)}</dt>
                        <dd>
                          <span class="verevon-effective-policy__value">{valueText(item, copy())}</span>
                          <Show when={originText(item, copy())}>
                            {(origin) => <span class="verevon-effective-policy__origin">{origin()}</span>}
                          </Show>
                          <Show when={revisionText(item, copy())}>
                            {(revision) => <span class="verevon-effective-policy__meta">{revision()}</span>}
                          </Show>
                          <Show when={item.locked}>
                            <span class="verevon-effective-policy__lock">
                              <Lock size={12} aria-hidden="true" /> {copy().locked}
                            </span>
                          </Show>
                        </dd>
                      </div>
                    )}
                  </For>
                </dl>
              </Show>
            </Show>
          </div>
        </Show>
      </section>
    </Show>
  )
}
