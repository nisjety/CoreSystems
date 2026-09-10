import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { useI18n } from '@/shared/i18n'

/**
 * The Space cockpit shell: the six tabs the adoption plan specifies
 * (`docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md`, "Space home").
 *
 * # Presentational only, on purpose
 *
 * It fetches nothing. Every tab's content is passed in, so the page that owns
 * the Space decides what to load and when. Two reasons this matters here rather
 * than being generic good taste:
 *
 * 1. The space-scoped gateway surface is moving right now — `/membership` was
 *    replaced by `/actions` mid-flight. A shell that fetched would break every
 *    time that happens, and would duplicate the Space client another workstream
 *    owns.
 * 2. The BFF stays a proxy. Nothing here asks it to compose a cockpit; the
 *    cockpit is assembled in the browser from what the owning planes already
 *    return.
 *
 * # Honest about what does not exist yet
 *
 * Chat, Activity, Agent and Members all have real Space-scoped sources now.
 * Work and Knowledge still have none: Model Plane's run and schedule listings
 * carry no `space_id` filter, and Data Plane honours a Space decision on only
 * its three retrieval endpoints. Rather than render a convincing empty state
 * that implies "nothing here", an unsupplied tab says which owner plane has not
 * published a Space projection yet — the same honesty `SpacePage` already
 * applies to run receipts. A tab that looks finished but shows nothing is how a
 * feature quietly becomes "built but never wired".
 *
 * # Tabs are hash-driven
 *
 * `#arbeid`, `#aktivitet`, ... so deep links work without a router change, and
 * the anchor links already in `SpacePage` keep working. When the plan's
 * `/spaces/:spaceId/work` routes land, the owner swaps `activeTab` for a route
 * param and nothing else here changes.
 */

export type SpaceTabId = 'chat' | 'arbeid' | 'kunnskap' | 'aktivitet' | 'agent' | 'medlemmer'

interface SpaceTabDefinition {
  readonly id: SpaceTabId
  readonly labelNo: string
  readonly labelEn: string
  /** What this tab answers, per the plan's six questions. */
  readonly purposeNo: string
  readonly purposeEn: string
  /**
   * Which plane must publish a Space projection before this tab can show
   * anything. Named in the unavailable state so the gap is attributable rather
   * than mysterious. Plane names are not translated — they are the system's
   * own proper nouns, same as everywhere else this codebase names a plane.
   */
  readonly owner: string
}

const SPACE_TABS: readonly SpaceTabDefinition[] = [
  {
    id: 'chat',
    labelNo: 'Samtaler',
    labelEn: 'Chat',
    purposeNo: 'Alle tråder i rommet.',
    purposeEn: 'All threads in the Space.',
    owner: 'Model Plane',
  },
  {
    id: 'arbeid',
    labelNo: 'Arbeid',
    labelEn: 'Work',
    purposeNo: 'Arbeid i kø eller under kjøring, planer, overvåkinger og gjenforsøk.',
    purposeEn: 'Work queued or running, plans, monitors, and retries.',
    owner: 'Model Plane',
  },
  {
    id: 'kunnskap',
    labelNo: 'Kunnskap',
    labelEn: 'Knowledge',
    purposeNo: 'Dokumenter, kilder, filer og minne som gjelder her.',
    purposeEn: 'Documents, sources, files, and memory relevant here.',
    owner: 'Data Plane',
  },
  {
    id: 'aktivitet',
    labelNo: 'Aktivitet',
    labelEn: 'Activity',
    purposeNo: 'Hva som har skjedd, hva det kostet, og beviset for det.',
    purposeEn: 'What happened, what it cost, and the proof of it.',
    owner: 'Model Plane',
  },
  {
    id: 'agent',
    labelNo: 'Agent',
    labelEn: 'Agent',
    purposeNo: 'Aktiv modell, ferdigheter og hvilke handlinger som er tillatt her.',
    purposeEn: 'The active model, its skills, and which actions are permitted here.',
    owner: 'Model Plane',
  },
  {
    id: 'medlemmer',
    labelNo: 'Medlemmer',
    labelEn: 'Members',
    purposeNo: 'Hvem som ser og styrer rommet, og med hvilken rolle.',
    purposeEn: 'Who can see and manage the Space, and with what role.',
    owner: 'Control Plane',
  },
]

const TAB_IDS = new Set<string>(SPACE_TABS.map((tab) => tab.id))
const DEFAULT_TAB: SpaceTabId = 'chat'
const LEGACY_HASH_ALIASES: Readonly<Record<string, SpaceTabId>> = {
  members: 'medlemmer',
  work: 'arbeid',
  activity: 'aktivitet',
}

function tabFromHash(hash: string): SpaceTabId {
  const value = hash.replace(/^#/, '').trim().toLowerCase()
  if (TAB_IDS.has(value)) return value as SpaceTabId
  return LEGACY_HASH_ALIASES[value] ?? DEFAULT_TAB
}

export interface SpaceCockpitProps {
  /**
   * Content per tab. An omitted tab is not an error — it renders the
   * "not published yet" state naming its owner plane.
   */
  readonly tabs?: Partial<Record<SpaceTabId, JSX.Element>>
  /** Starting tab when the URL carries no recognized hash. */
  readonly initialTab?: SpaceTabId
}

export function SpaceCockpit(props: SpaceCockpitProps) {
  const i18n = useI18n()
  const [active, setActive] = createSignal<SpaceTabId>(props.initialTab ?? DEFAULT_TAB)
  // One evaluation of the caller's tabs object per change — see the note on
  // `content` below for why reading `props.tabs` per tab is not free.
  const tabs = createMemo(() => props.tabs ?? {})
  const tabButtons: Partial<Record<SpaceTabId, HTMLButtonElement>> = {}

  createEffect(
    () => undefined,
    () => {
      if (typeof window === 'undefined') return undefined
      // Adopt an incoming deep link, but never override an explicit initialTab
      // with the default when the hash carries nothing meaningful.
      if (window.location.hash) setActive(tabFromHash(window.location.hash))
      const onHashChange = () => setActive(tabFromHash(window.location.hash))
      window.addEventListener('hashchange', onHashChange)
      return () => window.removeEventListener('hashchange', onHashChange)
    },
  )

  const select = (id: SpaceTabId, options?: { focus?: boolean }) => {
    setActive(id)
    if (typeof window !== 'undefined') {
      // Replace rather than push: flipping tabs should not fill the back stack.
      window.history.replaceState(null, '', `#${id}`)
    }
    if (options?.focus) tabButtons[id]?.focus()
  }

  /**
   * Roving focus, so the tab strip behaves like a tablist rather than six
   * separate buttons.
   */
  const onKeyDown = (event: KeyboardEvent) => {
    const order = SPACE_TABS.map((tab) => tab.id)
    const index = order.indexOf(active())
    if (index < 0) return
    const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0
    if (step === 0) return
    event.preventDefault()
    const next = order[(index + step + order.length) % order.length]
    if (next) select(next, { focus: true })
  }

  return (
    <div class="verevon-space-cockpit">
      <div
        class="verevon-space-tabs"
        role="tablist"
        aria-label={i18n.tr('Romvisninger', 'Space views')}
        onKeyDown={onKeyDown}
      >
        <For each={SPACE_TABS}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              id={`space-tab-${tab.id}`}
              aria-selected={active() === tab.id ? 'true' : 'false'}
              aria-controls={`space-panel-${tab.id}`}
              tabindex={active() === tab.id ? 0 : -1}
              class={`verevon-space-tab${active() === tab.id ? ' verevon-space-tab--active' : ''}`}
              onClick={() => select(tab.id)}
              ref={(element) => { tabButtons[tab.id] = element }}
            >
              {i18n.tr(tab.labelNo, tab.labelEn)}
            </button>
          )}
        </For>
      </div>

      <For each={SPACE_TABS}>
        {(tab) => {
          // Memoized, and reading the memoized `tabs` object rather than
          // `props.tabs` directly.
          //
          // `props.tabs` is a getter over the caller's object literal, so every
          // read re-runs that literal — and Solid JSX constructs a component
          // eagerly, so re-running it MOUNTS every panel again. This `For` has
          // six tabs and each read `props.tabs` twice (once for `when`, once
          // for the child), so a single context resolve produced twelve
          // constructions of all six panels. Panels that fetch on mount
          // (`SpaceWorkPanel`, `SpaceKnowledgePanel`,
          // `SpaceInstructionsSection`) each fired a burst of identical
          // requests — measured live as six `/work` and six `/knowledge` calls
          // inside 3ms, repeated on every membership recheck.
          const content = createMemo(() => tabs()[tab.id])
          return (
            <section
              role="tabpanel"
              id={`space-panel-${tab.id}`}
              aria-labelledby={`space-tab-${tab.id}`}
              class="verevon-space-panel"
              tabindex={active() === tab.id ? 0 : -1}
              hidden={active() !== tab.id}
            >
              <Show
                when={content()}
                fallback={
                  <div class="verevon-space-panel-unavailable">
                    <p class="verevon-space-panel-purpose">{i18n.tr(tab.purposeNo, tab.purposeEn)}</p>
                    <p class="verevon-space-panel-reason">
                      {i18n.tr(
                        `${tab.owner} har ikke publisert en romprojeksjon for denne fanen ennå, så det finnes ingenting å vise her. Dette er en manglende kobling, ikke et tomt rom.`,
                        `${tab.owner} has not published a Space projection for this tab yet, so there is nothing to show here. This is a missing connection, not an empty Space.`,
                      )}
                    </p>
                  </div>
                }
              >
                {content()}
              </Show>
            </section>
          )
        }}
      </For>
    </div>
  )
}
