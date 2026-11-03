import { createSignal, For, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { getChangeDiff, type ChangeDiff, type ChangeDiffOp } from '@/shared/api/monitoring-client'
import { changeDiffMessage, diffReasonMessage } from '@/features/ingestions/lib/errors'

/**
 * The text difference between two stored versions of a page: which paragraphs
 * were added, removed or moved. Without a `diffId` it compares the latest two
 * versions; with one it reads that stored diff. Quarry answers 501 while
 * `QUARRY_EDGE__CHANGE_TEXT_DIFF` is off, and the copy says so.
 */
export function ChangeDiffPanel(props: { url: string; diffId?: string | null }) {
  const i18n = useI18n()
  const tr = i18n.tr
  const [view, setView] = createSignal<ChangeDiff | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)

  async function load() {
    setLoading(true)
    setError(null)
    try {
      setView(await getChangeDiff({ url: props.url, diffId: props.diffId ?? undefined }))
    } catch (cause) {
      setView(null)
      setError(changeDiffMessage(cause, tr, { no: 'Kunne ikke hente endringen.', en: 'Could not load the change.' }))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div class="ingestions-change-diff">
      <Button size="xs" variant="secondary" disabled={loading()} onClick={() => void load()}>
        {tr('Vis endringen', 'Show the change')}
      </Button>
      <Show when={error()}>{(message) => <div class="ingestions-alert" role="alert">{message()}</div>}</Show>
      <Show when={view()}>{(result) => <DiffBody view={result()} />}</Show>
    </div>
  )
}

function DiffBody(props: { view: ChangeDiff }) {
  const tr = useI18n().tr
  return (
    <Show
      when={props.view.diff}
      fallback={
        <p class="ingestions-muted">{diffReasonMessage(props.view.reason, tr) ?? tr('Det finnes ingen endring å vise.', 'There is no change to show.')}</p>
      }
    >
      {(diff) => (
        <div class="ingestions-code-card">
          <p class="ingestions-muted">
            {diff().realDelta
              ? tr(
                  `${diff().added} lagt til · ${diff().removed} fjernet · ${diff().moved} flyttet · ${diff().unchanged} uendret`,
                  `${diff().added} added · ${diff().removed} removed · ${diff().moved} moved · ${diff().unchanged} unchanged`,
                )
              : tr('Bare bytene på siden endret seg. Teksten er den samme.', 'Only the bytes on the page changed. The text is the same.')}
          </p>
          <ol class="ingestions-diff-ops">
            <For each={diff().ops}>{(op) => <DiffOpRow op={op} />}</For>
          </ol>
          <Show when={diff().truncated}>
            <p class="ingestions-muted">{tr('Listen er kuttet. Det finnes flere endringer.', 'The list is cut short. There are more changes.')}</p>
          </Show>
        </div>
      )}
    </Show>
  )
}

function DiffOpRow(props: { op: ChangeDiffOp }) {
  const tr = useI18n().tr
  const label = () => (props.op.op === 'added' ? tr('Lagt til', 'Added') : props.op.op === 'removed' ? tr('Fjernet', 'Removed') : tr('Flyttet', 'Moved'))
  // Quarry numbers paragraphs from zero; the page shows them from one.
  const place = () => `${(props.op.from ?? 0) + 1} → ${(props.op.to ?? 0) + 1}`
  return (
    <li class={`ingestions-diff-op ingestions-diff-op--${props.op.op}`}>
      <strong>{label()}</strong>
      <Show when={props.op.op === 'moved'} fallback={<span>{props.op.text ?? ''}</span>}>
        <span class="ingestions-mono">{tr(`avsnitt ${place()}`, `paragraph ${place()}`)}</span>
      </Show>
    </li>
  )
}
