import {
  createEffect,
  createContext,
  onCleanup,
  onMount,
  useContext,
  type Accessor,
  type JSX,
} from 'solid-js'
import { createStore } from 'solid-js/store'
import {
  defaultAgentSelectionId,
  defaultChatbotAddOnId,
  defaultChatbotBuilderSectionId,
  defaultWorkflowBuilderToolId,
  getAgentFeatureFromSearch,
  getAgentSelectionFromSearch,
  getChatbotAddOnFromSearch,
  getChatbotBuilderSectionFromSearch,
  getDefaultAgentFeatureForRole,
  getWorkflowBuilderToolFromSearch,
  isAgentFeatureForRole,
  isAgentSelectionId,
  isChatbotAddOnId,
  isChatbotBuilderSectionId,
  isCoreAgentRoleId,
  isWorkflowBuilderToolId,
  type AgentFeatureId,
  type AgentSelectionId,
  type ChatbotAddOnId,
  type ChatbotBuilderSectionId,
  type WorkflowBuilderToolId,
} from '@/features/agents/lib/agent-roles'

type AgentLocationState = {
  addOn: ChatbotAddOnId
  feature: AgentFeatureId
  selection: AgentSelectionId
  section: ChatbotBuilderSectionId
  workflowTool: WorkflowBuilderToolId
}

type AgentSelectionContextValue = {
  setAddOn: (nextAddOn: ChatbotAddOnId) => void
  setAddOnFromValue: (value: string) => void
  setFeature: (selection: AgentSelectionId, nextFeature: AgentFeatureId) => void
  setFeatureFromValue: (selection: AgentSelectionId, value: string) => void
  setSection: (nextSection: ChatbotBuilderSectionId) => void
  setSectionFromValue: (value: string) => void
  setSelection: (nextSelection: AgentSelectionId) => void
  setSelectionFromValue: (value: string) => void
  setWorkflowTool: (nextTool: WorkflowBuilderToolId) => void
  setWorkflowToolFromValue: (value: string) => void
  state: AgentLocationState
}

const AgentSelectionContext = createContext<AgentSelectionContextValue>()

type AgentRouterLocation = Readonly<{
  pathname: string
  search: string
}>

export function AgentsProvider(props: { children: JSX.Element; routeLocation?: AgentRouterLocation }) {
  const [state, setState] = createStore<AgentLocationState>(readLocationState())

  // CoreShell owns the Solid router. Its location updates for ordinary
  // client-side navigation without emitting a browser `popstate` event, so
  // keep this URL-derived state in sync through that reactive boundary rather
  // than letting a prior /agents selection leak into another workspace.
  createEffect(() => {
    const routeLocation = props.routeLocation
    if (!routeLocation) return
    setState(readLocationStateFromSnapshot(routeLocation.pathname, routeLocation.search))
  })

  const syncFromWindow = () => {
    setState(readLocationState())
  }

  const pushAgentLocation = (updateUrl: (url: URL) => void) => {
    if (typeof window === 'undefined') return

    const url = new URL(window.location.href)
    updateUrl(url)
    window.history.pushState(null, '', `${url.pathname}${url.search}${url.hash}`)
    setState(readLocationState(url))
  }

  const contextValue: AgentSelectionContextValue = {
    state,
    setSelection(nextSelection) {
      pushAgentLocation((url) => {
        if (nextSelection === defaultAgentSelectionId) {
          url.searchParams.delete('agent')
          url.searchParams.delete('stage')
          url.searchParams.delete('feature')
          url.searchParams.delete('view')
          url.searchParams.delete('addon')
          url.searchParams.delete('tool')
          return
        }

        if (nextSelection === 'chatbot') {
          url.searchParams.set('agent', nextSelection)
          url.searchParams.delete('stage')
          url.searchParams.delete('feature')
          url.searchParams.delete('tool')
          if (!isChatbotBuilderSectionId(url.searchParams.get('view'))) {
            url.searchParams.set('view', defaultChatbotBuilderSectionId)
          }
          if (!isChatbotAddOnId(url.searchParams.get('addon'))) {
            url.searchParams.set('addon', defaultChatbotAddOnId)
          }
          return
        }

        if (nextSelection === 'workflow') {
          url.searchParams.set('agent', nextSelection)
          url.searchParams.delete('stage')
          url.searchParams.delete('feature')
          url.searchParams.delete('view')
          url.searchParams.delete('addon')
          if (!isWorkflowBuilderToolId(url.searchParams.get('tool'))) {
            url.searchParams.set('tool', defaultWorkflowBuilderToolId)
          }
          return
        }

        if (isCoreAgentRoleId(nextSelection)) {
          url.searchParams.set('agent', nextSelection)
          url.searchParams.delete('stage')
          url.searchParams.delete('view')
          url.searchParams.delete('addon')
          url.searchParams.delete('tool')
          if (!isAgentFeatureForRole(nextSelection, url.searchParams.get('feature'))) {
            url.searchParams.set('feature', getDefaultAgentFeatureForRole(nextSelection))
          }
        }
      })
    },
    setSelectionFromValue(value) {
      contextValue.setSelection(isAgentSelectionId(value) ? value : defaultAgentSelectionId)
    },
    setFeature(selection, nextFeature) {
      if (!isCoreAgentRoleId(selection)) return

      const safeFeature = isAgentFeatureForRole(selection, nextFeature)
        ? nextFeature
        : getDefaultAgentFeatureForRole(selection)

      pushAgentLocation((url) => {
        url.searchParams.set('agent', selection)
        url.searchParams.set('feature', safeFeature)
        url.searchParams.delete('stage')
        url.searchParams.delete('view')
        url.searchParams.delete('addon')
        url.searchParams.delete('tool')
      })
    },
    setFeatureFromValue(selection, value) {
      if (!isCoreAgentRoleId(selection)) return

      contextValue.setFeature(
        selection,
        isAgentFeatureForRole(selection, value)
          ? value
          : getDefaultAgentFeatureForRole(selection),
      )
    },
    setSection(nextSection) {
      pushAgentLocation((url) => {
        url.searchParams.set('agent', 'chatbot')
        url.searchParams.set('view', nextSection)
        if (!isChatbotAddOnId(url.searchParams.get('addon'))) {
          url.searchParams.set('addon', defaultChatbotAddOnId)
        }
        url.searchParams.delete('stage')
        url.searchParams.delete('tool')
      })
    },
    setSectionFromValue(value) {
      contextValue.setSection(
        isChatbotBuilderSectionId(value)
          ? value
          : defaultChatbotBuilderSectionId,
      )
    },
    setAddOn(nextAddOn) {
      pushAgentLocation((url) => {
        url.searchParams.set('agent', 'chatbot')
        url.searchParams.set('addon', nextAddOn)
        if (!isChatbotBuilderSectionId(url.searchParams.get('view'))) {
          url.searchParams.set('view', defaultChatbotBuilderSectionId)
        }
        url.searchParams.delete('stage')
        url.searchParams.delete('tool')
      })
    },
    setAddOnFromValue(value) {
      contextValue.setAddOn(isChatbotAddOnId(value) ? value : defaultChatbotAddOnId)
    },
    setWorkflowTool(nextTool) {
      pushAgentLocation((url) => {
        url.searchParams.set('agent', 'workflow')
        url.searchParams.set('tool', nextTool)
        url.searchParams.delete('stage')
        url.searchParams.delete('view')
        url.searchParams.delete('addon')
      })
    },
    setWorkflowToolFromValue(value) {
      contextValue.setWorkflowTool(
        isWorkflowBuilderToolId(value) ? value : defaultWorkflowBuilderToolId,
      )
    },
  }

  onMount(() => {
    syncFromWindow()
    window.addEventListener('popstate', syncFromWindow)
    onCleanup(() => {
      window.removeEventListener('popstate', syncFromWindow)
    })
  })

  return (
    <AgentSelectionContext.Provider value={contextValue}>
      {props.children}
    </AgentSelectionContext.Provider>
  )
}

export function useAgentSelection() {
  const context = useAgentSelectionContext()

  return [
    () => context.state.selection,
    context.setSelection,
    context.setSelectionFromValue,
  ] as const
}

export function useAgentFeature(agentSelection: AgentSelectionId | Accessor<AgentSelectionId>) {
  const context = useAgentSelectionContext()

  return [
    () => context.state.feature,
    (nextFeature: AgentFeatureId) => context.setFeature(resolveSelection(agentSelection), nextFeature),
    (value: string) => context.setFeatureFromValue(resolveSelection(agentSelection), value),
  ] as const
}

export function useChatbotBuilderSection() {
  const context = useAgentSelectionContext()

  return [
    () => context.state.section,
    context.setSection,
    context.setSectionFromValue,
  ] as const
}

export function useChatbotAddOn() {
  const context = useAgentSelectionContext()

  return [
    () => context.state.addOn,
    context.setAddOn,
    context.setAddOnFromValue,
  ] as const
}

export function useWorkflowBuilderTool() {
  const context = useAgentSelectionContext()

  return [
    () => context.state.workflowTool,
    context.setWorkflowTool,
    context.setWorkflowToolFromValue,
  ] as const
}

function useAgentSelectionContext() {
  const context = useContext(AgentSelectionContext)
  if (!context) {
    throw new Error('AgentsProvider is required for the agents feature')
  }
  return context
}

function resolveSelection(agentSelection: AgentSelectionId | Accessor<AgentSelectionId>) {
  return typeof agentSelection === 'function'
    ? (agentSelection as Accessor<AgentSelectionId>)()
    : agentSelection
}

function readLocationState(url = getCurrentUrl()): AgentLocationState {
  return readLocationStateFromSnapshot(url?.pathname ?? '', url?.search ?? '')
}

function readLocationStateFromSnapshot(pathname: string, search: string): AgentLocationState {
  const selection = getAgentSelectionFromSnapshot(pathname, search)

  return {
    selection,
    feature: isCoreAgentRoleId(selection)
      ? getAgentFeatureFromSearch(search, selection)
      : getDefaultAgentFeatureForRole('service'),
    section: getChatbotBuilderSectionFromSearch(search),
    addOn: getChatbotAddOnFromSearch(search),
    workflowTool: getWorkflowBuilderToolFromSearch(search),
  }
}

function getAgentSelectionFromSnapshot(pathname: string, search: string): AgentSelectionId {
  const selection = getAgentSelectionFromSearch(search)
  if (selection !== defaultAgentSelectionId) return selection
  if (pathname.startsWith('/agents/chatbots')) return 'chatbot'
  return selection
}

function getCurrentUrl() {
  if (typeof window === 'undefined') return null
  return new URL(window.location.href)
}
