/**
 * Presentation rules for human posts in a room (WS01-S6). Nothing here authorizes anything:
 * Control decides who may post (a signed `thread:append` decision per send) and Session Core
 * enforces it. These only decide what the composer offers, so a member is not shown a control
 * that would be refused every time.
 */

/**
 * Whether the composer offers posting to this role. Mirrors the floor Control applies to a
 * shared-thread append (`ValidateForSharedThread`: editor, manager, owner), as
 * `canEditSpaceInstructions` does on the page. A viewer, or a role the SPA does not know, gets a
 * disabled composer with the reason stated.
 */
export function canPostInSpace(role: string | undefined): boolean {
  return role === 'editor' || role === 'manager' || role === 'owner'
}

/** Characters in a post, counted the way the gateway counts them (code points, not UTF-16 units). */
export function postLength(content: string): number {
  return Array.from(content).length
}
