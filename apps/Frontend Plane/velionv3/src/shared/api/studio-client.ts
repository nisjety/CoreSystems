import { requestJson } from '@/shared/api/http'
import type { SocialPost, SocialProviderKey } from '@/shared/api/social-client'

export type StudioSection = 'canvas' | 'campaigns' | 'templates'
export type StudioBlockKind = 'profile' | 'image' | 'text' | 'brand' | 'video' | 'link' | 'social'

export type StudioBlock = {
  id: string
  kind: StudioBlockKind
  title: string
  body?: string | null
  imageUrl?: string | null
  x: number
  y: number
  width: number
  height: number
}

export type StudioProject = {
  id: string
  orgId: string
  ownerUserId: string
  updatedByUserId: string
  title: string
  status: 'draft'
  blocks: StudioBlock[]
  selectedBlockId?: string | null
  socialDraftId?: string | null
  socialExportedAt?: string | null
  createdAt: string
  updatedAt: string
}

export type CreateStudioProjectInput = {
  title?: string
  blocks?: StudioBlock[]
  selectedBlockId?: string | null
}

export type SaveStudioProjectInput = {
  title?: string
  blocks?: StudioBlock[]
  selectedBlockId?: string | null
}

export type ExportStudioSocialDraftInput = {
  title?: string
  body?: string
  platforms?: SocialProviderKey[]
  scheduledAt?: string
}

export type StudioSocialDraftExport = {
  project: StudioProject
  socialPost: SocialPost
}

function orgHeaders(orgId: string): HeadersInit {
  return { 'x-velion-org-id': orgId }
}

export function listStudioProjects(orgId: string) {
  return requestJson<{ projects: StudioProject[] }>('/api/v1/studio/projects', {
    headers: orgHeaders(orgId),
  })
}

export function createStudioProject(orgId: string, input: CreateStudioProjectInput) {
  return requestJson<{ project: StudioProject }>('/api/v1/studio/projects', {
    method: 'POST',
    body: JSON.stringify(input),
    headers: orgHeaders(orgId),
  })
}

export function getStudioProject(orgId: string, projectId: string) {
  return requestJson<{ project: StudioProject }>(`/api/v1/studio/projects/${encodeURIComponent(projectId)}`, {
    headers: orgHeaders(orgId),
  })
}

export function saveStudioProject(orgId: string, projectId: string, input: SaveStudioProjectInput) {
  return requestJson<{ project: StudioProject }>(`/api/v1/studio/projects/${encodeURIComponent(projectId)}`, {
    method: 'PUT',
    body: JSON.stringify(input),
    headers: orgHeaders(orgId),
  })
}

export function exportStudioProjectToSocialDraft(
  orgId: string,
  projectId: string,
  input: ExportStudioSocialDraftInput = {},
) {
  return requestJson<StudioSocialDraftExport>(
    `/api/v1/studio/projects/${encodeURIComponent(projectId)}/export/social-draft`,
    {
      method: 'POST',
      body: JSON.stringify(input),
      headers: orgHeaders(orgId),
    },
  )
}
