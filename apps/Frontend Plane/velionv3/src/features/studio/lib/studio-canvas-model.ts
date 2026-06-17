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

export const initialBlocks: StudioBlock[] = [
  {
    id: 'profile',
    kind: 'profile',
    title: 'Ava Berg',
    body: 'Creative lead, Velion',
    imageUrl: 'https://images.unsplash.com/photo-1494790108377-be9c29b29330?auto=format&fit=crop&w=420&q=80',
    x: 92,
    y: 82,
    width: 390,
    height: 300,
  },
  {
    id: 'positioning',
    kind: 'text',
    title: 'Campaign hook',
    body: 'Turn support signals into public trust. Show the workflow, not the promise.',
    x: 520,
    y: 92,
    width: 310,
    height: 210,
  },
  {
    id: 'brand',
    kind: 'brand',
    title: 'VELION',
    body: 'Quiet operations, visible momentum',
    x: 868,
    y: 90,
    width: 330,
    height: 220,
  },
  {
    id: 'workspace',
    kind: 'image',
    title: 'Product workspace',
    body: 'Dashboard crop for launch story',
    imageUrl: 'https://images.unsplash.com/photo-1497366811353-6870744d04b2?auto=format&fit=crop&w=520&q=80',
    x: 1252,
    y: 90,
    width: 320,
    height: 590,
  },
  {
    id: 'motion',
    kind: 'image',
    title: 'Motion background',
    body: 'Use as short-form opening scene',
    imageUrl: 'https://images.unsplash.com/photo-1500530855697-b586d89ba3ee?auto=format&fit=crop&w=780&q=80',
    x: 92,
    y: 420,
    width: 690,
    height: 270,
  },
  {
    id: 'x-card',
    kind: 'link',
    title: '@velion on X',
    body: 'x.com/velion',
    x: 870,
    y: 350,
    width: 320,
    height: 265,
  },
]

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
    return constrainBlock({
      ...base,
      imageUrl: 'https://images.unsplash.com/photo-1518005020951-eccb494ad742?auto=format&fit=crop&w=520&q=80',
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
