export const workspaceSettingsSectionIds = [
  'workspace',
  'members',
  'billing',
  'sso',
  'org-security',
  'integrations',
  'router-policy',
  'finetune',
] as const

export type WorkspaceSettingsSectionId = (typeof workspaceSettingsSectionIds)[number]

export type SectionDetail = {
  id: WorkspaceSettingsSectionId
  label: string
  title: string
  description: string
  saveLabel: string
}

export const workspaceSettingsSections: SectionDetail[] = [
  {
    id: 'workspace',
    label: 'Workspace',
    title: 'Workspace settings',
    description: 'Manage shared identity, domains, regional defaults, and operational ownership.',
    saveLabel: 'Save workspace',
  },
  {
    id: 'members',
    label: 'Members & roles',
    title: 'Members & roles',
    description: 'Control who has access, what they can do, and how seats are used.',
    saveLabel: 'Save members',
  },
  {
    id: 'billing',
    label: 'Billing',
    title: 'Billing',
    description: 'Review plan, usage, payment method, invoices, and spending controls.',
    saveLabel: 'Save billing',
  },
  {
    id: 'sso',
    label: 'SSO',
    title: 'SSO',
    description: 'Configure organization sign-in, identity providers, domain enforcement, and provisioning.',
    saveLabel: 'Save SSO',
  },
  {
    id: 'org-security',
    label: 'Org security',
    title: 'Org security',
    description: 'Set organization-wide security requirements, session policy, and audit controls.',
    saveLabel: 'Save security',
  },
  {
    id: 'integrations',
    label: 'Integrations',
    title: 'Integrations',
    description: 'Connect shared support, CRM, communication, and automation systems.',
    saveLabel: 'Save integrations',
  },
  {
    id: 'router-policy',
    label: 'Router policy',
    title: 'Router policy',
    description: 'Tune the Velion intent layer: complexity scoring, budgets, and the model routing table.',
    saveLabel: 'Save router policy',
  },
  {
    id: 'finetune',
    label: 'Fine-tune jobs',
    title: 'Fine-tune jobs',
    description: 'Manage Azure model fine-tuning jobs: upload training data, launch jobs, and track status.',
    saveLabel: 'Save fine-tune',
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
