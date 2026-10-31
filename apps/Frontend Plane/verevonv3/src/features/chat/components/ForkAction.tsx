import { createSignal, Show } from 'solid-js'
import { forkChatThread } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

/** The fork action is shown only when the build turns it on, as the other chat switches are. */
export const threadForkEnabled = () => import.meta.env.VITE_CHAT_THREAD_FORK === 'true'

/**
 * "Fork from here" on an assistant turn (build plan WS17 S6): starts a new chat from a copy of
 * this one, up to the turn. The control goes away once the deployment answers that forking is
 * off, and a refused copy says so where the person is looking. The new thread is handed to
 * `onForked` when the caller wants to open it.
 */
export function ForkAction(props: {
  threadId: string
  messageId: string
  onForked?: (threadId: string) => void
}) {
  const i18n = useI18n()
  const [busy, setBusy] = createSignal(false)
  const [off, setOff] = createSignal(false)
  const [refused, setRefused] = createSignal(false)

  async function fork() {
    if (busy()) return
    setBusy(true)
    setRefused(false)
    try {
      const session = await forkChatThread(props.threadId, { fromMessageId: props.messageId })
      if (session?.threadId) props.onForked?.(session.threadId)
    } catch (error) {
      if (codeOf(error) === 'history_fork_disabled') setOff(true)
      else setRefused(true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Show when={!off()}>
      <button type="button" disabled={busy()} onClick={() => void fork()}>
        {busy() ? i18n.tr('Forgrener …', 'Forking …') : i18n.tr('Forgren herfra', 'Fork from here')}
      </button>
      <Show when={refused()}>
        <span role="alert">{i18n.tr('Samtalen kunne ikke forgrenes.', 'This chat could not be forked.')}</span>
      </Show>
    </Show>
  )
}

function codeOf(error: unknown): string | undefined {
  if (typeof error !== 'object' || error === null || !('code' in error)) return undefined
  return typeof error.code === 'string' ? error.code : undefined
}
