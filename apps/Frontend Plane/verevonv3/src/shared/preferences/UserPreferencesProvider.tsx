import { createEffect, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { subscribeLocaleChange, useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import '@/styles/ui-baseline.css'
import {
  applyDensity,
  clearPreferences,
  currentPreferences,
  densityFrom,
  startPreferencesBootstrap,
} from './preferences-store'

/**
 * Loads the signed-in user's server-held preferences once (and again after the
 * tab was hidden for a minute), applies locale precedence and the density
 * attribute, and writes language toggles through. It never blocks render and is
 * fail-open: an error keeps the cached locale and the defaults. Mounted inside
 * `I18nProvider` and outside `CoreShell`; signed-out pages keep cache-only behaviour.
 */
export function UserPreferencesProvider(props: { children?: JSX.Element }) {
  const i18n = useI18n()

  createEffect(
    () => getSession().status === 'authenticated',
    (authenticated) => {
      if (!authenticated) return
      const dispose = startPreferencesBootstrap({
        getLocale: () => untrack(() => i18n.locale()),
        applyServerLocale: i18n.applyServerLocale,
        subscribeLocaleChange,
      })
      return () => {
        dispose()
        clearPreferences()
      }
    },
  )

  createEffect(
    () => densityFrom(currentPreferences()),
    (density) => applyDensity(density),
  )

  return <>{props.children}</>
}
