// Central Norwegian (Bokmål) UI strings.
//
// Verevon's UI is Norwegian-facing, but user-visible copy has historically been
// scattered inline across components (e.g. the sidebar labels in
// features/core/lib/sidebar-navigation.ts). This module is the single source of
// truth for Norwegian copy so labels stay consistent and translatable.
//
// Convention: NEW features import their copy from here; existing scattered
// strings are migrated into this module incrementally. This is the seam — not a
// claim that every string in the app already routes through it.

// Limits the refusal sentences quote; the numbers live in
// features/chat/lib/attachment-policy.ts.
type AttachmentLimits = { maxFileBytes: number; maxTotalBytes: number; maxImages: number }
const megabytes = (bytes: number) => `${Math.round((bytes / 1_048_576) * 10) / 10} MB`

export const no = {
  leads: {
    title: 'Leads',
    subtitle:
      'Finn norske bedrifter i Enhetsregisteret etter bransje, sted og størrelse. Kun bedriftsdata — ingen kontakter eller navngitte personer.',
    naeringskode: 'Næringskode',
    kommunenummer: 'Kommunenummer',
    organisasjonsform: 'Organisasjonsform',
    minEmployees: 'Min. ansatte',
    maxEmployees: 'Maks. ansatte',
    search: 'Søk',
    searching: 'Søker …',
    employeeBandHint: 'Enhetsregisteret kan ikke filtrere på 1–4 ansatte; bruk 0 eller 5+.',
    noResults: 'Ingen bedrifter matchet disse filtrene.',
    nameList: 'Gi listen et navn',
    saveSelected: (count: number) => `Lagre ${count} som liste`,
    savedLists: 'Lagrede lister',
    loading: 'Laster …',
    noSavedLists: 'Ingen lagrede lister ennå.',
    exportCsv: 'Eksporter CSV',
    delete: 'Slett',
    companies: (count: number) => `${count} bedrifter`,
    saved: (name: string, count: number) => `Lagret «${name}» (${count} bedrifter).`,
    saveValidation: 'Gi listen et navn og velg minst én bedrift.',
    saveError: 'Kunne ikke lagre listen. Prøv igjen.',
    searchError: 'Søket feilet. Juster filtrene og prøv igjen.',
    columns: {
      navn: 'Navn',
      orgnr: 'Org.nr',
      form: 'Form',
      naering: 'Næring',
      sted: 'Sted',
      ansatte: 'Ansatte',
    },
  },
  attachments: {
    refusal: {
      unsupported_type: (name: string) =>
        `«${name}» ble ikke lagt ved: filtypen støttes ikke. Bruk bilde (PNG, JPEG, WebP, GIF) eller PDF, DOCX, XLSX, PPTX, TXT, MD, CSV, JSON eller HTML.`,
      too_large: (name: string, limits: AttachmentLimits) =>
        `«${name}» ble ikke lagt ved: for stor. Maks ${megabytes(limits.maxFileBytes)} per fil og ${megabytes(limits.maxTotalBytes)} samlet.`,
      route_unsupported: (name: string) =>
        `«${name}» ble ikke lagt ved: ChatGPT-abonnementet tar ikke imot bilder. Velg en annen modell for å bruke bilder.`,
      do_images_unsupported: (name: string) =>
        `«${name}» ble ikke lagt ved: Utfør kan ikke bruke bilder ennå. Bytt til Spør for å analysere bildet.`,
      too_many_images: (name: string, limits: AttachmentLimits) =>
        `«${name}» ble ikke lagt ved: bare ${limits.maxImages} bilde kan analyseres per melding.`,
      zdr_refused: (name: string) =>
        `«${name}» ble ikke lagt ved: midlertidig samtale tar ikke imot bilder.`,
    },
    notSent:
      'Bildet ble ikke sendt: ChatGPT-abonnementet tar ikke imot bilder. Fjern bildet eller velg en annen modell. Ingenting ble sendt.',
  },
  effectivePolicy: {
    title: 'Gjeldende policy',
    intro: 'Verdiene som gjelder her, og nivået som satte hver av dem. Et låsikon betyr at et høyere nivå bestemmer verdien.',
    empty: 'Ingen innstillinger gjelder her.',
    loadFailed: 'Gjeldende policy kunne ikke lastes.',
    setBy: (origin: string) => `Satt av: ${origin}`,
    sourceUnknown: 'Kilde ikke oppgitt',
    origin: {
      platform: 'Plattformstandard',
      org: 'Organisasjonen',
      space: 'Rommet',
      user: 'Deg',
      request: 'Denne forespørselen',
    },
    locked: 'Låst av et høyere nivå',
    unavailable: 'Kunne ikke leses akkurat nå',
    notSet: 'Ikke satt',
    redacted: 'Bare synlig for administratorer',
    redactedCount: (count: number) => `${count} oppføringer, bare synlig for administratorer`,
    on: 'På',
    off: 'Av',
    revision: (revision: number) => `Revisjon ${revision}`,
    keys: {
      approval_posture: 'Godkjenningsnivå',
      zdr: 'Null datalagring (ZDR)',
      privacy_tier_floor: 'Minste personvernnivå',
      model_access: 'Modelltilgang',
      routing: 'Ruting',
      budget: 'Budsjett',
      web_access: 'Tilgang til nettet',
      browser_site_mode: 'Modus for nettsider',
    },
  },
} as const
