import { z } from 'zod'

import limits from './space-sidebar-limits.json'

// WS16-S9: the personal Spaces sidebar document and the actions that edit it.
//
// A layout, not content: room references, a label the person typed for a section, and display
// toggles. Nothing in authorization reads it; hide, star and sort change what is drawn, never what
// is allowed (docs/plans WS16 D8.8). Application (convex-core `spaceSidebarRules.ts`) enforces the
// same caps; the numbers are pinned by `space-sidebar-limits.json` (+ `.sha256`), a golden that is
// byte-identical to `apps/Application Plane/convex-core/test/fixtures/space-sidebar-limits.json`.

export const SIDEBAR_LIMITS = {
  maxBytes: limits.maxBytes,
  starred: limits.starred,
  hidden: limits.hidden,
  sections: limits.sections,
  refsPerSection: limits.refsPerSection,
  name: limits.name,
} as const

/** Closed set of built-in group ids; the group registry (space-sidebar-groups.ts) owns the labels. */
export const SIDEBAR_GROUP_IDS = ['channels', 'dms', 'personal', 'archived'] as const
export type SidebarGroupId = (typeof SIDEBAR_GROUP_IDS)[number]
export const SIDEBAR_SECTION_SORTS = ['manual', 'name', 'activity'] as const
export const SIDEBAR_GROUP_SORTS = ['name', 'activity'] as const
export const SIDEBAR_COMPUTED_KEYS = ['needsYou', 'unread', 'working', 'muted'] as const

export const SIDEBAR_REF_PATTERN = /^[A-Za-z0-9_.:-]{1,128}$/
export const SIDEBAR_SECTION_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/

const refSchema = z.string().regex(SIDEBAR_REF_PATTERN)
const sectionIdSchema = z.string().regex(SIDEBAR_SECTION_ID_PATTERN)
const sectionSortSchema = z.enum(SIDEBAR_SECTION_SORTS)
const groupSortSchema = z.enum(SIDEBAR_GROUP_SORTS)
const groupIdSchema = z.enum(SIDEBAR_GROUP_IDS)

/** A section label: NFC, trimmed, 1..name code points. Uniqueness is the reducer's job. */
export function normalizeSectionName(raw: string): string {
  return raw.normalize('NFC').trim()
}
export function isValidSectionName(name: string): boolean {
  const length = Array.from(name).length
  return name === normalizeSectionName(name) && length >= 1 && length <= SIDEBAR_LIMITS.name
}

export const sidebarSectionSchema = z.strictObject({
  id: sectionIdSchema,
  name: z.string().refine(isValidSectionName),
  collapsed: z.boolean(),
  sort: sectionSortSchema,
  refs: z.array(refSchema).max(SIDEBAR_LIMITS.refsPerSection),
})

export const sidebarDocSchema = z.strictObject({
  v: z.literal(1),
  starred: z.array(refSchema).max(SIDEBAR_LIMITS.starred),
  hidden: z.array(refSchema).max(SIDEBAR_LIMITS.hidden),
  sections: z.array(sidebarSectionSchema).max(SIDEBAR_LIMITS.sections),
  groups: z.array(z.strictObject({
    id: groupIdSchema,
    collapsed: z.boolean().optional(),
    sort: groupSortSchema.optional(),
  })),
  computed: z.strictObject({
    needsYou: z.boolean(),
    unread: z.boolean(),
    working: z.boolean(),
    muted: z.boolean(),
  }),
})

export type SidebarDoc = z.infer<typeof sidebarDocSchema>
export type SidebarSection = z.infer<typeof sidebarSectionSchema>

export const sidebarActionSchema = z.discriminatedUnion('type', [
  z.strictObject({ type: z.literal('star'), ref: refSchema }),
  z.strictObject({ type: z.literal('unstar'), ref: refSchema }),
  z.strictObject({ type: z.literal('star.move'), ref: refSchema, beforeRef: refSchema.nullable() }),
  z.strictObject({ type: z.literal('section.create'), id: sectionIdSchema, name: z.string() }),
  z.strictObject({ type: z.literal('section.rename'), id: sectionIdSchema, name: z.string() }),
  z.strictObject({ type: z.literal('section.remove'), id: sectionIdSchema }),
  z.strictObject({ type: z.literal('section.collapse'), id: sectionIdSchema, collapsed: z.boolean() }),
  z.strictObject({ type: z.literal('section.sort'), id: sectionIdSchema, sort: sectionSortSchema }),
  z.strictObject({ type: z.literal('section.move'), id: sectionIdSchema, beforeId: sectionIdSchema.nullable() }),
  z.strictObject({
    type: z.literal('room.move'),
    ref: refSchema,
    sectionId: sectionIdSchema.nullable(),
    beforeRef: refSchema.nullable().optional(),
  }),
  z.strictObject({ type: z.literal('room.hide'), ref: refSchema }),
  z.strictObject({ type: z.literal('room.unhide'), ref: refSchema }),
  z.strictObject({
    type: z.literal('group.update'),
    groupId: groupIdSchema,
    collapsed: z.boolean().optional(),
    sort: groupSortSchema.nullable().optional(),
  }),
  z.strictObject({ type: z.literal('computed.toggle'), key: z.enum(SIDEBAR_COMPUTED_KEYS), on: z.boolean() }),
])

export type SidebarAction = z.infer<typeof sidebarActionSchema>

export function emptySidebarDoc(): SidebarDoc {
  return {
    v: 1,
    starred: [],
    hidden: [],
    sections: [],
    groups: [],
    // Needs you and Muted are WS16-S10 slots; the toggles ride in the document from day one.
    computed: { needsYou: true, unread: false, working: false, muted: false },
  }
}

/** The document, or null when the value is not a valid one. */
export function parseSidebarDoc(value: unknown): SidebarDoc | null {
  const result = sidebarDocSchema.safeParse(value)
  return result.success ? result.data : null
}
