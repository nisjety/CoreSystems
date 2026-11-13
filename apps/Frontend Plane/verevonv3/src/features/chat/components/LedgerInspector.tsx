/**
 * The inspector drawer of the run ledger (EP-14 S4): the ids, status, recorded times and receipts of
 * the record the reader picked, with copy buttons for the request id and the run id.
 *
 * It shows only what the Trace already holds. A time that was not recorded reads as such and is never
 * filled in. Copying takes no focus away from the list. With `VITE_CHAT_REQUEST_INSPECTOR` on, a turn
 * with a run id also offers the request inspector (EP-14 S7).
 */

import { createSignal, For, Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { uiEventLabel } from '@/shared/chat/verevon-ui-events'
import { useI18n } from '@/shared/i18n'
import { formatLatency } from './chat-media-markdown'
import { formatClock, recordAt, roundNumberOf, spanMs, type LedgerRecord, type LedgerTurn } from '../lib/run-ledger'
import { RequestInspector, requestInspectorEnabled } from './RequestInspector'

type CopyState = 'request' | 'run' | 'failed' | null

export function LedgerInspector(props: {
  turn: LedgerTurn
  record: LedgerRecord
  /** The record's title, as the list shows it. */
  title: string
  /** The record's status in words, when it has one. */
  status?: string
  onClose: () => void
}) {
  const i18n = useI18n()
  const [copied, setCopied] = createSignal<CopyState>(null)

  const copy = async (which: 'request' | 'run', value: string) => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(which)
    } catch {
      setCopied('failed')
    }
  }

  const notRecorded = () => i18n.tr('ikke registrert', 'not recorded')
  const clock = (at: string | undefined) => (at === undefined ? notRecorded() : formatClock(at, i18n.locale()) || notRecorded())
  const duration = (ms: number | undefined) => (ms === undefined ? notRecorded() : formatLatency(ms))
  const call = () => (props.record.kind === 'call' ? props.record : undefined)
  // Known only when the turn's timing carried rounds and the record's time fell inside one.
  const round = () => roundNumberOf(props.turn, props.record.key)
  const lifecycle = () => props.turn.records.flatMap((record) => (
    record.kind === 'event' && record.event.type.startsWith('lifecycle.') ? [record] : []
  ))
  const copyMessage = () => {
    switch (copied()) {
      case 'request': return i18n.tr('Forespørsel-id er kopiert.', 'Request id copied.')
      case 'run': return i18n.tr('Kjørings-id er kopiert.', 'Run id copied.')
      case 'failed': return i18n.tr('Kunne ikke kopiere. Merk id-en og kopier den selv.', 'Could not copy. Select the id and copy it yourself.')
      default: return ''
    }
  }

  return (
    <aside class="verevon-chat-ledger__inspector" aria-label={i18n.tr('Detaljer for valgt post', 'Details for the selected record')}>
      <header>
        <h3>{props.title}</h3>
        <button type="button" class="verevon-chat-ledger__close" onClick={() => props.onClose()}>
          {i18n.tr('Lukk', 'Close')}
        </button>
      </header>
      <dl class="verevon-run-proof__receipt">
        <Field label={i18n.tr('Forespørsel-id', 'Request id')}>
          <Show when={props.turn.requestId} fallback={notRecorded()}>
            {(id) => (
              <span class="verevon-chat-ledger__id">
                <code title={id()}>{id()}</code>
                <button type="button" onClick={() => void copy('request', id())}>
                  {i18n.tr('Kopier forespørsel-id', 'Copy request id')}
                </button>
              </span>
            )}
          </Show>
        </Field>
        <Field label={i18n.tr('Kjørings-id', 'Run id')}>
          <Show when={props.turn.runId} fallback={notRecorded()}>
            {(id) => (
              <span class="verevon-chat-ledger__id">
                <code title={id()}>{id()}</code>
                <button type="button" onClick={() => void copy('run', id())}>
                  {i18n.tr('Kopier kjørings-id', 'Copy run id')}
                </button>
              </span>
            )}
          </Show>
        </Field>
        <Show when={call()}>
          {(entry) => (
            <>
              <Field label={i18n.tr('Verktøykall-id', 'Tool call id')}>
                <code>{entry().id ?? notRecorded()}</code>
              </Field>
              <Field label={i18n.tr('Status', 'Status')}>{props.status ?? notRecorded()}</Field>
              <Field label={i18n.tr('Start', 'Start')}>{clock(entry().startAt)}</Field>
              <Field label={i18n.tr('Slutt', 'End')}>{clock(entry().endAt)}</Field>
              <Field label={i18n.tr('Varighet', 'Duration')}>{duration(entry().durationMs)}</Field>
            </>
          )}
        </Show>
        <Show when={call() === undefined}>
          <Field label={i18n.tr('Tidspunkt', 'Time')}>{clock(recordAt(props.record))}</Field>
        </Show>
        <Show when={round() !== undefined}>
          <Field label={i18n.tr('Runde', 'Round')}>{round()}</Field>
        </Show>
        <Field label={i18n.tr('Turens tidsrom', 'Turn span')}>
          {duration(spanMs(props.turn.startAt, props.turn.endAt))}
        </Field>
        <Field label={i18n.tr('Kvittering', 'Receipt')}>
          <Show when={props.turn.receiptIds.length > 0} fallback={notRecorded()}>
            <For each={props.turn.receiptIds}>
              {(receiptId) => <code title={receiptId}>{receiptId}</code>}
            </For>
          </Show>
        </Field>
      </dl>
      <Show when={props.record.kind === 'unmatched'}>
        <p>{i18n.tr('Ingen kall med denne id-en er registrert i turen.', 'No call with this id is recorded in this turn.')}</p>
      </Show>
      <Show when={call() !== undefined && call()?.durationMs === undefined}>
        <p>{i18n.tr('Varigheten er ikke registrert for dette kallet.', 'Timing is not recorded for this call.')}</p>
      </Show>
      <Show when={lifecycle().length > 0}>
        <h4>{i18n.tr('Livssyklushendelser', 'Lifecycle notices')}</h4>
        <For each={lifecycle()}>
          {(record) => (
            <p>
              <strong>{uiEventLabel(record.event)}</strong> <time>{clock(record.event.at)}</time>
            </p>
          )}
        </For>
      </Show>
      <Show when={requestInspectorEnabled && props.turn.runId}>
        {(runId) => (
          <>
            <h4>{i18n.tr('Forespørsel', 'Request')}</h4>
            <RequestInspector runId={runId()} />
          </>
        )}
      </Show>
      <p class="verevon-chat-ledger__copy-status" role="status">{copyMessage()}</p>
    </aside>
  )
}

function Field(props: { label: string; children: JSX.Element }) {
  return (
    <div class="verevon-run-proof__receipt-row">
      <dt>{props.label}</dt>
      <dd>{props.children}</dd>
    </div>
  )
}
