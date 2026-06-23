import { getSessionContext } from '@/shared/api/auth-client'
import {
  listStudioProjects,
  type StudioBlock,
  type StudioBlockKind,
  type StudioProject,
} from '@/shared/api/studio-client'

export type {
  StudioBlock,
  StudioBlockKind,
  StudioProject,
}

export type StudioLayoutField = 'x' | 'y' | 'width' | 'height'
export type StudioPreviewDevice = 'desktop' | 'mobile'
export type StudioPersistenceSource = 'fallback' | 'live'
export type StudioPersistenceAction = 'export' | 'save'

export const CANVAS_WIDTH = 1600
export const CANVAS_HEIGHT = 760
export const CANVAS_PADDING = 24
export const GRID_SIZE = 8
export const MIN_BLOCK_WIDTH = 180
export const MIN_BLOCK_HEIGHT = 140
export const DUPLICATE_OFFSET = 32

/**
 * Phase 4 PR-3 seed strip: the canvas now starts empty. The previous demo blocks
 * (a fabricated persona plus stock imagery) were a verbatim mirror of the gateway
 * seed and implied real org content that does not exist. Both have been stripped
 * so the canvas opens to the honest "Start a Studio board" empty state until the
 * user (or a loaded project) adds blocks.
 */
export const initialBlocks: StudioBlock[] = []

export async function loadStudioWorkspace(): Promise<{
  orgId: string
  project: StudioProject | null
  source: StudioPersistenceSource
}> {
  const ctx = await getSessionContext().catch(() => null)
  const activeOrgId = ctx?.orgs[0]?.id ?? ctx?.orgId ?? ''

  if (!activeOrgId) {
    return { orgId: '', project: null, source: 'fallback' }
  }

  try {
    const result = await listStudioProjects(activeOrgId)
    return { orgId: activeOrgId, project: result.projects[0] ?? null, source: 'live' }
  } catch {
    return { orgId: activeOrgId, project: null, source: 'fallback' }
  }
}

export function createBlock(kind: StudioBlockKind, count: number): StudioBlock {
  const base = {
    id: `${kind}_${Date.now()}_${count}`,
    kind,
    title: `${titleForKind(kind)} ${count}`,
    body: bodyForKind(kind),
    x: 180 + count * 28,
    y: 130 + count * 28,
    width: kind === 'text' ? 300 : 320,
    height: kind === 'text' ? 190 : 240,
  }

  if (kind === 'image' || kind === 'profile') {
    // Phase 4 PR-3 seed strip: a freshly added image block starts with no image
    // (an empty placeholder the user fills) rather than fabricated stock imagery.
    return constrainBlock({
      ...base,
      height: 260,
    })
  }

  return constrainBlock(base)
}

export function normalizeStudioBlock(block: StudioBlock): StudioBlock {
  return constrainBlock({
    ...block,
    body: block.body ?? undefined,
    imageUrl: block.imageUrl ?? undefined,
  })
}

export function serializeBlocks(blocks: StudioBlock[]): StudioBlock[] {
  return blocks.map((block) => ({
    ...block,
    body: block.body ?? null,
    imageUrl: block.imageUrl ?? null,
  }))
}

export function buildSocialDraftBody(blocks: StudioBlock[]) {
  const body = blocks
    .map((block) => [block.title, block.body].filter(Boolean).join(': '))
    .filter(Boolean)
    .join('\n\n')

  return body || 'Studio canvas draft.'
}

export function constrainBlock(block: StudioBlock): StudioBlock {
  const width = clamp(Math.round(block.width), MIN_BLOCK_WIDTH, CANVAS_WIDTH - CANVAS_PADDING * 2)
  const height = clamp(Math.round(block.height), MIN_BLOCK_HEIGHT, CANVAS_HEIGHT - CANVAS_PADDING * 2)

  return {
    ...block,
    width,
    height,
    x: clampToCanvas(Math.round(block.x), width, CANVAS_WIDTH),
    y: clampToCanvas(Math.round(block.y), height, CANVAS_HEIGHT),
  }
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max)
}

export function clampToCanvas(value: number, size: number, canvasSize: number) {
  return clamp(value, CANVAS_PADDING, Math.max(CANVAS_PADDING, canvasSize - size - CANVAS_PADDING))
}

export function snapToGrid(value: number) {
  return Math.round(value / GRID_SIZE) * GRID_SIZE
}

export function cloneBlocks(blocks: StudioBlock[]) {
  return blocks.map((block) => ({ ...block }))
}

export function isEditableTarget(target: EventTarget | null) {
  return target instanceof HTMLElement && Boolean(target.closest('input, textarea, [contenteditable="true"]'))
}

function titleForKind(kind: StudioBlockKind) {
  switch (kind) {
    case 'image':
      return 'Image asset'
    case 'video':
      return 'Video scene'
    case 'link':
      return 'Reference link'
    case 'social':
      return 'Social draft'
    case 'brand':
      return 'Brand block'
    case 'profile':
      return 'Profile'
    case 'text':
    default:
      return 'Text note'
  }
}

function bodyForKind(kind: StudioBlockKind) {
  switch (kind) {
    case 'social':
      return 'Draft a post angle, hook, and CTA before sending it to Social.'
    case 'video':
      return 'Scene idea, shot note, or generated media placeholder.'
    case 'link':
      return 'Attach a trend, competitor, source, or product page.'
    case 'image':
      return 'Attach or generate a campaign visual.'
    default:
      return 'Add campaign context, constraints, or a creative direction.'
  }
}
