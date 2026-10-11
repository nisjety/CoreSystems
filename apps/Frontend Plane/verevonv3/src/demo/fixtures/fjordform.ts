/**
 * Fictional world for the product scenes (PLAN_2026-10-11.md section 1 and 4).
 *
 * Everything a frame can show comes from here: the organisation Fjordform AS,
 * the user Ida Berg, the customer Nordlys Handel AS and order 4821. No real
 * person, customer, order, e-mail address, company or token appears anywhere
 * in this file. All visible strings are Norwegian bokmål.
 *
 * Fixed fictional date: Friday 9 October 2026, 12:00 Europe/Oslo ("i dag").
 */

export const FJORDFORM_NOW_ISO = '2026-10-09T12:00:00+02:00'

export const fjordformUser = {
  id: 'usr_fjordform_ida',
  email: 'ida.berg@fjordform.example',
  name: 'Ida Berg',
  emailVerified: true,
  image: null,
  role: 'member',
}

export const fjordformOrg = {
  id: 'org_fjordform',
  name: 'Fjordform AS',
  role: 'owner',
}

export const fjordformSession = {
  status: 'authenticated',
  user: fjordformUser,
  org: fjordformOrg,
  permissions: ['chat:use', 'knowledge:read', 'agents:run'],
  onboardingStatus: 'COMPLETED',
}

/** `/api/v1/navbar` – profile, quiet notifications, light theme, Pro plan. */
export const fjordformNavbar = {
  calendar: { events: [], notes: [] },
  notifications: { configured: true, messages: [], notifications: [], unreadCount: 0 },
  plan: 'pro',
  planTrial: false,
  trialEndsAt: null,
  profile: {
    id: fjordformUser.id,
    name: fjordformUser.name,
    email: fjordformUser.email,
    status: 'online',
  },
  theme: { theme: 'light', configured: true, colorScheme: null },
}

/**
 * `/api/v1/models` – the catalog behind the picker. The three Verevon intent
 * modes are pinned client-side; the catalog only has to be a valid list. Two
 * fictional entries keep the picker's provider groups non-empty without naming
 * a real vendor contract.
 */
export const fjordformModels = {
  models: [
    { id: 'nordisk-standard', name: 'Nordisk Standard', provider: 'verevon', modality: 'chat', cost_tier: 'low', cheap: true, features: ['chat', 'tools', 'citations'] },
    { id: 'nordisk-dyp', name: 'Nordisk Dyp', provider: 'verevon', modality: 'chat', cost_tier: 'high', cheap: false, features: ['chat', 'tools', 'citations', 'reasoning'] },
  ],
}

export const fjordformGdprStatus = {
  pending: false,
  org_name: fjordformOrg.name,
}

/** Composer settings panels (skills, providers) – connected but quiet. */
export const fjordformSkills = { skills: [] as unknown[] }
export const fjordformProviders = { providers: [] as unknown[] }
export const fjordformConnections = { connections: [] as unknown[] }
export const fjordformMemories = { memories: [] as unknown[], total: 0 }

// ── Scene 02 · Plan ────────────────────────────────────────────────────────

export const nordlysThreadId = 'thr_nordlys_4821'
export const nordlysRequestId = 'req_nordlys_4821'
export const nordlysRunId = 'run_nordlys_4821'

export const nordlysQuestion = 'Hva er status på Nordlys-leveransen, og hva må vi gjøre nå?'

export const nordlysTitle = 'Nordlys-leveransen · status og neste steg'

/** The three grounding sources, in the order they are shown. */
export const nordlysSources = [
  {
    id: 'src_visma_4821',
    title: 'Ordre 4821 · Visma.net',
    url: 'https://visma.fjordform.example/salgsordre/4821',
    snippet: 'Status: Bekreftet. Opprinnelig levering: 12. okt. Avvik: pakke forsinket hos transportør, ny leveringsdato onsdag 14. okt.',
    source_group_id: 'kb:visma',
    source_group_label: 'Visma.net',
    origin: 'knowledge',
  },
  {
    id: 'src_avtale_nordlys',
    title: 'Kundeavtale Nordlys.pdf · pkt. 4.2',
    url: 'https://dokumenter.fjordform.example/avtaler/nordlys-handel.pdf',
    snippet: 'Ved forsinkelse skal kunden varsles samme dag med ny forventet leveringsdato.',
    source_group_id: 'kb:dokumenter',
    source_group_label: 'Dokumenter',
    origin: 'knowledge',
  },
  {
    id: 'src_epost_nordlys_0910',
    title: 'E-post · Nordlys Handel · 9. okt',
    url: 'https://post.fjordform.example/innboks/nordlys-handel/2026-10-09',
    snippet: 'Kan dere bekrefte om leveransen kommer som avtalt? Vi har montører klare 12. oktober.',
    source_group_id: 'kb:epost',
    source_group_label: 'E-post',
    origin: 'knowledge',
  },
]

/** Assistant line, streamed in chunks. */
export const nordlysAnswerChunks = [
  'Leveransen er forsinket. ',
  'Her er status og neste steg.',
]

/**
 * The draft block added after the status is in place: two lines, so the final
 * frame keeps the whole answer in view. Every claim in it is stated by the
 * Visma order (forsinket hos transportør, ny leveringsdato onsdag 14. okt).
 * Wednesday 14 October 2026 is two days after the agreed Monday 12 October (the day the
 * e-mail says the fitters are ready), so it reads as a delay.
 */
export const nordlysDraftChunks = [
  '\n\n> **Utkast til kunden:** ',
  'Hei! Leveransen er forsinket hos transportøren. ',
  'Ny forventet levering er onsdag 14. oktober.',
]

export const nordlysArtifactId = 'art_status_nordlys'
export const nordlysArtifactTitle = 'Status · Nordlys-leveransen'

/**
 * Status lines with their source references, revealed one by one. Each
 * reference names the source that actually states the line, and all three
 * sources are cited.
 */
export const nordlysStatusLines = [
  { text: 'Forsinket hos transportør.', ref: 'Visma.net' },
  { text: 'Ny levering: onsdag 14. okt.', ref: 'Visma.net' },
  { text: 'Montører står klare 12. okt.', ref: 'E-post 9. okt' },
  { text: 'Varsle kunden i dag.', ref: 'Kundeavtale pkt. 4.2' },
]

export const nordlysPlanRows = [
  'Bekreft ny leveringsdato',
  'Send kundemelding',
  'Oppdater prosjektplan',
]

/**
 * The status document as markdown. A list, not a table: the real table
 * renderer is 620 px wide minimum and would scroll sideways inside the
 * 440 px Resultat canvas, hiding the source column.
 */
export function nordlysArtifactMarkdown(lineCount: number, withPlan: boolean): string {
  // The reference is emphasis so the film can fade it in just after its line
  // (scenes/02-plan.css). The document has no H1: the artifact header above it
  // already carries the title, and a second copy read as a file manager.
  const lines = nordlysStatusLines
    .slice(0, lineCount)
    .map((line) => `- **${line.text}** *· ${line.ref}*`)
  const parts = [...lines]
  if (withPlan) {
    parts.push('', '## Klar for neste steg', '', ...nordlysPlanRows.map((row) => `- [ ] ${row}`))
  }
  return parts.join('\n')
}

/** The approval gate under the answer: nothing is created before a person says yes. */
export const nordlysApproval = {
  id: 'apr_nordlys_oppfolging',
  run_id: nordlysRunId,
  kind: 'Forbered oppfølging',
  status: 'APPROVAL_STATE_REQUESTED',
  detail: 'Tre steg er klare. Ingenting sendes før du godkjenner.',
}

/**
 * Steps shown under Arbeid while the answer is built (the badge counts 1 → 3).
 * The third step's id is the run id: the scene later re-sends it with status
 * `awaiting_approval`, which updates that same step (the badge stays at 3) and
 * makes the controller fetch the run's approvals.
 */
export const nordlysSteps = [
  { id: 'steg-kilder', title: 'Leser kilder', detail: 'Ordre 4821, kundeavtale og e-post fra Nordlys Handel.', status: 'done' },
  { id: 'steg-status', title: 'Setter sammen status', detail: 'Fire funn med kildehenvisning.', status: 'done' },
  { id: nordlysRunId, title: 'Lager planutkast', detail: 'Tre neste steg, klare for godkjenning.', status: 'done' },
]
