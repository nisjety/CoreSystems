import { For, Show, createSignal } from 'solid-js'

import {
  headlineText,
  itemHref,
  itemSalience,
  itemSentence,
  needsYouSentence,
  SNOOZE_PRESETS,
  snoozeLabel,
} from '@/features/attention/lib/attention-grammar'
import type { AttentionItem, AttentionNeedsYou } from '@/shared/api/spaces-attention-items-client'
import { useI18n } from '@/shared/i18n'

/** How long ago, in the viewer's language. An unreadable time draws nothing. */
function ago(at: number | null, locale: string, now: number): string {
  if (at === null || !Number.isFinite(at) || at <= 0) return ''
  const seconds = Math.round((at - now) / 1000)
  const abs = Math.abs(seconds)
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  if (abs < 60) return formatter.format(0, 'second')
  if (abs < 3600) return formatter.format(Math.round(seconds / 60), 'minute')
  if (abs < 86_400) return formatter.format(Math.round(seconds / 3600), 'hour')
  return formatter.format(Math.round(seconds / 86_400), 'day')
}

export interface AttentionItemRowProps {
  readonly item: AttentionItem
  /** The row is being changed; its buttons wait. */
  readonly busy: boolean
  readonly onState: (
    item: AttentionItem,
    change: { readonly state: 'done' | 'open' } | { readonly state: 'snoozed'; readonly snoozeUntil: number },
  ) => void
}

/** One stored inbox row: a sentence, the released title (if any), and Done / Snooze / Reopen. */
export function AttentionItemRow(props: AttentionItemRowProps) {
  const i18n = useI18n()
  const [snoozing, setSnoozing] = createSignal(false)
  const headline = () => headlineText(props.item, i18n.tr)
  const salience = () => itemSalience(props.item.kind)

  return (
    <li class="verevon-attention-row" data-kind={props.item.kind} data-salience={salience()}>
      <a class="verevon-attention-row__main" href={itemHref(props.item)} link>
        <span class="verevon-attention-row__sentence">{itemSentence(props.item, i18n.tr)}</span>
        <Show when={headline()}>
          {(text) => (
            <span class="verevon-attention-row__headline" data-state={props.item.headlineState}>
              {text()}
            </span>
          )}
        </Show>
        <time class="verevon-attention-row__time">{ago(props.item.updatedAt, i18n.locale(), Date.now())}</time>
      </a>
      <div class="verevon-attention-row__actions">
        <Show
          when={props.item.state === 'open'}
          fallback={
            <button
              type="button"
              class="verevon-attention-action"
              disabled={props.busy}
              onClick={() => props.onState(props.item, { state: 'open' })}
            >
              {i18n.tr('Åpne igjen', 'Reopen')}
            </button>
          }
        >
          <button
            type="button"
            class="verevon-attention-action"
            disabled={props.busy}
            onClick={() => props.onState(props.item, { state: 'done' })}
          >
            {i18n.tr('Ferdig', 'Done')}
          </button>
          <button
            type="button"
            class="verevon-attention-action"
            aria-expanded={snoozing() ? 'true' : 'false'}
            disabled={props.busy}
            onClick={() => setSnoozing((open) => !open)}
          >
            {i18n.tr('Utsett', 'Snooze')}
          </button>
        </Show>
      </div>
      <Show when={snoozing() && props.item.state === 'open'}>
        <div class="verevon-attention-row__snooze" role="group" aria-label={i18n.tr('Utsett til', 'Snooze until')}>
          <For each={SNOOZE_PRESETS}>
            {(preset) => (
              <button
                type="button"
                class="verevon-attention-action"
                disabled={props.busy}
                onClick={() => {
                  setSnoozing(false)
                  props.onState(props.item, { state: 'snoozed', snoozeUntil: Date.now() + preset.ms })
                }}
              >
                {snoozeLabel(preset.id, i18n.tr)}
              </button>
            )}
          </For>
        </div>
      </Show>
    </li>
  )
}

/** One live approval waiting on the viewer. Opening it leads to the post where it is decided. */
export function AttentionNeedsYouRow(props: { readonly entry: AttentionNeedsYou }) {
  const i18n = useI18n()
  const headline = () => headlineText(props.entry, i18n.tr)
  return (
    <li class="verevon-attention-row" data-kind="needs_you" data-salience="critical">
      <a class="verevon-attention-row__main" href={itemHref(props.entry)} link>
        <span class="verevon-attention-row__sentence">{needsYouSentence(props.entry, i18n.tr)}</span>
        <Show when={headline()}>
          {(text) => (
            <span class="verevon-attention-row__headline" data-state={props.entry.headlineState}>
              {text()}
            </span>
          )}
        </Show>
        <time class="verevon-attention-row__time">{ago(props.entry.requestedAt, i18n.locale(), Date.now())}</time>
      </a>
    </li>
  )
}
