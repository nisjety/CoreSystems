import { Show } from 'solid-js'
import { dismissStaleBuildNotice, staleBuildNotice } from '@/app/chunk-recovery'
import { useI18n } from '@/shared/i18n'

/**
 * Shown when a part of the app could not load because a newer build replaced it
 * and the automatic reload did not (or must not) fix that. The reload here is the
 * user's choice, so a draft they are holding is never discarded for them.
 */
export function StaleBuildNotice() {
  const i18n = useI18n()
  return (
    <Show when={staleBuildNotice()}>
      <div
        role="status"
        aria-live="polite"
        data-testid="stale-build-notice"
        style={{
          position: 'fixed',
          left: '50%',
          bottom: '16px',
          transform: 'translateX(-50%)',
          'z-index': '1000',
          display: 'flex',
          'align-items': 'center',
          gap: '12px',
          'max-width': 'calc(100vw - 32px)',
          padding: '10px 16px',
          'border-radius': '12px',
          background: 'var(--card, #fff)',
          color: 'var(--card-foreground, #1a1a1a)',
          'box-shadow': '0 4px 24px rgba(0, 0, 0, 0.18)',
          'font-size': '13px',
        }}
      >
        <span>
          {i18n.tr(
            'En ny versjon er tilgjengelig. Last siden på nytt for å fortsette.',
            'A new version is available. Reload the page to continue.',
          )}
        </span>
        <button type="button" onClick={() => window.location.reload()}>
          {i18n.tr('Last på nytt', 'Reload')}
        </button>
        <button type="button" onClick={dismissStaleBuildNotice}>
          {i18n.tr('Lukk', 'Dismiss')}
        </button>
      </div>
    </Show>
  )
}
