import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'

import { ago, itemHref, itemSentence, needsYouSentence, orderItems } from '@/features/attention/lib/attention-grammar'
import { actorDisplayName, createRosterNames } from '@/features/attention/lib/attention-roster'
import { spaceAttentionAvailable, spaceAttentionTotals } from '@/features/spaces/lib/space-attention'
import { ApiError } from '@/shared/api/http'
import {
  listAttentionItems,
  type AttentionItem,
  type AttentionNeedsYou,
} from '@/shared/api/spaces-attention-items-client'
import { ArrowUpRight } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'

import './dashboard-needs-you.css'

/** Rows the Home card draws. The inbox holds the rest and is one click away. */
const GLANCE_ROWS = 4
/** Rows asked for from the inbox: a little more than the card draws, so the order has something to choose from. */
const GLANCE_READ = 6

type Phase = 'loading' | 'ready' | 'off' | 'failed'
type Glance = { readonly kind: 'needs_you'; readonly entry: AttentionNeedsYou } | { readonly kind: 'item'; readonly item: AttentionItem }

/**
 * "Trenger deg" on Home (WS11-S7): the few inbox rows that most want the viewer, with the whole inbox
 * one link away. It reads the same inbox the page reads and holds nothing in the browser. It draws
 * nothing while it loads, when the inbox is not turned on, and when nothing is open; a read that
 * fails is said in words, never shown as an empty inbox.
 */
export function DashboardNeedsYouCard() {
  const i18n = useI18n()
  const session = getSession()
  const [phase, setPhase] = createSignal<Phase>('loading')
  const [needsYou, setNeedsYou] = createSignal<readonly AttentionNeedsYou[]>([])
  const [items, setItems] = createSignal<readonly AttentionItem[]>([])
  const [gapped, setGapped] = createSignal(false)
  // The shell's shared poller says whether the surface is on ("off" is not asked for) and holds the counts.
  // When either moves, the inbox moved too, so the card reads it again; while both hold still, it does not
  // ask again on every poll.
  const shell = () => {
    if (!spaceAttentionAvailable()) return 'off'
    const totals = spaceAttentionTotals()
    return totals ? `on:${totals.badge}.${totals.unreadRooms}.${totals.otherOpen}` : 'on'
  }
  const key = createMemo(() => `${session.user?.id ?? ''}\u0000${session.activeOrg?.id ?? ''}\u0000${shell()}`)

  createEffect(key, (value) => {
    const [user = '', org = '', state = ''] = value.split('\u0000')
    if (!user || !org) {
      setPhase('loading')
      return
    }
    if (state === 'off') {
      setPhase('off')
      return
    }
    let live = true
    void listAttentionItems({ state: 'open', limit: GLANCE_READ }).then(
      (page) => {
        if (!live) return
        setNeedsYou(page.needsYou)
        setItems(orderItems(page.items))
        setGapped(page.gaps.length > 0)
        setPhase('ready')
      },
      (error: unknown) => {
        if (!live) return
        setNeedsYou([])
        setItems([])
        setGapped(false)
        setPhase(error instanceof ApiError && error.code === 'space_attention_disabled' ? 'off' : 'failed')
      },
    )
    return () => {
      live = false
    }
  })

  const glance = (): readonly Glance[] => [
    ...needsYou().map((entry): Glance => ({ kind: 'needs_you', entry })),
    ...items().map((item): Glance => ({ kind: 'item', item })),
  ].slice(0, GLANCE_ROWS)
  const shown = () => phase() === 'failed' || (phase() === 'ready' && (glance().length > 0 || gapped()))

  return (
    <Show when={shown()}>
      <section class="home-overview-card home-needs-you" aria-label={i18n.tr('Trenger deg', 'Needs you')}>
        <header class="home-overview-card__header">
          <div>
            <span class="home-eyebrow">{i18n.tr('Trenger deg', 'Needs you')}</span>
            <Show when={glance().length > 0}>
              <h2>{i18n.tr('Venter på deg', 'Waiting for you')}</h2>
            </Show>
          </div>
        </header>
        <Show when={phase() === 'failed'}>
          <p class="home-needs-you__state">
            {i18n.tr('Trenger deg kunne ikke hentes akkurat nå.', 'Needs you could not be loaded right now.')}
          </p>
        </Show>
        <Show when={phase() === 'ready' && gapped()}>
          <p class="home-needs-you__state">
            {i18n.tr(
              'Noe kunne ikke leses akkurat nå, så listen kan mangle rader.',
              'Something could not be read right now, so rows may be missing.',
            )}
          </p>
        </Show>
        <Show when={glance().length > 0}>
          <ul class="home-needs-you__list">
            <For each={glance()}>
              {(row) => (row.kind === 'needs_you' ? <NeedsYouGlance entry={row.entry} /> : <ItemGlance item={row.item} />)}
            </For>
          </ul>
        </Show>
        <footer>
          <a link href="/attention">
            {i18n.tr('Åpne innboksen', 'Open your inbox')}
            <ArrowUpRight size={14} />
          </a>
        </footer>
      </section>
    </Show>
  )
}

function NeedsYouGlance(props: { readonly entry: AttentionNeedsYou }) {
  const i18n = useI18n()
  return (
    <li class="home-needs-you__row" data-kind="needs_you">
      <a link href={itemHref(props.entry)}>
        <span class="home-needs-you__copy">
          <strong>{needsYouSentence(props.entry, i18n.tr)}</strong>
        </span>
        <time>{ago(props.entry.requestedAt, i18n.locale(), Date.now())}</time>
        <ArrowUpRight size={14} />
      </a>
    </li>
  )
}

function ItemGlance(props: { readonly item: AttentionItem }) {
  const i18n = useI18n()
  // The room's roster names the person or agent the row is about; until it answers, the fallback words stand.
  const names = createRosterNames(() => props.item.spaceRef)
  return (
    <li class="home-needs-you__row" data-kind={props.item.kind}>
      <a link href={itemHref(props.item)}>
        <span class="home-needs-you__copy">
          <strong>{itemSentence(props.item, i18n.tr, actorDisplayName(props.item.actor, names()))}</strong>
        </span>
        <time>{ago(props.item.updatedAt, i18n.locale(), Date.now())}</time>
        <ArrowUpRight size={14} />
      </a>
    </li>
  )
}
