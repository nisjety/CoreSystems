import type { ClientErrorClass } from '@/shared/ui/resilience/classify-client-error'

/**
 * Failure card copy, Norwegian first (WS07-S1). Kept here rather than in the
 * API error seam so a transport change never rewrites what the user reads.
 */
export interface FailureCopy {
  readonly titleNo: string
  readonly titleEn: string
  readonly bodyNo: string
  readonly bodyEn: string
}

export const FAILURE_COPY: Readonly<Record<ClientErrorClass, FailureCopy>> = {
  stale_deploy: {
    titleNo: 'En ny versjon er tilgjengelig',
    titleEn: 'A new version is available',
    bodyNo: 'Denne siden ble åpnet før en oppdatering. Last inn på nytt for å fortsette.',
    bodyEn: 'This page was opened before an update. Reload to continue.',
  },
  session_expired: {
    titleNo: 'Økten din er utløpt',
    titleEn: 'Your session has expired',
    bodyNo: 'Logg inn på nytt for å fortsette der du slapp.',
    bodyEn: 'Sign in again to pick up where you left off.',
  },
  gateway_unreachable: {
    titleNo: 'Får ikke kontakt med tjenesten',
    titleEn: 'Cannot reach the service',
    bodyNo: 'Sjekk nettforbindelsen din og prøv igjen om litt.',
    bodyEn: 'Check your connection and try again in a moment.',
  },
  render_error: {
    titleNo: 'Noe gikk galt her',
    titleEn: 'Something went wrong here',
    bodyNo: 'Resten av appen fungerer fortsatt. Prøv igjen, eller last inn siden på nytt.',
    bodyEn: 'The rest of the app still works. Try again, or reload the page.',
  },
}

export const FAILURE_ACTIONS = {
  reload: { no: 'Last inn på nytt', en: 'Reload' },
  retry: { no: 'Prøv igjen', en: 'Retry' },
  signIn: { no: 'Logg inn', en: 'Sign in' },
  copyDetails: { no: 'Kopier detaljer', en: 'Copy details' },
  copied: { no: 'Kopiert', en: 'Copied' },
} as const
