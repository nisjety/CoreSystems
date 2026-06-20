// Central Norwegian (Bokmål) UI strings.
//
// Velion's UI is Norwegian-facing, but user-visible copy has historically been
// scattered inline across components (e.g. the sidebar labels in
// features/core/lib/sidebar-navigation.ts). This module is the single source of
// truth for Norwegian copy so labels stay consistent and translatable.
//
// Convention: NEW features import their copy from here; existing scattered
// strings are migrated into this module incrementally. This is the seam — not a
// claim that every string in the app already routes through it.

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
} as const
