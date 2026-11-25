import { For, Show, createSignal } from 'solid-js'

import {
  ago,
  customSnooze,
  customSnoozeBounds,
  customSnoozeProblemText,
  headlineText,
  itemHref,
  itemSalience,
  itemSentence,
  needsYouSentence,
  SNOOZE_PRESETS,
  snoozeLabel,
  type CustomSnoozeProblem,
} from '@/features/attention/lib/attention-grammar'
import { actorDisplayName, createRosterNames } from '@/features/attention/lib/attention-roster'
import { ThreadFollowControl } from '@/features/attention/components/ThreadFollowControl'
import type { AttentionItem, AttentionNeedsYou } from '@/shared/api/spaces-attention-items-client'
import { useI18n } from '@/shared/i18n'

export interface AttentionItemRowProps {
  readonly item: AttentionItem
  /** The row is being changed; its buttons wait. */
  readonly busy: boolean
  readonly onState: (
    item: AttentionItem,
    change: { readonly state: 'done' | 'open' } | { readonly state: 'snoozed'; readonly snoozeUntil: number },
  ) => void
}

/**
 * One stored inbox row: a sentence, the released title (if any), and Done / Snooze / Reopen. A row
 * about a post also opens that post's Follow / Mute choice (C-PRIO-05), read only when opened.
 */
export function AttentionItemRow(props: AttentionItemRowProps) {
  const i18n = useI18n()
  const [snoozing, setSnoozing] = createSignal(false)
  const [custom, setCustom] = createSignal(false)
  const [customAt, setCustomAt] = createSignal('')
  const [customProblem, setCustomProblem] = createSignal<CustomSnoozeProblem | null>(null)
  const [following, setFollowing] = createSignal(false)
  // The room's roster names the person or agent a row is about. Until it answers, the fallback words stand.
  const names = createRosterNames(() => props.item.spaceRef)
  const headline = () => headlineText(props.item, i18n.tr)
  const salience = () => itemSalience(props.item.kind)
  const sentence = () => itemSentence(props.item, i18n.tr, actorDisplayName(props.item.actor, names()))

  // A typed time is checked against the same window the server enforces; a refusal says why and changes nothing.
  function submitCustom() {
    const result = customSnooze(customAt(), Date.now())
    if (!result.ok) {
      setCustomProblem(result.problem)
      return
    }
    setSnoozing(false)
    setCustom(false)
    setCustomProblem(null)
    props.onState(props.item, { state: 'snoozed', snoozeUntil: result.snoozeUntil })
  }

  return (
    <li class="verevon-attention-row" data-kind={props.item.kind} data-salience={salience()}>
      <a class="verevon-attention-row__main" href={itemHref(props.item)} link>
        <span class="verevon-attention-row__sentence">{sentence()}</span>
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
            onClick={() => {
              setSnoozing((open) => !open)
              setCustom(false)
              setCustomProblem(null)
            }}
          >
            {i18n.tr('Utsett', 'Snooze')}
          </button>
        </Show>
        <Show when={props.item.threadId}>
          <button
            type="button"
            class="verevon-attention-action"
            aria-expanded={following() ? 'true' : 'false'}
            onClick={() => setFollowing((open) => !open)}
          >
            {i18n.tr('Varsler', 'Notifications')}
          </button>
        </Show>
      </div>
      <Show when={following() ? props.item.threadId : null}>
        {(threadId) => <ThreadFollowControl spaceRef={props.item.spaceRef} threadId={threadId()} attentionOn />}
      </Show>
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
          <button
            type="button"
            class="verevon-attention-action"
            aria-expanded={custom() ? 'true' : 'false'}
            disabled={props.busy}
            onClick={() => {
              setCustomProblem(null)
              if (!custom()) setCustomAt(customSnoozeBounds(Date.now()).initial)
              setCustom((open) => !open)
            }}
          >
            {i18n.tr('Egendefinert', 'Custom')}
          </button>
        </div>
        <Show when={custom()}>
          <div class="verevon-attention-row__custom" role="group" aria-label={i18n.tr('Egendefinert tidspunkt', 'Custom time')}>
            <input
              type="datetime-local"
              class="verevon-attention-row__picker"
              aria-label={i18n.tr('Utsett til tidspunkt', 'Snooze until this time')}
              min={customSnoozeBounds(Date.now()).min}
              max={customSnoozeBounds(Date.now()).max}
              value={customAt()}
              disabled={props.busy}
              onInput={(event) => setCustomAt(event.currentTarget.value)}
            />
            <button type="button" class="verevon-attention-action" disabled={props.busy} onClick={() => submitCustom()}>
              {i18n.tr('Utsett til dette', 'Snooze to this time')}
            </button>
          </div>
          <Show when={customProblem()}>
            {(problem) => (
              <p class="verevon-attention__notice verevon-attention-row__problem" role="alert">
                {customSnoozeProblemText(problem(), i18n.tr)}
              </p>
            )}
          </Show>
        </Show>
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
