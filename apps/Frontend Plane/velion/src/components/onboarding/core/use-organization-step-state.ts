import { useReducer } from 'react'
import type { BrregEnhet } from '@/lib/services/brreg-service'

interface OrgCreateData {
  organizationName: string
  organizationSlug: string
  plan: 'free' | 'pro' | 'enterprise'
}

interface OrgJoinData {
  invitationCode: string
}

interface OrganizationStepState {
  loading: boolean
  error: string | null
  action: 'create' | 'join'
  createData: OrgCreateData
  brregEnhet: BrregEnhet | null
  brregSkipped: boolean
  joinData: OrgJoinData
}

type OrganizationStepAction =
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_ACTION'; payload: 'create' | 'join' }
  | { type: 'SET_CREATE_DATA'; payload: Partial<OrgCreateData> }
  | { type: 'SET_BRREG_ENHET'; payload: BrregEnhet | null }
  | { type: 'SET_BRREG_SKIPPED'; payload: boolean }
  | { type: 'SET_JOIN_DATA'; payload: Partial<OrgJoinData> }
  | { type: 'UPDATE_NAME'; payload: { name: string; slug: string } }
  | { type: 'SELECT_BRREG'; payload: { enhet: BrregEnhet; name: string; slug: string } }
  | { type: 'SUBMIT_START' }
  | { type: 'SUBMIT_ERROR'; payload: string }
  | { type: 'SUBMIT_END' }

const initialState: OrganizationStepState = {
  loading: false,
  error: null,
  action: 'create',
  createData: { organizationName: '', organizationSlug: '', plan: 'free' },
  brregEnhet: null,
  brregSkipped: false,
  joinData: { invitationCode: '' },
}

function reducer(state: OrganizationStepState, action: OrganizationStepAction): OrganizationStepState {
  switch (action.type) {
    case 'SET_LOADING':
      return { ...state, loading: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_ACTION':
      return { ...state, action: action.payload }
    case 'SET_CREATE_DATA':
      return { ...state, createData: { ...state.createData, ...action.payload } }
    case 'SET_BRREG_ENHET':
      return { ...state, brregEnhet: action.payload }
    case 'SET_BRREG_SKIPPED':
      return { ...state, brregSkipped: action.payload }
    case 'SET_JOIN_DATA':
      return { ...state, joinData: { ...state.joinData, ...action.payload } }
    case 'UPDATE_NAME':
      return {
        ...state,
        createData: { ...state.createData, organizationName: action.payload.name, organizationSlug: action.payload.slug },
        brregEnhet: null,
        brregSkipped: false,
      }
    case 'SELECT_BRREG':
      return {
        ...state,
        brregEnhet: action.payload.enhet,
        createData: { ...state.createData, organizationName: action.payload.name, organizationSlug: action.payload.slug },
      }
    case 'SUBMIT_START':
      return { ...state, error: null, loading: true }
    case 'SUBMIT_ERROR':
      return { ...state, error: action.payload, loading: false }
    case 'SUBMIT_END':
      return { ...state, loading: false }
    default:
      return state
  }
}

export function useOrganizationStepState() {
  return useReducer(reducer, initialState)
}

export type { OrganizationStepState, OrganizationStepAction }
