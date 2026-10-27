import type { SpaceAgent } from '@/shared/api/spaces-client'

/**
 * Agent avatar colour helpers (WS07-S7). Colour is presentation only: it never
 * enters a decision, and it is never the only signal (the "Agent" chip always
 * accompanies it). The pattern matches the gateway's `valid_avatar_color` and
 * the Convex write check, so a value that fails here is simply rendered neutral.
 */
const AVATAR_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/

/** The colour when it is exactly `#rrggbb`, else undefined (neutral avatar). */
export function validAvatarColor(value: unknown): string | undefined {
  return typeof value === 'string' && AVATAR_COLOR_PATTERN.test(value) ? value : undefined
}

function channel(value: number): number {
  const c = value / 255
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}

/** WCAG relative luminance of a validated `#rrggbb` colour. */
export function relativeLuminance(hex: string): number {
  const r = channel(Number.parseInt(hex.slice(1, 3), 16))
  const g = channel(Number.parseInt(hex.slice(3, 5), 16))
  const b = channel(Number.parseInt(hex.slice(5, 7), 16))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG contrast ratio between two validated colours. */
export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a)
  const lb = relativeLuminance(b)
  const hi = Math.max(la, lb)
  const lo = Math.min(la, lb)
  return (hi + 0.05) / (lo + 0.05)
}

/** Glyph colour with the higher contrast against the fill: white or near-black. */
export function avatarTextColor(fill: string): '#ffffff' | '#111111' {
  return contrastRatio(fill, '#ffffff') >= contrastRatio(fill, '#111111') ? '#ffffff' : '#111111'
}

/**
 * The roster agent a turn names, by display name. Exactly one match gives that
 * agent; zero or several give undefined, because no per-turn agent id exists yet
 * and guessing between same-named agents would paint the wrong identity.
 */
export function agentForName(
  agents: readonly SpaceAgent[],
  name: string | undefined,
): SpaceAgent | undefined {
  const wanted = name?.trim()
  if (!wanted) return undefined
  const matches = agents.filter((agent) => agent.name?.trim() === wanted)
  return matches.length === 1 ? matches[0] : undefined
}

/** The validated colour to paint for an agent, neutral when its identity is unpublished. */
export function agentAvatarColor(agent: SpaceAgent | undefined): string | undefined {
  if (!agent?.identity_published) return undefined
  return validAvatarColor(agent.avatar_color)
}
