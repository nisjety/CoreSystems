import { Hash, Users } from '@/shared/icons'
import type { SidebarGroupId } from '@/shared/actions/space-sidebar'
import type { SidebarGroupSpec } from '@/features/spaces/lib/space-sidebar-organization'

// WS16-S9: the group registry of the Spaces sidebar (docs/plans WS16 D8.6). The panel renders the
// built-in groups from this ordered list and never names one. WS12 registers `dms` and WS04
// registers `archived` with `registerSpaceSidebarGroup` instead of editing the panel body. The ids
// are a closed set (`SIDEBAR_GROUP_IDS`) because they are stored in the sidebar document and the
// server validates them.

export type SpaceSidebarGroupDef = SidebarGroupSpec & {
  readonly id: SidebarGroupId
  /** Heading, Norwegian first (the project's `tr` convention). */
  readonly label: (tr: (no: string, en: string) => string) => string
  readonly icon: typeof Hash
  readonly defaultCollapsed: boolean
  /** Whether a room can be dropped on the header to return it to this group. */
  readonly dropTarget: boolean
}

const DM_KINDS = new Set(['dm', 'group_dm'])

const BUILT_IN: SpaceSidebarGroupDef[] = [
  {
    id: 'channels',
    label: (tr) => tr('Kanaler', 'Channels'),
    icon: Hash,
    defaultCollapsed: false,
    dropTarget: true,
    match: (space) => space.kind !== 'personal' && !DM_KINDS.has(space.kind),
    // Slack-shaped, as the panel always was: archived rooms sink to the bottom, the organization's
    // own room pins to the top, the rest keep the server's order.
    defaultCompare: (a, b) =>
      Number(a.archived === true) - Number(b.archived === true)
      || Number(b.is_organization_room === true) - Number(a.is_organization_room === true),
  },
  {
    id: 'personal',
    label: (tr) => tr('Personlig rom', 'Personal Space'),
    icon: Users,
    defaultCollapsed: false,
    dropTarget: true,
    match: (space) => space.kind === 'personal',
  },
]

let registry: readonly SpaceSidebarGroupDef[] = BUILT_IN

/** The groups in render order. */
export function spaceSidebarGroups(): readonly SpaceSidebarGroupDef[] {
  return registry
}

/**
 * Adds a group (WS12 `dms`, WS04 `archived`). `before` places it ahead of an existing group id;
 * otherwise it goes last. Registering the same id again replaces the entry.
 */
export function registerSpaceSidebarGroup(def: SpaceSidebarGroupDef, options: { before?: SidebarGroupId } = {}): void {
  const rest = registry.filter((existing) => existing.id !== def.id)
  const index = options.before ? rest.findIndex((existing) => existing.id === options.before) : -1
  registry = index < 0 ? [...rest, def] : [...rest.slice(0, index), def, ...rest.slice(index)]
}

/** Test seam: restores the built-in registry. */
export function resetSpaceSidebarGroupsForTests(): void {
  registry = BUILT_IN
}
