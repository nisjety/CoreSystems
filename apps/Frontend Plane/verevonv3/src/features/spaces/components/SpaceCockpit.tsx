import { createContext, createEffect, createMemo, createSignal, For, Show, useContext, type Accessor, type Setter } from 'solid-js'
import type { JSX } from '@solidjs/web'

import { useI18n } from '@/shared/i18n'
import { RegionBoundary } from '@/shared/ui/resilience/RegionBoundary'
import {
  DEFAULT_SPACE_TAB,
  spaceTabFromHash,
  visibleSpaceTabs,
  type SpaceTabId,
} from '../lib/space-tab-registry'

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

export type { SpaceTabId }

export interface SpaceCockpitProps {
  /**
   * Content per tab. An omitted tab is not an error — it renders the
   * "not published yet" state naming its owner plane.
   */
  readonly tabs?: Partial<Record<SpaceTabId, JSX.Element>>
  /** Starting tab when the URL carries no recognized hash. */
  readonly initialTab?: SpaceTabId
}

/**
 * Which tab is showing, for a panel that must fetch only while it is the active
 * one (WS07-S12). Panels mount eagerly and are merely hidden, so a panel that
 * loads on mount loads whether or not anyone opens it; one that reads this
 * context loads on first activation instead. Outside a cockpit it reports the
 * default tab, so a lazy panel rendered alone stays idle rather than guessing.
 */
export const ActiveSpaceTabContext = createContext<Accessor<SpaceTabId>>(() => DEFAULT_SPACE_TAB)

export function useActiveSpaceTab(): Accessor<SpaceTabId> {
  return useContext(ActiveSpaceTabContext)
}

export function SpaceCockpit(props: SpaceCockpitProps) {
  const [active, setActive] = createSignal<SpaceTabId>(props.initialTab ?? DEFAULT_SPACE_TAB)
  // The provider wraps the view so the memo that evaluates the callers' tabs
  // (and so constructs every panel) is created under it and the panels can read
  // the context.
  return (
    <ActiveSpaceTabContext value={active}>
      <SpaceCockpitView tabs={props.tabs} active={active} setActive={setActive} />
    </ActiveSpaceTabContext>
  )
}

function SpaceCockpitView(props: {
  readonly tabs?: SpaceCockpitProps['tabs']
  readonly active: Accessor<SpaceTabId>
  readonly setActive: Setter<SpaceTabId>
}) {
  const i18n = useI18n()
  const active = props.active
  const setActive = props.setActive
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
      if (window.location.hash) setActive(spaceTabFromHash(window.location.hash))
      const onHashChange = () => setActive(spaceTabFromHash(window.location.hash))
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
    const order = visibleSpaceTabs().map((tab) => tab.id)
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
        <For each={visibleSpaceTabs()}>
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

      <For each={visibleSpaceTabs()}>
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
                <RegionBoundary region="space-tab" resetKey={active} compact>
                  {content()}
                </RegionBoundary>
              </Show>
            </section>
          )
        }}
      </For>
    </div>
  )
}
