import { Show } from 'solid-js'
import type { ChatToolCall } from '@/features/chat/components/chat-types'
import { envelope } from '@/shared/chat-nodes/tool-presentation'
import { useI18n } from '@/shared/i18n'

type Streams = { stdout: string; stderr: string; exitCode?: number; truncated: boolean }

/** The streams of a run that reported them in its envelope, or `null` for plain text. */
function streamsOf(output: string | undefined): Streams | null {
  const body = envelope(output)
  if (!body || (typeof body.stdout !== 'string' && typeof body.stderr !== 'string')) return null
  return {
    stdout: typeof body.stdout === 'string' ? body.stdout : '',
    stderr: typeof body.stderr === 'string' ? body.stderr : '',
    exitCode: typeof body.exit_code === 'number' ? body.exit_code : undefined,
    truncated: body.truncated === true,
  }
}

function commandOf(args: unknown): string | undefined {
  if (!args || typeof args !== 'object') return undefined
  const { command } = args as Record<string, unknown>
  return typeof command === 'string' && command.trim() ? command.trim() : undefined
}

/**
 * The terminal viewer of the computer panel (P1): a command or a process, read as the transcript its tool returned.
 * The output is text in a monospace block, never markup. A code run that reports its streams shows stdout and stderr
 * apart, with its exit code. It runs nothing: a viewer reads what the transcript already holds.
 */
export function TerminalViewer(props: { call: ChatToolCall }) {
  const i18n = useI18n()
  const streams = () => streamsOf(props.call.output)
  const command = () => commandOf(props.call.args)

  return (
    <div class="verevon-computer-viewer" data-computer-viewer="terminal">
      <Show when={command()}>
        {(line) => <div class="verevon-computer-viewer__meta">$ {line()}</div>}
      </Show>
      <Show
        when={streams()}
        fallback={
          <Show when={props.call.output}>
            {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
          </Show>
        }
      >
        {(value) => (
          <>
            <Show when={value().stdout}>
              {(text) => <pre class="verevon-computer-viewer__output">{text()}</pre>}
            </Show>
            <Show when={value().stderr}>
              {(text) => <pre class="verevon-computer-viewer__stderr">{text()}</pre>}
            </Show>
            <Show when={value().exitCode !== undefined}>
              <div class="verevon-computer-viewer__meta">
                {i18n.tr('Avslutningskode', 'Exit code')} {value().exitCode}
              </div>
            </Show>
          </>
        )}
      </Show>
      <Show when={streams()?.truncated}>
        <div class="verevon-computer-viewer__meta">{i18n.tr('Utdataen ble avkortet', 'The output was truncated')}</div>
      </Show>
    </div>
  )
}
