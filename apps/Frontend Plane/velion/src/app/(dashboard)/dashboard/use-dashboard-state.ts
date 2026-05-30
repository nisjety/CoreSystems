import { useReducer } from 'react'
import type { ActiveTab } from '@/components/dashboard'

interface User {
  id?: string
  name?: string
  email?: string
}

interface Session {
  user?: User
}

interface DashboardState {
  session: Session | null
  isLoading: boolean
  error: string | null
  activeTab: ActiveTab
  searchQuery: string
}

type DashboardAction =
  | { type: 'SET_SESSION'; payload: Session | null }
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_ACTIVE_TAB'; payload: ActiveTab }
  | { type: 'SET_SEARCH_QUERY'; payload: string }
  | { type: 'SESSION_LOAD_SUCCESS'; payload: Session }
  | { type: 'SESSION_LOAD_ERROR'; payload: string }

const initialState: DashboardState = {
  session: null,
  isLoading: true,
  error: null,
  activeTab: 'Chat',
  searchQuery: '',
}

function reducer(state: DashboardState, action: DashboardAction): DashboardState {
  switch (action.type) {
    case 'SET_SESSION':
      return { ...state, session: action.payload }
    case 'SET_LOADING':
      return { ...state, isLoading: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_ACTIVE_TAB':
      return { ...state, activeTab: action.payload }
    case 'SET_SEARCH_QUERY':
      return { ...state, searchQuery: action.payload }
    case 'SESSION_LOAD_SUCCESS':
      return { ...state, session: action.payload, isLoading: false }
    case 'SESSION_LOAD_ERROR':
      return { ...state, error: action.payload, isLoading: false }
    default:
      return state
  }
}

export function useDashboardState() {
  return useReducer(reducer, initialState)
}

export type { DashboardState, DashboardAction }
