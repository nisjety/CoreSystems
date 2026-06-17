import { useReducer } from 'react'

interface TeamMember {
  email: string
  role: 'admin' | 'member' | 'viewer'
}

interface TeamStepState {
  loading: boolean
  error: string | null
  email: string
  role: 'admin' | 'member' | 'viewer'
  members: TeamMember[]
}

type TeamStepAction =
  | { type: 'SET_LOADING'; payload: boolean }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_EMAIL'; payload: string }
  | { type: 'SET_ROLE'; payload: 'admin' | 'member' | 'viewer' }
  | { type: 'ADD_MEMBER'; payload: TeamMember }
  | { type: 'REMOVE_MEMBER'; payload: string }
  | { type: 'CLEAR_INPUT' }
  | { type: 'SUBMIT_START' }
  | { type: 'SUBMIT_ERROR'; payload: string }
  | { type: 'SUBMIT_END' }

const initialState: TeamStepState = {
  loading: false,
  error: null,
  email: '',
  role: 'member',
  members: [],
}

function reducer(state: TeamStepState, action: TeamStepAction): TeamStepState {
  switch (action.type) {
    case 'SET_LOADING':
      return { ...state, loading: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_EMAIL':
      return { ...state, email: action.payload }
    case 'SET_ROLE':
      return { ...state, role: action.payload }
    case 'ADD_MEMBER':
      return { ...state, members: [...state.members, action.payload], email: '', role: 'member', error: null }
    case 'REMOVE_MEMBER':
      return { ...state, members: state.members.filter(m => m.email !== action.payload) }
    case 'CLEAR_INPUT':
      return { ...state, email: '', role: 'member' }
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

export function useTeamStepState() {
  return useReducer(reducer, initialState)
}

export type { TeamStepState, TeamStepAction, TeamMember }
