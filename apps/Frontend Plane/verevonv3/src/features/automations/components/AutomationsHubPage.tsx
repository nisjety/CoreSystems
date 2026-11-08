import { ArrowLeft } from '@/shared/icons'

import { useI18n } from '@/shared/i18n'
import { AutomationsHub } from './AutomationsHub'

/**
 * `/agents/automations`: the page frame around the hub. Reached by URL in this slice;
 * the agents landing links here once EP-6-S5 lands.
 */
export default function AutomationsHubPage() {
  const i18n = useI18n()

  return (
    <div class="verevon-automations-page">
      <header class="verevon-automations-page__topbar">
        <a href="/agents" link class="verevon-automations-page__back" aria-label={i18n.tr('Tilbake til agenter', 'Back to agents')}>
          <ArrowLeft size={16} />
        </a>
        <div>
          <p class="verevon-space-eyebrow">{i18n.tr('Agenter', 'Agents')}</p>
          <h1>{i18n.tr('Automatiseringer', 'Automations')}</h1>
        </div>
      </header>

      <p class="verevon-automations-page__intro">
        {i18n.tr(
          'Planlagte kjøringer du har laget. Pause dem, endre tidspunktet, forny godkjenningen, dupliser eller slett dem her.',
          'Scheduled runs you created. Pause them, change their time, renew their approval, duplicate or delete them here.',
        )}
      </p>

      <AutomationsHub />
    </div>
  )
}
