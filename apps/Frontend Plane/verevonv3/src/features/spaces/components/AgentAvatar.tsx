import { Show } from 'solid-js'

import { Bot } from '@/shared/icons'
import { avatarTextColor, validAvatarColor } from '../lib/agent-avatar'

export interface AgentAvatarProps {
  readonly name?: string
  /** Raw colour from the roster; re-validated here, a failure renders neutral. */
  readonly color?: string
  readonly size?: number
  readonly kind: 'agent' | 'member'
  readonly class?: string
}

function initialsOf(name: string | undefined): string {
  const parts = (name ?? '').trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '·'
  const first = parts[0]?.[0] ?? ''
  const last = parts.length > 1 ? (parts[parts.length - 1]?.[0] ?? '') : ''
  return `${first}${last}`.toUpperCase()
}

/**
 * One avatar for agents and members. Decorative: the name is always rendered
 * next to it, and an agent is always labelled with the "Agent" chip, so the
 * fill colour is never the only signal.
 */
export function AgentAvatar(props: AgentAvatarProps) {
  const fill = () => (props.kind === 'agent' ? validAvatarColor(props.color) : undefined)
  const size = () => props.size ?? 30
  return (
    <span
      class={['verevon-agent-avatar', props.class, { 'verevon-agent-avatar--neutral': props.kind === 'agent' && !fill() }]}
      data-kind={props.kind}
      aria-hidden="true"
      style={{
        width: `${size()}px`,
        height: `${size()}px`,
        'font-size': `${Math.max(10, Math.round(size() * 0.37))}px`,
        ...(fill() ? { background: fill() as string, color: avatarTextColor(fill() as string) } : {}),
      }}
    >
      <Show when={props.kind === 'agent'} fallback={initialsOf(props.name)}>
        <Bot size={Math.round(size() * 0.48)} />
      </Show>
    </span>
  )
}
