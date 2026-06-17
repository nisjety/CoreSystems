import { useReducer } from 'react'
import type { ChatActor, ErrorResponse } from '@/components/chat/api/orpc/chat'

interface OptimisticUserMessage {
  clientId: string
  sessionId: string
  content: string
  createdAt: number
}

interface ChatProviderState {
  actor: ChatActor | null
  actorLoading: boolean
  activeSessionId: string | null
  pendingRequests: number
  error: string | null
  /**
   * Optimistic user messages awaiting Convex confirmation. We render
   * these in the message list immediately on submit so the user sees
   * their bubble without waiting for the round-trip. Once a real
   * message with matching content appears in Convex (typically <300ms)
   * the optimistic entry is dropped via OPTIMISTIC_CONFIRM.
   */
  optimisticMessages: OptimisticUserMessage[]
}

type ChatProviderAction =
  | { type: 'SET_ACTOR'; payload: ChatActor | null }
  | { type: 'SET_ACTOR_LOADING'; payload: boolean }
  | { type: 'SET_ACTIVE_SESSION'; payload: string | null }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'INCREMENT_PENDING' }
  | { type: 'DECREMENT_PENDING' }
  | { type: 'ACTOR_LOAD_START' }
  | { type: 'ACTOR_LOAD_SUCCESS'; payload: ChatActor }
  | { type: 'ACTOR_LOAD_FAILURE'; payload: string }
  | { type: 'ACTOR_CLEAR' }
  | { type: 'OPTIMISTIC_ADD'; payload: OptimisticUserMessage }
  | { type: 'OPTIMISTIC_DROP'; payload: { clientId: string } }
  | { type: 'OPTIMISTIC_DROP_SESSION'; payload: { sessionId: string } }
  | { type: 'RESET' }

const initialState: ChatProviderState = {
  actor: null,
  actorLoading: true,
  activeSessionId: null,
  pendingRequests: 0,
  error: null,
  optimisticMessages: [],
}

function reducer(state: ChatProviderState, action: ChatProviderAction): ChatProviderState {
  switch (action.type) {
    case 'SET_ACTOR':
      return { ...state, actor: action.payload }
    case 'SET_ACTOR_LOADING':
      return { ...state, actorLoading: action.payload }
    case 'SET_ACTIVE_SESSION':
      return { ...state, activeSessionId: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'INCREMENT_PENDING':
      return { ...state, pendingRequests: state.pendingRequests + 1 }
    case 'DECREMENT_PENDING':
      return { ...state, pendingRequests: Math.max(0, state.pendingRequests - 1) }
    case 'ACTOR_LOAD_START':
      return { ...state, actorLoading: true, error: null }
    case 'ACTOR_LOAD_SUCCESS':
      return { ...state, actor: action.payload, actorLoading: false }
    case 'ACTOR_LOAD_FAILURE':
      return { ...state, actor: null, actorLoading: false, error: action.payload }
    case 'ACTOR_CLEAR':
      return { ...state, actor: null, actorLoading: false }
    case 'OPTIMISTIC_ADD':
      return {
        ...state,
        optimisticMessages: [...state.optimisticMessages, action.payload],
      }
    case 'OPTIMISTIC_DROP':
      return {
        ...state,
        optimisticMessages: state.optimisticMessages.filter(
          (m) => m.clientId !== action.payload.clientId,
        ),
      }
    case 'OPTIMISTIC_DROP_SESSION':
      return {
        ...state,
        optimisticMessages: state.optimisticMessages.filter(
          (m) => m.sessionId !== action.payload.sessionId,
        ),
      }
    case 'RESET':
      return { ...initialState, actorLoading: false }
    default:
      return state
  }
}

export function useChatProviderState() {
  return useReducer(reducer, initialState)
}

export type { ChatProviderState, ChatProviderAction, OptimisticUserMessage }
