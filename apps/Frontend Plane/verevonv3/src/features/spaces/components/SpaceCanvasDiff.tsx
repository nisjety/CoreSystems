import { createMemo, For, Show } from 'solid-js'

import { diffText, type DiffLine } from '@/shared/chat-nodes/diff'
import { useI18n } from '@/shared/i18n'

/**
 * The line difference between two texts of the room canvas (WS19-S7): the hunks `diffText` computes, the shared line
 * diff and not the chat's DiffView. Each changed line carries a sign as well as a colour, so the change is not shown
 * by colour alone. A text too long to compare line by line says so, rather than showing no change.
 */
export interface SpaceCanvasDiffProps {
  /** The text the change starts from. */
  readonly before: string
  /** The text after the change. */
  readonly after: string
}

const SIGN: Readonly<Record<DiffLine['kind'], string>> = { added: '+', removed: '-', context: ' ' }

export function SpaceCanvasDiff(props: SpaceCanvasDiffProps) {
  const tr = useI18n().tr
  const diff = createMemo(() => diffText(props.before, props.after))
  return (
    <Show
      when={!diff().truncated}
      fallback={
        <p class="verevon-space-canvas__hint">
          {tr('Teksten er for lang til å sammenligne linje for linje.', 'The text is too long to compare line by line.')}
        </p>
      }
    >
      <Show
        when={diff().hunks.length > 0}
        fallback={<p class="verevon-space-canvas__hint">{tr('Ingen endringer.', 'No differences.')}</p>}
      >
        <p class="verevon-space-canvas__meta">
          {tr(
            `Lagt til: ${diff().stat.added} · Fjernet: ${diff().stat.removed}`,
            `Added: ${diff().stat.added} · Removed: ${diff().stat.removed}`,
          )}
        </p>
        <div class="verevon-space-canvas__diff">
          <For each={diff().hunks}>
            {(hunk) => (
              <div class="verevon-space-canvas__hunk">
                <For each={hunk.lines}>
                  {(line) => (
                    <div class={`verevon-space-canvas__diff-line verevon-space-canvas__diff-line--${line.kind}`}>
                      <span class="verevon-space-canvas__diff-sign" aria-hidden="true">
                        {SIGN[line.kind]}
                      </span>
                      <span>{line.text}</span>
                    </div>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </Show>
    </Show>
  )
}
