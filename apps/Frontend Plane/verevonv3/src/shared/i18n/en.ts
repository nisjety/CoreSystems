// Central English UI strings for modules that already imported the Norwegian
// copy directly. New dashboard-shell work should prefer useI18n().tr(no, en)
// while older modules migrate incrementally.

// Limits the refusal sentences quote; the numbers live in
// features/chat/lib/attachment-policy.ts.
type AttachmentLimits = { maxFileBytes: number; maxTotalBytes: number; maxImages: number }
const megabytes = (bytes: number) => `${Math.round((bytes / 1_048_576) * 10) / 10} MB`

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
  attachments: {
    refusal: {
      unsupported_type: (name: string) =>
        `"${name}" was not attached: this file type is not supported. Use an image (PNG, JPEG, WebP, GIF) or a PDF, DOCX, XLSX, PPTX, TXT, MD, CSV, JSON or HTML file.`,
      too_large: (name: string, limits: AttachmentLimits) =>
        `"${name}" was not attached: it is too large. The limit is ${megabytes(limits.maxFileBytes)} per file and ${megabytes(limits.maxTotalBytes)} in total.`,
      route_unsupported: (name: string) =>
        `"${name}" was not attached: the ChatGPT subscription does not accept images. Pick another model to use images.`,
      do_images_unsupported: (name: string) =>
        `"${name}" was not attached: Do cannot use images yet. Switch to Ask to analyze the image.`,
      too_many_images: (name: string, limits: AttachmentLimits) =>
        `"${name}" was not attached: only ${limits.maxImages} image can be analyzed per message.`,
      zdr_refused: (name: string) =>
        `"${name}" was not attached: a temporary chat does not accept images.`,
    },
    notSent:
      'The image was not sent: the ChatGPT subscription does not accept images. Remove the image or pick another model. Nothing was sent.',
  },
  effectivePolicy: {
    title: 'Effective policy',
    intro: 'The values that apply here and the layer that set each one. A lock means a higher layer fixes the value.',
    empty: 'No settings apply here.',
    loadFailed: 'The effective policy could not be loaded.',
    setBy: (origin: string) => `Set by: ${origin}`,
    sourceUnknown: 'Source not reported',
    origin: {
      platform: 'Platform default',
      org: 'Organisation',
      space: 'Space',
      user: 'You',
      request: 'This request',
    },
    locked: 'Fixed by a higher layer',
    unavailable: 'Could not be read right now',
    notSet: 'Not set',
    redacted: 'Visible to admins only',
    redactedCount: (count: number) => `${count} entries, visible to admins only`,
    on: 'On',
    off: 'Off',
    revision: (revision: number) => `Revision ${revision}`,
    keys: {
      approval_posture: 'Approval posture',
      zdr: 'Zero data retention (ZDR)',
      privacy_tier_floor: 'Privacy tier floor',
      model_access: 'Model access',
      routing: 'Routing',
      budget: 'Budget',
      web_access: 'Web access',
      browser_site_mode: 'Browser site mode',
    },
  },
} as const
