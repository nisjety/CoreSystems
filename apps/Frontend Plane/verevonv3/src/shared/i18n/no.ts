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
  // Bekreftelse for null datalagring (ZDR) på en modelltilkobling som ikke kan gi ZDR (ZDR Z-20, Z-21, Z-21b).
  // UTKAST TIL FORMULERING, VENTER PÅ JURIST (OD-LEGAL, D-ZDR-09): eier og jurist bekrefter hver setning før den
  // tas i bruk. Dialogen bruker IKKE denne teksten til selve bekreftelsen: Integration Core skriver og lagrer
  // teksten, og dialogen viser den slik den er servert. Disse nøklene er rammen rundt: chipen, svarlinjen og
  // administratorlisten. Ingen av dem hevder ZDR for en chat med fravik, og de som beskriver dataene sier at
  // organisasjonens kunnskap og verktøyresultater som brukes i chatten, sendes til leverandøren.
  zdr: {
    waiver: {
      chip: 'Ikke ZDR i denne chatten',
      chip_title:
        'Denne chatten kjører på din egen tilkobling, som ikke kan gi null datalagring. Det du sender, også organisasjonens kunnskap og verktøyresultater som brukes i chatten, sendes til leverandøren, og chatten lagres som en vanlig chat.',
      answer_line:
        'Svaret ble laget på medlemmets egen tilkobling og lagres som en vanlig chat.',
      answer_line_title:
        'Ikke null datalagring. Organisasjonens kunnskap og verktøyresultater som ble brukt i denne chatten, ble sendt til leverandøren.',
      dialog: {
        loading: 'Laster ...',
        close: 'Lukk',
        saving: 'Lagrer ...',
        retry: 'Prøv igjen',
        loadFailed: 'Teksten kunne ikke lastes. Prøv igjen.',
        saveFailed: 'Bekreftelsen kunne ikke lagres. Prøv igjen.',
        changed: 'Teksten ble endret mens den var åpen. Les den nye teksten før du fortsetter.',
        unavailable: 'Denne tilkoblingen har ingen tekst å bekrefte akkurat nå.',
      },
      review: 'Se over null datalagring',
      revoke: 'Trekk tilbake bekreftelsen',
      revoke_confirm:
        'Trekke tilbake bekreftelsen din? Så lenge organisasjonen har null datalagring på, brukes ikke denne tilkoblingen før du bekrefter på nytt.',
      revoke_yes: 'Trekk tilbake',
      revoke_no: 'Behold',
      revoked: 'Bekreftelsen din er trukket tilbake.',
      revoke_failed: 'Bekreftelsen kunne ikke trekkes tilbake. Prøv igjen.',
      confirmed: 'Bekreftet.',
      send_again: 'Bekreftet. Du kan sende meldingen på nytt.',
      chat_review: 'Se over og bekreft',
      admin: {
        title: 'Bekreftelser',
        intro:
          'Hvilke medlemmer som har bekreftet at en tilkobling de eier ikke kan gi null datalagring. Listen viser hvem, hvilken leverandør og hvilken tekstversjon, aldri selve teksten.',
        loading: 'Laster ...',
        empty: 'Ingen bekreftelser ennå.',
        load_failed: 'Bekreftelsene kunne ikke lastes.',
        truncated: 'Bare de nyeste radene vises.',
        counts: (active: number, superseded: number, revoked: number) =>
          `${active} aktive, ${superseded} erstattet, ${revoked} trukket tilbake`,
        member: 'Medlem',
        provider: 'Leverandør',
        capability: 'Egenskap',
        text_version: 'Tekstversjon',
        locale: 'Språk',
        status: 'Status',
        acknowledged_at: 'Bekreftet',
        revoked_at: 'Trukket tilbake',
        status_active: 'Aktiv',
        status_superseded: 'Erstattet',
        status_revoked: 'Trukket tilbake',
      },
    },
  },
} as const
