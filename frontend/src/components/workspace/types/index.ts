// ─── Workspace section identifiers ────────────────────────────────────────────
export const WORKSPACE_SECTION_IDS = ['general', 'members', 'billing'] as const

export type WorkspaceSectionId = (typeof WORKSPACE_SECTION_IDS)[number]

export function isWorkspaceSectionId(value: string): value is WorkspaceSectionId {
  return WORKSPACE_SECTION_IDS.includes(value as WorkspaceSectionId)
}

// ─── Workspace general form ───────────────────────────────────────────────────
export interface WorkspaceGeneralFormValues {
  name: string
  slug: string
  description: string
  website: string
}
