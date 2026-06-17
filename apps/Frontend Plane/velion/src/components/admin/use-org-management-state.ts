import { useReducer } from 'react'
import type { OrgWithDetails } from '@/components/admin/services/admin-service'

interface OrgManagementState {
  orgs: OrgWithDetails[]
  loading: boolean
  error: string | null
  searchQuery: string
  selectedOrg: OrgWithDetails | null
}

type OrgManagementAction =
  | { type: 'SET_ORGS'; payload: OrgWithDetails[] }
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_SEARCH'; payload: string }
  | { type: 'SET_SELECTED_ORG'; payload: OrgWithDetails | null }
  | { type: 'FETCH_START' }
  | { type: 'FETCH_SUCCESS'; payload: OrgWithDetails[] }
  | { type: 'FETCH_ERROR'; payload: string }

const initialState: OrgManagementState = {
  orgs: [],
  loading: true,
  error: null,
  searchQuery: '',
  selectedOrg: null,
}

function reducer(state: OrgManagementState, action: OrgManagementAction): OrgManagementState {
  switch (action.type) {
    case 'SET_ORGS':
      return { ...state, orgs: action.payload }
    case 'SET_LOADING':
      return { ...state, loading: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_SEARCH':
      return { ...state, searchQuery: action.payload }
    case 'SET_SELECTED_ORG':
      return { ...state, selectedOrg: action.payload }
    case 'FETCH_START':
      return { ...state, loading: true }
    case 'FETCH_SUCCESS':
      return { ...state, orgs: action.payload, loading: false }
    case 'FETCH_ERROR':
      return { ...state, error: action.payload, loading: false }
    default:
      return state
  }
}

export function useOrgManagementState() {
  return useReducer(reducer, initialState)
}

export type { OrgManagementState, OrgManagementAction }
