export const workspaceSettingsSectionIds = [
  'workspace',
  'members',
  'platform-users',
  'billing',
  'sso',
  'org-security',
  'integrations',
  'trust',
  'audit-log',
  'router-policy',
  'finetune',
  'mcp',
  'skills',
  'org-instructions',
  'persona',
  'plugins',
  'cron',
  'quotas',
  'model-access',
  'browser-sites',
  'org-browser-policy',
  'space-policy',
  'memory',
  'learning',
] as const

export type WorkspaceSettingsSectionId = (typeof workspaceSettingsSectionIds)[number]

export type SectionDetail = {
  id: WorkspaceSettingsSectionId
  label: string
  title: string
  description: string
  saveLabel: string
  // Norwegian and English synonyms: search matches them whatever the UI language, so either word finds the page.
  keywords: readonly string[]
}

export const workspaceSettingsSections: SectionDetail[] = [
  {
    id: 'workspace',
    label: 'Workspace',
    title: 'Workspace settings',
    description: 'Manage shared identity, domains, regional defaults, and operational ownership.',
    saveLabel: 'Save workspace',
    keywords: ['arbeidsområde', 'domene', 'domain', 'språk', 'language', 'dataregion', 'data region', 'åpningstider', 'business hours', 'eier', 'owner', 'url'],
  },
  {
    id: 'members',
    label: 'Members & roles',
    title: 'Members & roles',
    description: 'Control who has access, what they can do, and how seats are used.',
    saveLabel: 'Save members',
    keywords: ['medlemmer', 'roller', 'invitere', 'invite', 'e-post', 'seter', 'seats', 'tilgang', 'access'],
  },
  {
    id: 'platform-users',
    label: 'All users (platform)',
    title: 'All users',
    description: 'Every user across all organizations. Platform super-admin only.',
    saveLabel: 'Save',
    keywords: ['brukere', 'alle brukere', 'users', 'superadmin', 'super admin'],
  },
  {
    id: 'billing',
    label: 'Billing',
    title: 'Billing',
    description: 'Review plan, usage, payment method, invoices, and spending controls.',
    saveLabel: 'Save billing',
    keywords: ['fakturering', 'faktura', 'invoice', 'abonnement', 'subscription', 'betaling', 'payment', 'kreditter', 'credits', 'plan'],
  },
  {
    id: 'sso',
    label: 'SSO',
    title: 'SSO',
    description: 'Configure organization sign-in, identity providers, domain enforcement, and provisioning.',
    saveLabel: 'Save SSO',
    keywords: ['innlogging', 'identitetsleverandør', 'identity provider', 'single sign-on', 'sign-in', 'login', 'domene', 'domain', 'provisjonering', 'provisioning'],
  },
  {
    id: 'org-security',
    label: 'Org security',
    title: 'Org security',
    description: 'Set organization-wide security requirements, session policy, and audit controls.',
    saveLabel: 'Save security',
    keywords: ['sikkerhet', 'security', 'organisasjonssikkerhet', 'økt', 'session', 'mfa', 'tofaktor', 'two-factor', 'revisjon', 'audit', 'zdr', 'zero data retention', 'minne', 'memory', 'læring', 'learning', 'support ai'],
  },
  {
    id: 'integrations',
    label: 'Integrations',
    title: 'Integrations',
    description: 'Connect shared support, CRM, communication, and automation systems.',
    saveLabel: 'Save integrations',
    keywords: ['integrasjoner', 'koble til', 'connect', 'kilder', 'sources', 'sharepoint', 'google', 'microsoft', 'meta', 'instagram'],
  },
  {
    id: 'trust',
    label: 'Trust Center',
    title: 'Trust Center',
    description:
      'See exactly what each connected app can access, what data the AI has fetched, and how it is retained.',
    saveLabel: 'Save trust settings',
    keywords: ['tillit', 'tillitssenter', 'personvern', 'privacy', 'retensjon', 'retention', 'oppbevaring', 'tilkoblede apper', 'connected apps', 'ai-aktivitet', 'ai activity', 'revisjon', 'audit'],
  },
  {
    id: 'audit-log',
    label: 'Audit log',
    title: 'Audit log',
    description: 'Browse the organization audit trail: who did what, when, and with what outcome. Owners and admins only; content-free by design.',
    saveLabel: 'Save audit log',
    keywords: ['revisjonslogg', 'logg', 'audit log', 'audit trail', 'hendelser', 'events', 'hvem gjorde hva'],
  },
  {
    id: 'router-policy',
    label: 'Router policy',
    title: 'Router policy',
    description: 'Tune the Verevon intent layer: complexity scoring, budgets, and the model routing table.',
    saveLabel: 'Save router policy',
    keywords: ['ruting', 'routing', 'budsjett', 'budget', 'kompleksitet', 'complexity', 'modellruting', 'model routing'],
  },
  {
    id: 'finetune',
    label: 'Fine-tune jobs',
    title: 'Fine-tune jobs',
    description: 'Manage Azure model fine-tuning jobs: upload training data, launch jobs, and track status.',
    saveLabel: 'Save fine-tune',
    keywords: ['finjustering', 'trening', 'training', 'tilpasset modell', 'custom model'],
  },
  {
    id: 'mcp',
    label: 'MCP-servere',
    title: 'MCP-servere',
    description: 'Registrer eksterne MCP-tjenere agenten kan bruke, og styr hvilke verktøy som er tillatt.',
    saveLabel: 'Lagre MCP-servere',
    keywords: ['verktøy', 'tools', 'tjenere', 'servers', 'external tools'],
  },
  {
    id: 'skills',
    label: 'Ferdigheter',
    title: 'Ferdigheter',
    description: 'Org-dekkende ferdigheter agenten bruker i chat — instrukser som injiseres når en samtale treffer utløser-nøkkelordene.',
    saveLabel: 'Lagre ferdigheter',
    keywords: ['utløser', 'trigger', 'nøkkelord', 'keywords'],
  },
  {
    id: 'org-instructions',
    label: 'Organisasjonsinstrukser',
    title: 'Organisasjonsinstrukser',
    description: 'Instrukser som gjelder for hele organisasjonen og legges til i hver samtale, sammen med rom- og agentinstrukser.',
    saveLabel: 'Lagre instrukser',
    keywords: ['instrukser', 'instructions', 'systemprompt'],
  },
  {
    id: 'persona',
    label: 'Persona',
    title: 'Persona',
    description: 'Dine egne, alltid-på instrukser, lagt til i hver ny samtale sammen med organisasjonens og rommets instrukser.',
    saveLabel: 'Lagre persona',
    keywords: ['tone', 'tonefall', 'stil', 'style', 'instrukser', 'instructions', 'personlig', 'personal'],
  },
  {
    id: 'plugins',
    label: 'Plugin-pakker',
    title: 'Plugin-pakker',
    description: 'Registrer plugin-manifester som kan bidra med verktøy, ferdigheter og hooks. Inaktiv til en administrator aktiverer dem.',
    saveLabel: 'Lagre plugins',
    keywords: ['utvidelser', 'extensions', 'pakker', 'packages', 'hooks', 'manifest'],
  },
  {
    id: 'cron',
    label: 'Planlagte kjøringer',
    title: 'Planlagte kjøringer',
    description: 'Tidsstyrte oppgaver agenten kjører automatisk via cron-uttrykk. Sweeperen fyrer forfalte planer og beregner neste kjøring.',
    saveLabel: 'Lagre planer',
    keywords: ['planlagt', 'planlegging', 'tidsplan', 'tidssone', 'schedule', 'cron', 'time zone', 'timezone'],
  },
  {
    id: 'quotas',
    label: 'Forbrukstak',
    title: 'Forbrukstak',
    description: 'Tak for kostnad og tokens per kjøring. Model Plane leser disse og håndhever dem i budsjettsjekken — de er ikke det samme som plankvotene under Fakturering.',
    saveLabel: 'Lagre tak',
    keywords: ['kvote', 'grenser', 'kostnad', 'budsjett', 'quota', 'limits', 'cost', 'tokens', 'budget', 'usage'],
  },
  {
    id: 'model-access',
    label: 'Modelltilgang',
    title: 'Modelltilgang',
    description: 'Hvilke AI-modeller medlemmene kan bruke, og hvor mye resonnering de kan be om. Håndheves av Model Plane; en modell som ikke er tillatt vises ikke i modellvelgeren.',
    saveLabel: 'Lagre modelltilgang',
    keywords: ['modeller', 'resonnering', 'tillatelsesliste', 'nektelisten', 'models', 'reasoning', 'allowlist', 'blocklist'],
  },
  {
    id: 'browser-sites',
    label: 'Nettsider for agenten',
    title: 'Nettsider for agenten',
    description: 'Nettsidene agenten kan åpne for deg, og hva du har valgt. Administratoren kan begrense dem; valget ditt kan aldri gi mer tilgang.',
    saveLabel: 'Lagre valg',
    keywords: ['nettsider', 'nettleser', 'websites', 'sites', 'browser'],
  },
  {
    id: 'org-browser-policy',
    label: 'Nettleserpolicy',
    title: 'Nettleserpolicy',
    description: 'Hvilke nettsider agenten kan åpne for medlemmene. Bare eiere og administratorer kan endre policyen.',
    saveLabel: 'Lagre nettleserpolicy',
    keywords: ['nettleserpolicy', 'blokkering', 'nettsider', 'blocked', 'websites', 'browser policy'],
  },
  {
    id: 'space-policy',
    label: 'Rom-policy',
    title: 'Rom-policy',
    description: 'Hva organisasjonen tillater i rom: samtale, kunnskap og agenter. Gjelder alle rom; en administrator endrer, Control Plane håndhever og logger.',
    saveLabel: 'Lagre rom-policy',
    keywords: ['rom', 'rooms', 'rompolicy', 'room policy', 'space policy', 'agenter i rom', 'kunnskap', 'knowledge'],
  },
  {
    id: 'memory',
    label: 'Minne',
    title: 'Minne',
    description: 'Se hva Verevon har lagret om deg på tvers av alle samtaler, og slett det du ikke vil at agenten skal huske.',
    saveLabel: 'Lagre',
    keywords: ['husk', 'glemme', 'remember', 'forget'],
  },
  {
    id: 'learning',
    label: 'Læring',
    title: 'Læring',
    description: 'Forslag Verevon har laget fra samtalene dine, og alt Verevon har lært. Et forslag brukes ikke før du godtar det.',
    saveLabel: 'Lagre',
    keywords: ['forslag', 'lært', 'proposals', 'learned'],
  },
]

const sectionDetails = Object.fromEntries(
  workspaceSettingsSections.map((section) => [section.id, section]),
) as Record<WorkspaceSettingsSectionId, SectionDetail>

export function isWorkspaceSettingsSection(value: string): value is WorkspaceSettingsSectionId {
  return workspaceSettingsSectionIds.includes(value as WorkspaceSettingsSectionId)
}

export function getWorkspaceSettingsSection(value: WorkspaceSettingsSectionId): SectionDetail {
  return sectionDetails[value]
}
