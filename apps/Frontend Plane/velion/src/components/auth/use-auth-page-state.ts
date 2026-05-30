import { useReducer } from 'react'
import { AuthMode } from './types/auth'

interface SessionUser {
  id?: string
}

interface AuthPageState {
  user: SessionUser | null
  authLoading: boolean
  currentMode: AuthMode
  isHydrated: boolean
  isPageVisible: boolean
  viewportHeight: number | null
  contentHeight: number | null
}

type AuthPageAction =
  | { type: 'SET_USER'; payload: SessionUser | null }
  | { type: 'SET_AUTH_LOADING'; payload: boolean }
  | { type: 'SET_CURRENT_MODE'; payload: AuthMode }
  | { type: 'SET_HYDRATED' }
  | { type: 'SET_PAGE_VISIBLE' }
  | { type: 'SET_VIEWPORT_HEIGHT'; payload: number }
  | { type: 'SET_CONTENT_HEIGHT'; payload: number | null }
  | { type: 'SESSION_SUCCESS'; payload: SessionUser }
  | { type: 'SESSION_FAILURE' }

function reducer(state: AuthPageState, action: AuthPageAction): AuthPageState {
  switch (action.type) {
    case 'SET_USER':
      return { ...state, user: action.payload }
    case 'SET_AUTH_LOADING':
      return { ...state, authLoading: action.payload }
    case 'SET_CURRENT_MODE':
      return { ...state, currentMode: action.payload }
    case 'SET_HYDRATED':
      return { ...state, isHydrated: true }
    case 'SET_PAGE_VISIBLE':
      return { ...state, isPageVisible: true }
    case 'SET_VIEWPORT_HEIGHT':
      return { ...state, viewportHeight: action.payload }
    case 'SET_CONTENT_HEIGHT':
      return { ...state, contentHeight: action.payload }
    case 'SESSION_SUCCESS':
      return { ...state, user: action.payload, authLoading: false }
    case 'SESSION_FAILURE':
      return { ...state, user: null, authLoading: false }
    default:
      return state
  }
}

export function createInitialAuthPageState(initialMode: AuthMode): AuthPageState {
  const normalizedMode: Extract<AuthMode, 'signin' | 'signup'> =
    initialMode === 'enterprise-sso' || initialMode === 'org' ? 'signin' : initialMode
  return {
    user: null,
    authLoading: true,
    currentMode: normalizedMode,
    isHydrated: false,
    isPageVisible: false,
    viewportHeight: null,
    contentHeight: null,
  }
}

export function useAuthPageState(initialMode: AuthMode) {
  return useReducer(reducer, initialMode, createInitialAuthPageState)
}

export type { AuthPageState, AuthPageAction }
