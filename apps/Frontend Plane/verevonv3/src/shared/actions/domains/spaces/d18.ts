import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// D18: per-member room history visibility, one human-only contract. The REST route is
// `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`
// (apps/gateway/src/domains/spaces/room_admin.rs, behind the gateway's `member_history_windows` switch); the dispatcher
// must ride the same gateway function as the route (`room_admin::set_member_history_window`), so the flag, Control's
// owner-or-manager rule, the audit row and the typed errors exist once. Control (user-core) owns the rule, the audit row
// and the floor every read signs. A window can only HIDE: it narrows what an already-authorised member reads and never
// widens anything.
//
// Model-access.ts closes this id: who may read a room's past is a human decision, and an agent never sets it. The input
// and output carry a mode, an instant and a revision, never message text, so a Zero Data Retention organization behaves
// the same. The output is the window Control confirmed.
//
// The pair rule is Control's (it answers 422 `member_history_window_invalid`, `details.field` is `mode` or `since_at`);
// the schema mirrors it so the browser refuses a bad pair before any call: `since_date` needs an instant that is not in
// the future, and the other two modes carry none.

const memberHistoryMode = z.enum(['full', 'since_join', 'since_date'])

const spacesSetMemberHistoryWindowInput = z
  .object({
    spaceRef: z.string().trim().min(1),
    memberId: z.string().trim().min(1),
    mode: memberHistoryMode,
    // An RFC 3339 instant: the start of the chosen day, in the caller's time zone, as a UTC instant.
    sinceAt: z.string().datetime({ offset: true }).optional(),
    // Optimistic concurrency: the window revision the caller read. A stale one answers 409 `room_member_changed`
    // with `details.current_revision`.
    expectedRevision: z.number().int().min(0).optional(),
  })
  .superRefine((value, context) => {
    if (value.mode === 'since_date') {
      if (value.sinceAt === undefined) {
        context.addIssue({ code: 'custom', path: ['sinceAt'], message: 'sinceAt is required with since_date.' })
      } else if (Date.parse(value.sinceAt) > Date.now()) {
        context.addIssue({ code: 'custom', path: ['sinceAt'], message: 'sinceAt cannot be in the future.' })
      }
    } else if (value.sinceAt !== undefined) {
      context.addIssue({ code: 'custom', path: ['sinceAt'], message: 'sinceAt is only allowed with since_date.' })
    }
  })

const spacesSetMemberHistoryWindowOutput = z.object({
  space_ref: z.string(),
  subject_id: z.string(),
  // False when the member already read under that window: a repeated change writes nothing.
  changed: z.boolean(),
  history_window: z.object({
    // `member`: the member's own setting decides. `room`: the room's history mode does.
    source: z.enum(['member', 'room']),
    mode: memberHistoryMode,
    since_at: z.string().nullable(),
    revision: z.number().int().min(0),
  }),
})

export const d18SpaceActions = [
  {
    id: 'spaces.set_member_history_window',
    label: 'Set Space member history window',
    description:
      'Set how much of a room\'s past one member can read: full history, from a date (not in the future), or since they joined. Only a room owner or manager may. A window only hides: it never widens what a member can read, and Control keeps the audit row. The organization room, personal Spaces and archived rooms are refused.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spacesSetMemberHistoryWindowInput,
    outputSchema: spacesSetMemberHistoryWindowOutput,
  },
] as const satisfies readonly ActionDescriptor[]
