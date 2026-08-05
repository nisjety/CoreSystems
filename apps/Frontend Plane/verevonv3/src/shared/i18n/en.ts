// Central English UI strings for modules that already imported the Norwegian
// copy directly. New dashboard-shell work should prefer useI18n().tr(no, en)
// while older modules migrate incrementally.

export const en = {
  leads: {
    title: 'Leads',
    subtitle:
      'Find Norwegian companies in the Central Coordinating Register by industry, location, and size. Company data only, no contacts or named people.',
    naeringskode: 'Industry code',
    kommunenummer: 'Municipality number',
    organisasjonsform: 'Organization form',
    minEmployees: 'Min. employees',
    maxEmployees: 'Max. employees',
    search: 'Search',
    searching: 'Searching ...',
    employeeBandHint: 'The register cannot filter by 1-4 employees; use 0 or 5+.',
    noResults: 'No companies matched these filters.',
    nameList: 'Name the list',
    saveSelected: (count: number) => `Save ${count} as list`,
    savedLists: 'Saved lists',
    loading: 'Loading ...',
    noSavedLists: 'No saved lists yet.',
    exportCsv: 'Export CSV',
    delete: 'Delete',
    companies: (count: number) => `${count} companies`,
    saved: (name: string, count: number) => `Saved "${name}" (${count} companies).`,
    saveValidation: 'Name the list and select at least one company.',
    saveError: 'Could not save the list. Try again.',
    searchError: 'Search failed. Adjust the filters and try again.',
    columns: {
      navn: 'Name',
      orgnr: 'Org no.',
      form: 'Form',
      naering: 'Industry',
      sted: 'Location',
      ansatte: 'Employees',
    },
  },
} as const
