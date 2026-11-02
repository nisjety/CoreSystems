import { Show } from 'solid-js'

import { dismissNewVersion, newVersionBuildId } from '@/shared/build/version-check'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import '@/shared/ui/resilience/resilience.css'

/**
 * Non-blocking notice that a newer build is live (WS07-S2). `role="status"`, so it
 * is announced politely and never traps focus. Reload is the user's choice: this
 * banner never reloads the page itself, so an unsent draft is never discarded.
 */
export function NewVersionBanner() {
  const i18n = useI18n()
  return (
    <Show when={newVersionBuildId() !== null}>
      <div class="new-version-banner" role="status" aria-live="polite" data-testid="new-version-banner">
        <span>
          {i18n.tr(
            'En ny versjon av Verevon er tilgjengelig.',
            'A new version of Verevon is available.',
          )}
        </span>
        <Button variant="primary" size="sm" onClick={() => window.location.reload()}>
          {i18n.tr('Last på nytt', 'Reload')}
        </Button>
        <Button variant="ghost" size="sm" onClick={dismissNewVersion}>
          {i18n.tr('Lukk', 'Dismiss')}
        </Button>
      </div>
    </Show>
  )
}
