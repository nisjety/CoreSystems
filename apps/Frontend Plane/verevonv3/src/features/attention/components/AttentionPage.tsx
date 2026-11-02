import { For, Show, createEffect, createSignal, onCleanup } from 'solid-js'

import { AttentionItemRow, AttentionNeedsYouRow } from '@/features/attention/components/AttentionItemRow'
import { ATTENTION_LANES, groupByLane, laneLabel } from '@/features/attention/lib/attention-grammar'
import { refreshSpaceAttention } from '@/features/spaces/lib/space-attention'
import { ApiError } from '@/shared/api/http'
import {
  listAttentionItems,
  setAttentionItemState,
  type AttentionItem,
  type AttentionItemState,
  type AttentionNeedsYou,
} from '@/shared/api/spaces-attention-items-client'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'

import '@/styles/attention.css'

type Phase = 'loading' | 'ready' | 'disabled' | 'failed'

const TABS: readonly AttentionItemState[] = ['open', 'snoozed', 'done']

function tabLabel(tab: AttentionItemState, tr: (no: string, en: string) => string): string {
  if (tab === 'open') return tr('Åpne', 'Open')
  if (tab === 'snoozed') return tr('Utsatt', 'Snoozed')
  return tr('Ferdig', 'Done')
}

/**
 * Home inbox at `/attention` (WS11-S7): what wants the viewer's attention across every room,
 * in lanes. The rows are the viewer's own and carry ids and a closed kind; titles are read at
 * render under the viewer's current access and appear only when released. Nothing is stored in
 * the browser. A source the gateway could not read is named, never shown as empty.
 */
export default function AttentionPage() {
  const i18n = useI18n()
  const [tab, setTab] = createSignal<AttentionItemState>('open')
  const [phase, setPhase] = createSignal<Phase>('loading')
  const [items, setItems] = createSignal<readonly AttentionItem[]>([])
  const [needsYou, setNeedsYou] = createSignal<readonly AttentionNeedsYou[]>([])
  const [gaps, setGaps] = createSignal<readonly string[]>([])
  const [refsOnly, setRefsOnly] = createSignal(false)
  const [cursor, setCursor] = createSignal<string | null>(null)
  const [busyId, setBusyId] = createSignal<string | null>(null)
  const [message, setMessage] = createSignal('')
  const [loadingMore, setLoadingMore] = createSignal(false)

  let disposed = false
  // A newer request supersedes an older one: a slow answer for a tab the viewer left is dropped.
  let generation = 0
  onCleanup(() => {
    disposed = true
  })

  async function load(append: boolean): Promise<void> {
    const mine = ++generation
    const state = tab()
    if (append) setLoadingMore(true)
    else setPhase('loading')
    setMessage('')
    try {
      const page = await listAttentionItems({ state, cursor: append ? cursor() : null })
      if (disposed || mine !== generation) return
      setItems((current) => (append ? [...current, ...page.items] : page.items))
      if (!append) setNeedsYou(page.needsYou)
      setGaps(page.gaps)
      setRefsOnly(page.mode === 'zdr_refs_only')
      setCursor(page.nextCursor)
      setPhase('ready')
    } catch (error) {
      if (disposed || mine !== generation) return
      if (error instanceof ApiError && error.code === 'space_attention_disabled') {
        setPhase('disabled')
        return
      }
      setMessage(translateApiError(error, i18n.tr))
      setPhase(append ? 'ready' : 'failed')
    } finally {
      if (!disposed && mine === generation) setLoadingMore(false)
    }
  }

  createEffect(
    () => 0,
    () => {
      void load(false)
    },
  )

  function choose(next: AttentionItemState) {
    if (next === tab()) return
    setTab(next)
    setItems([])
    setNeedsYou([])
    setCursor(null)
    void load(false)
  }

  async function change(
    item: AttentionItem,
    to: { readonly state: 'done' | 'open' } | { readonly state: 'snoozed'; readonly snoozeUntil: number },
  ) {
    if (busyId() !== null) return
    setBusyId(item.itemId)
    setMessage('')
    try {
      await setAttentionItemState(item.itemId, to)
      if (disposed) return
      // The row leaves this tab: every change moves it to another one.
      setItems((current) => current.filter((row) => row.itemId !== item.itemId))
      refreshSpaceAttention()
    } catch (error) {
      if (!disposed) setMessage(translateApiError(error, i18n.tr))
    } finally {
      if (!disposed) setBusyId(null)
    }
  }

  const lanes = () => groupByLane(items())
  const hasAnything = () => items().length > 0 || needsYou().length > 0

  return (
    <section class="verevon-attention" aria-labelledby="attention-title">
      <header class="verevon-attention__header">
        <h1 id="attention-title">{i18n.tr('Trenger deg', 'Needs you')}</h1>
        <div class="verevon-attention__tabs" role="group" aria-label={i18n.tr('Vis', 'Show')}>
          <For each={TABS}>
            {(candidate) => (
              <button
                type="button"
                class="verevon-attention__tab"
                aria-pressed={tab() === candidate ? 'true' : 'false'}
                onClick={() => choose(candidate)}
              >
                {tabLabel(candidate, i18n.tr)}
              </button>
            )}
          </For>
        </div>
      </header>

      <Show when={message()}>
        <p class="verevon-attention__notice" role="alert">{message()}</p>
      </Show>

      <Show when={phase() === 'disabled'}>
        <p class="verevon-attention__empty" role="status">
          {i18n.tr('Innboksen er ikke slått på ennå.', 'The attention inbox is not turned on yet.')}
        </p>
      </Show>

      <Show when={phase() === 'loading'}>
        <p class="verevon-attention__empty" role="status" aria-live="polite">
          {i18n.tr('Henter innboksen …', 'Loading your inbox…')}
        </p>
      </Show>

      <Show when={phase() === 'failed'}>
        <p class="verevon-attention__empty" role="status">
          <button type="button" class="verevon-attention-action" onClick={() => void load(false)}>
            {i18n.tr('Prøv igjen', 'Try again')}
          </button>
        </p>
      </Show>

      <Show when={phase() === 'ready'}>
        <Show when={gaps().length > 0}>
          <p class="verevon-attention__notice" role="status">
            {i18n.tr(
              'Noe kunne ikke leses akkurat nå, så tittel eller ventende godkjenninger kan mangle.',
              'Something could not be read right now, so titles or waiting approvals may be missing.',
            )}
          </p>
        </Show>
        <Show when={refsOnly()}>
          <p class="verevon-attention__notice" role="status">
            {i18n.tr(
              'Organisasjonen din lagrer ikke innhold, så titler vises ikke her.',
              'Your organization keeps no content, so titles are not shown here.',
            )}
          </p>
        </Show>

        <Show
          when={hasAnything()}
          fallback={
            <p class="verevon-attention__empty" role="status">
              {tab() === 'open'
                ? i18n.tr('Ingenting trenger deg akkurat nå.', 'Nothing needs you right now.')
                : i18n.tr('Ingenting her.', 'Nothing here.')}
            </p>
          }
        >
          <For each={ATTENTION_LANES}>
            {(lane) => {
              const rows = () => (lane === 'needs_you' ? [] : lanes()[lane])
              const count = () => (lane === 'needs_you' ? needsYou().length : rows().length)
              return (
                <Show when={count() > 0}>
                  <section class="verevon-attention__lane" data-lane={lane} aria-labelledby={`attention-lane-${lane}`}>
                    <h2 id={`attention-lane-${lane}`}>
                      {laneLabel(lane, i18n.tr)} <span class="verevon-attention__count">{count()}</span>
                    </h2>
                    <ul class="verevon-attention__list">
                      <Show
                        when={lane === 'needs_you'}
                        fallback={
                          <For each={rows()}>
                            {(item) => (
                              <AttentionItemRow item={item} busy={busyId() !== null} onState={change} />
                            )}
                          </For>
                        }
                      >
                        <For each={needsYou()}>{(entry) => <AttentionNeedsYouRow entry={entry} />}</For>
                      </Show>
                    </ul>
                  </section>
                </Show>
              )
            }}
          </For>
          <Show when={cursor()}>
            <button
              type="button"
              class="verevon-attention-action verevon-attention__more"
              disabled={loadingMore()}
              onClick={() => void load(true)}
            >
              {i18n.tr('Vis flere', 'Show more')}
            </button>
          </Show>
        </Show>
      </Show>
    </section>
  )
}
