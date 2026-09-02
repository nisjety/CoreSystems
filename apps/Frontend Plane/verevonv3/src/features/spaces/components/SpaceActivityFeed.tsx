import { createMemo, For, Show } from 'solid-js'
import { Dynamic } from '@solidjs/web'

import {
  buildSpaceActivity,
  type SpaceActivityItem,
  type ThreadLikeActivitySource,
} from '@/features/spaces/lib/activity-grammar'

/**
 * A Space's activity feed, rendered with the Verb/Object/Outcome grammar.
 *
 * Standalone and data-agnostic on purpose. It takes already-fetched threads
 * rather than fetching them, so whoever owns the Space cockpit decides when and
 * how the data arrives, and this stays a pure rendering surface with no opinion
 * about transport.
 *
 * It asks the BFF for nothing of its own. The classification lives in
 * `../lib/activity-grammar`, in the browser, over what the owning plane already
 * returned through the gateway's existing proxy route — no new endpoint and no
 * activity semantics pushed into the BFF.
 *
 * Styling is semantic classes only; this project has no Tailwind.
 */

interface SpaceActivityFeedProps {
  readonly threads: readonly ThreadLikeActivitySource[]
  /** Rendered instead of the list when there is genuinely nothing yet. */
  readonly emptyLabel?: string
}

function toneClass(item: SpaceActivityItem): string {
  return `verevon-activity-outcome verevon-activity-outcome--${item.outcome.tone}`
}

function rowClass(item: SpaceActivityItem): string {
  return [
    'verevon-activity-row',
    `verevon-activity-row--${item.salience}`,
    item.outcome.live ? 'verevon-activity-row--live' : '',
  ]
    .filter(Boolean)
    .join(' ')
}

function formatWhen(value?: string): string {
  if (!value) return ''
  try {
    return new Date(value).toLocaleString('nb-NO', { dateStyle: 'short', timeStyle: 'short' })
  } catch {
    return value
  }
}

export function SpaceActivityFeed(props: SpaceActivityFeedProps) {
  const items = createMemo(() => buildSpaceActivity(props.threads))

  return (
    <Show
      when={items().length > 0}
      fallback={
        <p class="verevon-activity-empty">
          {props.emptyLabel ?? 'Ingen aktivitet i dette rommet ennå.'}
        </p>
      }
    >
      <ul class="verevon-activity-feed">
        <For each={items()}>
          {(item) => (
            <li class={rowClass(item)}>
              {/* An item whose owning Space is unknown carries no href (the
                  `/chat` adoption fallback was removed). Render it as a span
                  rather than an anchor with no destination: an `<a>` without
                  href is announced as a link and focusable, promising a
                  navigation that does not exist. */}
              <Dynamic
                component={item.href ? 'a' : 'span'}
                class="verevon-activity-link"
                href={item.href}
                link={item.href ? true : undefined}
              >
                <span class="verevon-activity-verb">{item.verb}</span>
                {': '}
                <span class="verevon-activity-object">{item.object}</span>
                {' → '}
                <span
                  class={toneClass(item)}
                  /* "Never go dark": a moving row announces itself to assistive
                     tech as it changes, rather than silently mutating. */
                  aria-live={item.outcome.live ? 'polite' : undefined}
                >
                  {item.outcome.label}
                </span>
              </Dynamic>
              <Show when={item.at}>
                {(at) => <span class="verevon-activity-when">{formatWhen(at())}</span>}
              </Show>
            </li>
          )}
        </For>
      </ul>
    </Show>
  )
}
