import { A } from '@solidjs/router'
import { For } from 'solid-js'
import { supportSurfaceHref, type SupportSurface } from '@/features/support/lib/support-navigation'
import { useI18n } from '@/shared/i18n'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'

export function SupportTopTabs(props: { active: SupportSurface }) {
  const i18n = useI18n()
  const tabs = () => [
    { id: 'conversations' as const, label: i18n.tr('Samtaler', 'Conversations') },
    { id: 'tickets' as const, label: i18n.tr('Saksbehandling', 'Ticketing') },
    { id: 'outbound' as const, label: i18n.tr('Utgående', 'Outbound') },
  ]

  return (
    <nav class="verevon-support-switcher" aria-label={i18n.tr('Supportvisning', 'Support view')} role="tablist">
      <For each={tabs()}>
        {(tab) => (
          <A
            href={supportSurfaceHref(tab.id)}
            role="tab"
            aria-selected={props.active === tab.id}
            tabIndex={props.active === tab.id ? 0 : -1}
            onKeyDown={handleTabKeyDown}
            classList={{ 'verevon-support-switcher__item--active': props.active === tab.id }}
          >
            {tab.label}
          </A>
        )}
      </For>
    </nav>
  )
}
