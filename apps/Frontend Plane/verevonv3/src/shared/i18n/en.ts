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
  // Zero data retention (ZDR) acknowledgement for a model connection that cannot give ZDR (ZDR Z-20, Z-21, Z-21b).
  // DRAFT WORDING PENDING COUNSEL (OD-LEGAL, D-ZDR-09): the owner and counsel confirm every sentence here before
  // it ships. The acknowledgement dialog does NOT use this copy for its text: Integration Core composes and stores
  // that text, and the dialog shows it as served. These keys are the chrome around it, the chip, the answer line
  // and the admin list. None of them claims ZDR for a waived chat, and the ones that describe the data say that
  // organisation knowledge and tool results used in the chat are sent to the provider.
  zdr: {
    waiver: {
      chip: 'Not ZDR in this chat',
      chip_title:
        'This chat runs on your own connection, which cannot give zero data retention. What you send, including organisation knowledge and tool results used in the chat, is sent to the provider, and the chat is retained like a normal chat.',
      answer_line:
        "This answer was produced on the member's own connection and is retained like a normal chat.",
      answer_line_title:
        'Not zero data retention. Organisation knowledge and tool results used in this chat were sent to the provider.',
      dialog: {
        loading: 'Loading ...',
        close: 'Close',
        saving: 'Saving ...',
        retry: 'Try again',
        loadFailed: 'The text could not be loaded. Try again.',
        saveFailed: 'Your confirmation could not be saved. Try again.',
        changed: 'The text changed while it was open. Read the new text before you continue.',
        unavailable: 'This connection has no text to confirm right now.',
      },
      review: 'Review zero data retention',
      revoke: 'Withdraw confirmation',
      revoke_confirm:
        'Withdraw your confirmation? While your organisation has zero data retention on, this connection is not used until you confirm again.',
      revoke_yes: 'Withdraw',
      revoke_no: 'Keep',
      revoked: 'Your confirmation was withdrawn.',
      revoke_failed: 'The confirmation could not be withdrawn. Try again.',
      confirmed: 'Confirmed.',
      send_again: 'Confirmed. You can send the message again.',
      chat_review: 'Review and confirm',
      admin: {
        title: 'Acknowledgements',
        intro:
          'Which members have confirmed that a connection of theirs cannot give zero data retention. The list shows who, which provider and which text version, never the text itself.',
        loading: 'Loading ...',
        empty: 'No acknowledgements yet.',
        load_failed: 'The acknowledgements could not be loaded.',
        truncated: 'Only the newest rows are shown.',
        counts: (active: number, superseded: number, revoked: number) =>
          `${active} active, ${superseded} superseded, ${revoked} withdrawn`,
        member: 'Member',
        provider: 'Provider',
        capability: 'Capability',
        text_version: 'Text version',
        locale: 'Language',
        status: 'Status',
        acknowledged_at: 'Confirmed',
        revoked_at: 'Withdrawn',
        status_active: 'Active',
        status_superseded: 'Superseded',
        status_revoked: 'Withdrawn',
      },
    },
  },
} as const
