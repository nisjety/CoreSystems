import type { JSX } from '@solidjs/web'
import { Globe, MessageSquareText } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'

// The chat header control that shows or hides the browser body of the dock (WS9.H). It takes the chat
// header's own button class, so it reads like the other header actions. Presentation only: the page
// decides what a press does.
export function OpenBrowserButton(props: { open: () => boolean; onToggle: () => void }): JSX.Element {
  const i18n = useI18n()
  return (
    <button
      type="button"
      class="verevon-chat-header-button"
      aria-pressed={props.open() ? 'true' : 'false'}
      title={props.open()
        ? i18n.tr('Skjul nettleseren', 'Hide the browser')
        : i18n.tr('Vis nettleseren ved siden av samtalen', 'Show the browser beside the chat')}
      onClick={() => props.onToggle()}
    >
      <Globe size={14} aria-hidden="true" />
      <span>{i18n.tr('Nettleser', 'Browser')}</span>
    </button>
  )
}

// "Ask about this page": moves the person to the chat composer. Nothing from the page is attached: page
// context is governed by F-t2-028, and this cut sends only what the person types.
export function AskAboutPageButton(props: { onAsk: () => void }): JSX.Element {
  const i18n = useI18n()
  return (
    <button
      type="button"
      class="browser-dock-pane__ask"
      title={i18n.tr(
        'Gå til meldingsfeltet. Ingen innhold fra siden legges ved.',
        'Go to the message box. No page content is attached.',
      )}
      onClick={() => props.onAsk()}
    >
      <MessageSquareText size={13} aria-hidden="true" />
      <span>{i18n.tr('Spør om denne siden', 'Ask about this page')}</span>
    </button>
  )
}
