import { createContext, useContext, type Accessor } from 'solid-js'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'

export type NettBrowserOwner = { userId: string; orgId: string; sessionId: string }
export type NettTabSelection = { sessionId: string; tabId: string; origin: string; tabEpoch: number }

/** The current user's assistant session and a way for visible Nett browsers to identify themselves. */
export type AssistantBrowserContextValue = {
  view: Accessor<AssistantAgentView | null>
  stop: () => void
  claimRetained?: (sessionId: string) => boolean
  /** Reports whether Nett currently shows the agent's stream. */
  setSurfaceVisible?: (visible: boolean) => void
  /** Increments when the dock asks Nett to reveal its agent view. */
  revealVersion?: Accessor<number>
  registerNettBrowser?: (readOwner: Accessor<NettBrowserOwner | null>) => () => void
  prepareContinuation?: (tab: NettTabSelection) => void
  taskBusy?: Accessor<boolean>
}

export const AssistantBrowserContext = createContext<AssistantBrowserContextValue>({ view: () => null, stop: () => undefined })
export const useAssistantBrowser = () => useContext(AssistantBrowserContext)
