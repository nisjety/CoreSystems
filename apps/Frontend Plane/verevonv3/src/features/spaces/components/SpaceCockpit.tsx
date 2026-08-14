import { createSignal, For, onCleanup, onMount, Show, type JSX } from 'solid-js'

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
 * Only Chat and Activity have a real space-scoped source today (threads), and
 * Agent has one as of the new actions catalog. Work, Knowledge and Members have
 * no space-scoped endpoint at all. Rather than render a convincing empty state
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
  readonly label: string
  /** What this tab answers, per the plan's six questions. */
  readonly purpose: string
  /**
   * Which plane must publish a Space projection before this tab can show
   * anything. Named in the unavailable state so the gap is attributable rather
   * than mysterious.
   */
  readonly owner: string
}

const SPACE_TABS: readonly SpaceTabDefinition[] = [
  { id: 'chat', label: 'Samtaler', purpose: 'Alle tråder i rommet.', owner: 'Model Plane' },
  {
    id: 'arbeid',
    label: 'Arbeid',
    purpose: 'Arbeid i kø eller under kjøring, planer, overvåkinger og gjenforsøk.',
    owner: 'Model Plane',
  },
  {
    id: 'kunnskap',
    label: 'Kunnskap',
    purpose: 'Dokumenter, kilder, filer og minne som gjelder her.',
    owner: 'Data Plane',
  },
  {
    id: 'aktivitet',
    label: 'Aktivitet',
    purpose: 'Hva som har skjedd, hva det kostet, og beviset for det.',
    owner: 'Model Plane',
  },
  {
    id: 'agent',
    label: 'Agent',
    purpose: 'Aktiv modell, ferdigheter og hvilke handlinger som er tillatt her.',
    owner: 'Model Plane',
  },
  {
    id: 'medlemmer',
    label: 'Medlemmer',
    purpose: 'Hvem som ser og styrer rommet, og med hvilken rolle.',
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
  const [active, setActive] = createSignal<SpaceTabId>(props.initialTab ?? DEFAULT_TAB)
  const tabButtons: Partial<Record<SpaceTabId, HTMLButtonElement>> = {}

  onMount(() => {
    if (typeof window === 'undefined') return
    // Adopt an incoming deep link, but never override an explicit initialTab
    // with the default when the hash carries nothing meaningful.
    if (window.location.hash) setActive(tabFromHash(window.location.hash))
    const onHashChange = () => setActive(tabFromHash(window.location.hash))
    window.addEventListener('hashchange', onHashChange)
    onCleanup(() => window.removeEventListener('hashchange', onHashChange))
  })

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
      <div class="verevon-space-tabs" role="tablist" aria-label="Romvisninger" onKeyDown={onKeyDown}>
        <For each={SPACE_TABS}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              id={`space-tab-${tab.id}`}
              aria-selected={active() === tab.id}
              aria-controls={`space-panel-${tab.id}`}
              tabIndex={active() === tab.id ? 0 : -1}
              class={`verevon-space-tab${active() === tab.id ? ' verevon-space-tab--active' : ''}`}
              onClick={() => select(tab.id)}
              ref={(element) => { tabButtons[tab.id] = element }}
            >
              {tab.label}
            </button>
          )}
        </For>
      </div>

      <For each={SPACE_TABS}>
        {(tab) => {
          const content = () => props.tabs?.[tab.id]
          return (
            <section
              role="tabpanel"
              id={`space-panel-${tab.id}`}
              aria-labelledby={`space-tab-${tab.id}`}
              class="verevon-space-panel"
              tabIndex={active() === tab.id ? 0 : -1}
              hidden={active() !== tab.id}
            >
              <Show
                when={content()}
                fallback={
                  <div class="verevon-space-panel-unavailable">
                    <p class="verevon-space-panel-purpose">{tab.purpose}</p>
                    <p class="verevon-space-panel-reason">
                      {tab.owner} har ikke publisert en romprojeksjon for denne fanen ennå, så det
                      finnes ingenting å vise her. Dette er en manglende kobling, ikke et tomt rom.
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
