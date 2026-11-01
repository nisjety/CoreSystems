import { z } from 'zod'

// A chat project is a Space the caller can start a conversation in. This contract
// describes where a chat goes and who it reaches; it grants nothing. Control decides
// on every turn whether the caller may read or write the Space, and the gateway
// refuses a stale or forged project reference the same way.
export const chatProjectRoles = ['editor', 'manager', 'owner'] as const

export const chatProjectSchema = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  name: z.string().trim().min(1).max(120),
  kind: z.enum(['personal', 'room']),
  role: z.enum(chatProjectRoles),
})

export type ChatProject = z.infer<typeof chatProjectSchema>

/**
 * A personal Space, or a named room that is not the organization's own channel and
 * is active. The organization's channel is never a project: its audience is every
 * member of the organization, which a first-send confirm cannot describe honestly.
 * An unknown `is_organization_room` is excluded rather than assumed to be false.
 */
export function isEligibleChatProject(space: {
  readonly kind: string
  readonly lifecycle: string
  readonly is_organization_room?: boolean
}): boolean {
  if (space.lifecycle !== 'active') return false
  if (space.kind === 'personal') return true
  return space.kind === 'room' && space.is_organization_room === false
}

/** A viewer reads a room but may not post to it, so only these roles start a project chat. */
export function canStartChatProject(role: string | undefined): boolean {
  return (chatProjectRoles as readonly string[]).includes(role ?? '')
}
