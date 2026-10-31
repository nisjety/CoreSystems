import { For } from 'solid-js'
import { CalendarClock, FileText, Lightbulb } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'

export function ChatWorkLinks() {
  const i18n = useI18n()
  const links = () => [
    { href: '/chat/findings', label: i18n.tr('Funn', 'Findings'), icon: Lightbulb },
    { href: '/chat/reports', label: i18n.tr('Rapporter', 'Reports'), icon: FileText },
    { href: '/chat/routines', label: i18n.tr('Rutiner', 'Routines'), icon: CalendarClock },
  ]
  return (
    <nav class="chat-work-links" aria-label={i18n.tr('Chat-arbeidsområder', 'Chat work areas')}>
      <For each={links()}>{({ href, label, icon: Icon }) => (
        <a href={href} link>
          <Icon size={17} aria-hidden="true" />
          <span>{label}</span>
        </a>
      )}</For>
    </nav>
  )
}
