import { useReducer } from 'react'

interface SecurityState {
  current: string
  next: string
  confirm: string
  success: boolean
  validationError: string | null
}

type SecurityAction =
  | { type: 'SET_CURRENT'; payload: string }
  | { type: 'SET_NEXT'; payload: string }
  | { type: 'SET_CONFIRM'; payload: string }
  | { type: 'SET_VALIDATION_ERROR'; payload: string | null }
  | { type: 'SUBMIT_SUCCESS' }
  | { type: 'CLEAR_SUCCESS' }
  | { type: 'RESET_FORM' }

const initialState: SecurityState = {
  current: '',
  next: '',
  confirm: '',
  success: false,
  validationError: null,
}

function reducer(state: SecurityState, action: SecurityAction): SecurityState {
  switch (action.type) {
    case 'SET_CURRENT':
      return { ...state, current: action.payload }
    case 'SET_NEXT':
      return { ...state, next: action.payload }
    case 'SET_CONFIRM':
      return { ...state, confirm: action.payload }
    case 'SET_VALIDATION_ERROR':
      return { ...state, validationError: action.payload }
    case 'SUBMIT_SUCCESS':
      return { ...state, success: true, current: '', next: '', confirm: '' }
    case 'CLEAR_SUCCESS':
      return { ...state, success: false }
    case 'RESET_FORM':
      return initialState
    default:
      return state
  }
}

export function useSecurityState() {
  return useReducer(reducer, initialState)
}

export type { SecurityState, SecurityAction }
