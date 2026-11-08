import { useI18n } from '@/shared/i18n'
import type { SpaceFormatter } from '@/shared/i18n/format'
import { currentPreferences, formatPrefsFrom } from './preferences-store'

/**
 * The viewer's date and time formatter: the UI locale plus the date format,
 * time format and time zone the server says they chose. Pass the result
 * straight to `formatWhen` / `formatWhenWithTime` / `formatRelativeWhen`.
 *
 * Both fields are getters, so a call made inside JSX or a memo re-runs when the
 * locale or the preferences change. Works without a provider: before the
 * preferences load (or when signed out) `prefs` is undefined and the locale's
 * own defaults apply.
 */
export function useFormat(): SpaceFormatter {
  const i18n = useI18n()
  return {
    get locale() {
      return i18n.locale()
    },
    get prefs() {
      return formatPrefsFrom(currentPreferences())
    },
  }
}
