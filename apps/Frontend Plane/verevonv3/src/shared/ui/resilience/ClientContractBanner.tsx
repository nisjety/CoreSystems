import { Show } from 'solid-js'

import { clientContractMismatch } from '@/shared/build/client-contract'
import { useI18n } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import '@/shared/ui/resilience/resilience.css'

/**
 * Non-blocking notice that this tab is older than the gateway still supports
 * (WS07-S3). `role="status"`, so it is announced politely and never traps focus.
 * Reload is the user's choice: the banner never reloads the page itself (the
 * `enforce` policy's own reload lives in `checkClientContract` and only runs when
 * no unsaved-work probe reports work).
 */
export function ClientContractBanner() {
  const i18n = useI18n()
  return (
    <Show when={clientContractMismatch()}>
      <div class="new-version-banner" role="status" aria-live="polite" data-testid="client-contract-banner">
        <span>
          {i18n.tr(
            'Denne versjonen av Verevon er utdatert og fungerer kanskje ikke riktig. Last på nytt for å oppdatere.',
            'This version of Verevon is out of date and may not work correctly. Reload to update.',
          )}
        </span>
        <Button variant="primary" size="sm" onClick={() => window.location.reload()}>
          {i18n.tr('Last på nytt', 'Reload')}
        </Button>
      </div>
    </Show>
  )
}
