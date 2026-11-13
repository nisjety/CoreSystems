import { onSettled, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import { chatConnectionPillEnabled, chatConnectivity, watchChatConnectivity } from '../lib/chat-connectivity'
import './ConnectionPill.css'

/**
 * Says plainly, above the composer, when the chat is offline or cannot reach its service right now (F-t3-078).
 * `online` renders nothing, so a good connection looks exactly as it did before. Behind
 * `VITE_CHAT_CONNECTION_PILL=true`. Presentation only: it never gates a send.
 */
export function ConnectionPill() {
  const i18n = useI18n()
  if (!chatConnectionPillEnabled()) return null
  // Started once settled, not in the component body: the watcher's first sync may write a signal.
  // The returned stop function is the cleanup.
  onSettled(() => watchChatConnectivity())

  return (
    <Show when={chatConnectivity() !== 'online'}>
      <p class="verevon-chat-connection-pill" role="status" data-state={chatConnectivity()}>
        {chatConnectivity() === 'offline'
          ? i18n.tr('Frakoblet', 'Offline')
          : i18n.tr('Kobler til på nytt…', 'Reconnecting…')}
      </p>
    </Show>
  )
}
