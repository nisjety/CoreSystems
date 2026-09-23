import { For } from 'solid-js'
import { supportSurfaceHref, type SupportSurface } from '@/features/support/lib/support-navigation'
import { useI18n } from '@/shared/i18n'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'

export function SupportTopTabs(props: { active: SupportSurface }) {
  const i18n = useI18n()
  const tabs = () => [
    { id: 'conversations' as const, label: i18n.tr('Samtaler', 'Conversations') },
    // Drafts Verevon proposed, awaiting a person. They live in the support
    // workspace only — the chat page lists `origin == "chat"` threads and
    // never reads this queue.
    { id: 'drafts' as const, label: i18n.tr('Utkast', 'Drafts') },
    { id: 'tickets' as const, label: i18n.tr('Saksbehandling', 'Ticketing') },
    { id: 'outbound' as const, label: i18n.tr('Utgående', 'Outbound') },
    { id: 'remote' as const, label: i18n.tr('Fjernhjelp', 'Remote support') },
  ]

  return (
    <nav class="verevon-support-switcher" aria-label={i18n.tr('Supportvisning', 'Support view')} role="tablist">
      <For each={tabs()}>
        {(tab) => (
          <a
            href={supportSurfaceHref(tab.id)}
            link
            role="tab"
            aria-selected={props.active === tab.id ? 'true' : 'false'}
            tabindex={props.active === tab.id ? 0 : -1}
            onKeyDown={handleTabKeyDown}
            class={{ 'verevon-support-switcher__item--active': props.active === tab.id }}
          >
            {tab.label}
          </a>
        )}
      </For>
    </nav>
  )
}
