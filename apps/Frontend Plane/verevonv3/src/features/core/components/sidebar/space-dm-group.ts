import { MessageCircle } from '@/shared/icons'
import { isDmKind } from '@/features/spaces/lib/space-kind'
import { registerSpaceSidebarGroup } from './space-sidebar-groups'

// WS12-S3: the "Direct messages" group of the Spaces sidebar, registered through the WS16-S9 group
// registry instead of editing the panel body. It sits above "Personal Space". A DM the caller has
// hidden (server-side, `space.hidden`) matches no group, so it is not drawn until a new message
// brings it back or it is opened again from "New message". The channels group already excludes the
// DM kinds. Importing this module registers the group once.
registerSpaceSidebarGroup(
  {
    id: 'dms',
    label: (tr) => tr('Direktemeldinger', 'Direct messages'),
    icon: MessageCircle,
    defaultCollapsed: false,
    dropTarget: true,
    match: (space) => isDmKind(space.kind) && space.hidden !== true,
  },
  { before: 'personal' },
)
