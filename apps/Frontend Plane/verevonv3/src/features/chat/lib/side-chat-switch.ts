/** Only the literal `true` enables the side chat, as for the other chat switches. */
export function isSideChatEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/**
 * Fixed per build, like every `VITE_` switch: read once, at load. The side chat's panel, and the connection a turn keeps
 * across a reload (it names the side question's model), are the only things it turns on.
 */
export const sideChatEnabled = isSideChatEnabled(import.meta.env.VITE_CHAT_SIDE_CHAT)
