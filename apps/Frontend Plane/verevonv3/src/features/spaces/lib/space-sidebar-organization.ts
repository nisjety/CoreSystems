import {
  emptySidebarDoc,
  isValidSectionName,
  normalizeSectionName,
  sidebarActionSchema,
  sidebarDocSchema,
  SIDEBAR_LIMITS,
  type SidebarAction,
  type SidebarDoc,
  type SidebarSection,
} from '@/shared/actions/space-sidebar'
import { LocalizedError } from '@/shared/i18n/errors'
import type { SpaceSummary } from '@/shared/api/spaces-client'

// WS16-S9: the pure half of the personal Spaces sidebar. `applySidebarAction` is the one reducer the
// store, the keyboard, the drag and the Move dialog all go through, in the style of
// `applyChatOrganizationAction`. Nothing here reads authority: the room list always comes from
// `listSpaces()` (Control's index), so a document can never add a room, only change how the rooms
// the viewer already has are drawn.
//
// Invariants (docs/plans WS16 D8.4): a room is in at most one custom section; `room.hide` removes it
// from `starred`; `star` removes it from `hidden`; `section.remove` returns its rooms to their
// built-in group (membership is derived, so removing the section is enough); every cap is the same
// number Application enforces.

const limitError = (no: string, en: string) => new LocalizedError({ no, en })

const sectionNotFound = () => limitError(
  'Fant ikke gruppen. Den kan ha blitt fjernet. Last siden på nytt.',
  'Section not found. It may have been removed.',
)
const nameTaken = () => limitError(
  'Du har allerede en gruppe med dette navnet. Velg et annet navn.',
  'A section with this name already exists.',
)
const nameInvalid = () => limitError(
  `Gruppenavnet må ha 1 til ${SIDEBAR_LIMITS.name} tegn.`,
  `A section name must be 1 to ${SIDEBAR_LIMITS.name} characters.`,
)

function without(list: readonly string[], ref: string): string[] {
  return list.filter((item) => item !== ref)
}

/** Insert `ref` before `beforeRef` (end when null/absent/not in the list). `list` must not hold `ref`. */
function insertBefore(list: readonly string[], ref: string, beforeRef: string | null | undefined): string[] {
  const index = beforeRef ? list.indexOf(beforeRef) : -1
  if (index < 0) return [...list, ref]
  return [...list.slice(0, index), ref, ...list.slice(index)]
}

function validName(raw: string): string {
  const name = normalizeSectionName(raw)
  if (!isValidSectionName(name)) throw nameInvalid()
  return name
}

function assertNameFree(sections: readonly SidebarSection[], name: string, exceptId?: string): void {
  const key = name.toLowerCase()
  if (sections.some((section) => section.id !== exceptId && section.name.toLowerCase() === key)) throw nameTaken()
}

function updateSection(doc: SidebarDoc, id: string, patch: (section: SidebarSection) => SidebarSection): SidebarDoc {
  if (!doc.sections.some((section) => section.id === id)) throw sectionNotFound()
  return { ...doc, sections: doc.sections.map((section) => (section.id === id ? patch(section) : section)) }
}

/** Removes `ref` from every custom section. */
function unsectioned(sections: readonly SidebarSection[], ref: string): SidebarSection[] {
  return sections.map((section) => (section.refs.includes(ref) ? { ...section, refs: without(section.refs, ref) } : section))
}

/**
 * Applies one action to a document and returns the next one. Throws `LocalizedError` for a limit or
 * a reference that does not exist; the document passed in is never mutated.
 */
export function applySidebarAction(doc: SidebarDoc, input: SidebarAction): SidebarDoc {
  const action = sidebarActionSchema.parse(input)
  let next: SidebarDoc
  switch (action.type) {
    case 'star': {
      if (doc.starred.includes(action.ref)) return doc
      if (doc.starred.length >= SIDEBAR_LIMITS.starred) {
        throw limitError(`Du kan ha opptil ${SIDEBAR_LIMITS.starred} stjernemerkede rom.`, `You can star up to ${SIDEBAR_LIMITS.starred} rooms.`)
      }
      next = { ...doc, starred: [...doc.starred, action.ref], hidden: without(doc.hidden, action.ref) }
      break
    }
    case 'unstar':
      next = { ...doc, starred: without(doc.starred, action.ref) }
      break
    case 'star.move': {
      if (!doc.starred.includes(action.ref)) return doc
      next = { ...doc, starred: insertBefore(without(doc.starred, action.ref), action.ref, action.beforeRef) }
      break
    }
    case 'section.create': {
      if (doc.sections.length >= SIDEBAR_LIMITS.sections) {
        throw limitError(`Du kan ha opptil ${SIDEBAR_LIMITS.sections} grupper.`, `You can have up to ${SIDEBAR_LIMITS.sections} sections.`)
      }
      if (doc.sections.some((section) => section.id === action.id)) {
        throw limitError('Gruppen finnes allerede.', 'Section already exists.')
      }
      const name = validName(action.name)
      assertNameFree(doc.sections, name)
      next = { ...doc, sections: [...doc.sections, { id: action.id, name, collapsed: false, sort: 'manual', refs: [] }] }
      break
    }
    case 'section.rename': {
      const name = validName(action.name)
      assertNameFree(doc.sections, name, action.id)
      next = updateSection(doc, action.id, (section) => ({ ...section, name }))
      break
    }
    case 'section.remove': {
      if (!doc.sections.some((section) => section.id === action.id)) throw sectionNotFound()
      next = { ...doc, sections: doc.sections.filter((section) => section.id !== action.id) }
      break
    }
    case 'section.collapse':
      next = updateSection(doc, action.id, (section) => ({ ...section, collapsed: action.collapsed }))
      break
    case 'section.sort':
      next = updateSection(doc, action.id, (section) => ({ ...section, sort: action.sort }))
      break
    case 'section.move': {
      const moving = doc.sections.find((section) => section.id === action.id)
      if (!moving) throw sectionNotFound()
      const rest = doc.sections.filter((section) => section.id !== action.id)
      const index = action.beforeId ? rest.findIndex((section) => section.id === action.beforeId) : -1
      next = { ...doc, sections: index < 0 ? [...rest, moving] : [...rest.slice(0, index), moving, ...rest.slice(index)] }
      break
    }
    case 'room.move': {
      const cleared = unsectioned(doc.sections, action.ref)
      // Moving a room is "put it here": it leaves Starred and Hidden so the move is what the person sees.
      const base: SidebarDoc = {
        ...doc,
        sections: cleared,
        starred: without(doc.starred, action.ref),
        hidden: without(doc.hidden, action.ref),
      }
      if (action.sectionId === null) {
        next = base
        break
      }
      const target = cleared.find((section) => section.id === action.sectionId)
      if (!target) throw sectionNotFound()
      if (target.refs.length >= SIDEBAR_LIMITS.refsPerSection) {
        throw limitError(
          `En gruppe kan ha opptil ${SIDEBAR_LIMITS.refsPerSection} rom.`,
          `A section can hold up to ${SIDEBAR_LIMITS.refsPerSection} rooms.`,
        )
      }
      next = updateSection(base, target.id, (section) => ({
        ...section,
        refs: insertBefore(section.refs, action.ref, action.beforeRef),
        // Placing a room by hand means the order is manual now.
        sort: action.beforeRef ? 'manual' : section.sort,
      }))
      break
    }
    case 'room.hide': {
      if (doc.hidden.includes(action.ref)) return doc
      if (doc.hidden.length >= SIDEBAR_LIMITS.hidden) {
        throw limitError(`Du kan skjule opptil ${SIDEBAR_LIMITS.hidden} rom.`, `You can hide up to ${SIDEBAR_LIMITS.hidden} rooms.`)
      }
      next = { ...doc, hidden: [...doc.hidden, action.ref], starred: without(doc.starred, action.ref) }
      break
    }
    case 'room.unhide':
      next = { ...doc, hidden: without(doc.hidden, action.ref) }
      break
    case 'group.update': {
      const current = doc.groups.find((group) => group.id === action.groupId) ?? { id: action.groupId }
      const merged: SidebarDoc['groups'][number] = { ...current }
      if (action.collapsed !== undefined) merged.collapsed = action.collapsed
      if (action.sort === null) delete merged.sort
      else if (action.sort !== undefined) merged.sort = action.sort
      const rest = doc.groups.filter((group) => group.id !== action.groupId)
      const empty = merged.collapsed === undefined && merged.sort === undefined
      next = { ...doc, groups: empty ? rest : [...rest, merged] }
      break
    }
    case 'computed.toggle':
      next = { ...doc, computed: { ...doc.computed, [action.key]: action.on } }
      break
  }
  // The reducer is the last gate before a document is cached or sent: a result that breaks a rule
  // is a bug here, not something for the server to find.
  return sidebarDocSchema.parse(next)
}

/** Drops every reference to a room the viewer cannot see (deleted, removed, never theirs). */
export function pruneDoc(doc: SidebarDoc, visibleRefs: ReadonlySet<string>): SidebarDoc {
  const keep = (ref: string) => visibleRefs.has(ref)
  const starred = doc.starred.filter(keep)
  const hidden = doc.hidden.filter(keep)
  let changed = starred.length !== doc.starred.length || hidden.length !== doc.hidden.length
  const sections = doc.sections.map((section) => {
    const refs = section.refs.filter(keep)
    if (refs.length === section.refs.length) return section
    changed = true
    return { ...section, refs }
  })
  return changed ? { ...doc, starred, hidden, sections } : doc
}

/** Same document, ignoring object identity. Documents are small, so a JSON compare is exact enough. */
export function sidebarDocsEqual(a: SidebarDoc, b: SidebarDoc): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

/** What a built-in group needs to know to place rooms; the registry (core sidebar) supplies these. */
export type SidebarGroupSpec = {
  readonly id: string
  readonly match: (space: SpaceSummary) => boolean
  /** The order when the person has picked none (server order otherwise). */
  readonly defaultCompare?: (a: SpaceSummary, b: SpaceSummary) => number
}

export type SidebarModelSection = { readonly section: SidebarSection; readonly rooms: readonly SpaceSummary[] }
export type SidebarModelGroup = {
  readonly id: string
  readonly rooms: readonly SpaceSummary[]
  readonly collapsed: boolean
  readonly sort: 'name' | 'activity' | null
}

export type SidebarModel = {
  /** Slot 2. Always present (maybe empty) so the header can be a drop target. */
  readonly starred: readonly SpaceSummary[]
  /** Slot 3. Every custom section, empty ones included, in document order. */
  readonly sections: readonly SidebarModelSection[]
  /** Slot 4, registry order, only groups that hold a room. */
  readonly groups: readonly SidebarModelGroup[]
  /** Slot 6. Rooms hidden by the person; shown only behind "Show hidden" or when searching. */
  readonly hidden: readonly SpaceSummary[]
  readonly hiddenCount: number
}

export type SidebarModelInput = {
  readonly spaces: readonly SpaceSummary[]
  readonly doc: SidebarDoc
  readonly groups: readonly SidebarGroupSpec[]
  readonly query: string
  /** Whether a room matches the (already normalised) query. Called only with a non-empty query. */
  readonly matches: (space: SpaceSummary, query: string) => boolean
  /** The name on screen; used for name sort. */
  readonly displayName: (space: SpaceSummary) => string
  readonly locale?: string
}

/**
 * Builds the ordered, deduplicated render model (docs/plans WS16 D8.6 slot table). Slots 1 (Needs
 * you) and 5 (Muted) need WS11's attention store and arrive with WS16-S10; before that nothing here
 * falls back to the browser store. A room appears once: Starred, else its custom section, else its
 * built-in group. Hidden rooms are slot 6 and, while searching, are included when they match.
 */
export function buildSidebarModel(input: SidebarModelInput): SidebarModel {
  const { spaces, doc, groups: specs, query, matches, displayName, locale } = input
  const byRef = new Map<string, SpaceSummary>()
  for (const space of spaces) byRef.set(space.space_ref, space)
  const visible = (space: SpaceSummary) => !query || matches(space, query)
  const collator = (a: SpaceSummary, b: SpaceSummary) => displayName(a).localeCompare(displayName(b), locale)

  const hiddenRefs = new Set(doc.hidden)
  const hiddenAll = doc.hidden.map((ref) => byRef.get(ref)).filter((space): space is SpaceSummary => Boolean(space))
  const placed = new Set<string>()

  const starred: SpaceSummary[] = []
  for (const ref of doc.starred) {
    const space = byRef.get(ref)
    if (!space || hiddenRefs.has(ref) || placed.has(ref)) continue
    placed.add(ref)
    if (visible(space)) starred.push(space)
  }

  const sections: SidebarModelSection[] = doc.sections.map((section) => {
    const rooms: SpaceSummary[] = []
    for (const ref of section.refs) {
      const space = byRef.get(ref)
      if (!space || hiddenRefs.has(ref) || placed.has(ref)) continue
      placed.add(ref)
      if (visible(space)) rooms.push(space)
    }
    if (section.sort === 'name') rooms.sort(collator)
    return { section, rooms }
  })

  const buckets = new Map<string, SpaceSummary[]>(specs.map((spec) => [spec.id, []]))
  const fallback = specs.find((spec) => spec.id === 'channels') ?? specs[0]
  for (const space of spaces) {
    if (placed.has(space.space_ref) || hiddenRefs.has(space.space_ref)) continue
    if (!visible(space)) continue
    // A room no registered group claims lands in Channels, the group the old panel put every
    // non-personal room in, so a kind the registry does not know yet is never dropped.
    const home = specs.find((spec) => spec.match(space)) ?? fallback
    if (home) buckets.get(home.id)?.push(space)
  }

  const groups: SidebarModelGroup[] = []
  for (const spec of specs) {
    const rooms = buckets.get(spec.id) ?? []
    const state = doc.groups.find((group) => group.id === spec.id)
    const sort = state?.sort ?? null
    if (sort === 'name') rooms.sort(collator)
    else if (spec.defaultCompare) rooms.sort(spec.defaultCompare)
    // `activity` needs WS11's last_activity_at; until then it keeps the server order.
    if (rooms.length === 0) continue
    groups.push({ id: spec.id, rooms, collapsed: state?.collapsed === true, sort })
  }

  return {
    starred,
    sections,
    groups,
    hidden: hiddenAll.filter(visible),
    hiddenCount: hiddenAll.length,
  }
}

export { emptySidebarDoc }
