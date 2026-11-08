import { createContext, useContext } from 'solid-js'
import type { DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'

/** Shared access to the shell's visible navigation guide. */
export type AssistantNavigation = {
  start: (prompt: string, origin?: HTMLElement | null) => boolean
  canStart?: (prompt: string) => boolean
  submit?: (payload: DashboardComposerSubmitPayload) => Promise<void>
  zeroRetention?: () => boolean | undefined
  open?: () => void
  openWithPrompt?: (prompt: string) => void
  isAssistantVisible?: () => boolean
}

export const AssistantNavigationContext = createContext<AssistantNavigation>({ start: () => false })

export function useAssistantNavigation(): AssistantNavigation {
  return useContext(AssistantNavigationContext)
}
