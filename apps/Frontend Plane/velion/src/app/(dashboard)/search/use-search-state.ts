import { useReducer } from 'react'
import type { SearchSource } from '@/lib/api/search-api'

interface SearchState {
  query: string
  submittedQuery: string | null
  answer: string
  sources: SearchSource[]
  isStreaming: boolean
  hasSearched: boolean
}

type SearchAction =
  | { type: 'SET_QUERY'; payload: string }
  | { type: 'START_SEARCH'; payload: string }
  | { type: 'APPEND_ANSWER'; payload: string }
  | { type: 'SET_SOURCES'; payload: SearchSource[] }
  | { type: 'STOP_STREAMING' }

function reducer(state: SearchState, action: SearchAction): SearchState {
  switch (action.type) {
    case 'SET_QUERY':
      return { ...state, query: action.payload }
    case 'START_SEARCH':
      return {
        ...state,
        submittedQuery: action.payload,
        answer: '',
        sources: [],
        isStreaming: true,
        hasSearched: true,
      }
    case 'APPEND_ANSWER':
      return { ...state, answer: state.answer + action.payload }
    case 'SET_SOURCES':
      return { ...state, sources: action.payload }
    case 'STOP_STREAMING':
      return { ...state, isStreaming: false }
    default:
      return state
  }
}

export function createSearchInitialState(initialQuery: string | null): SearchState {
  return {
    query: initialQuery ?? '',
    submittedQuery: null,
    answer: '',
    sources: [],
    isStreaming: false,
    hasSearched: false,
  }
}

export function useSearchState(initialQuery: string | null) {
  return useReducer(reducer, initialQuery, createSearchInitialState)
}

export type { SearchState, SearchAction }
