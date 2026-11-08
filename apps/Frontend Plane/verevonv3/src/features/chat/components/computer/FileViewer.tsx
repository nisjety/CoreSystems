import { Show } from 'solid-js'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { envelope } from '@/shared/chat-nodes/tool-presentation'
import { useI18n } from '@/shared/i18n'

function pathOf(args: unknown): string | undefined {
  if (!args || typeof args !== 'object') return undefined
  const { path } = args as Record<string, unknown>
  return typeof path === 'string' && path.trim() ? path.trim() : undefined
}

/**
 * The file viewer of the computer panel (P1): a workspace file or listing, read as the text its tool returned, under
 * the path the call named. The text is shown as text, never as markup. It reads and changes nothing.
 */
export function FileViewer(props: { call: ChatToolCall }) {
  const i18n = useI18n()
  const path = () => pathOf(props.call.args)
  const truncated = () => envelope(props.call.output)?.truncated === true

  return (
    <div class="verevon-computer-viewer" data-computer-viewer="file">
      <Show when={path()}>
        {(name) => <div class="verevon-computer-viewer__meta">{name()}</div>}
      </Show>
      <Show when={props.call.output}>
        {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
      </Show>
      <Show when={truncated()}>
        <div class="verevon-computer-viewer__meta">{i18n.tr('Filen ble avkortet', 'The file was truncated')}</div>
      </Show>
    </div>
  )
}
