import { useReducer } from 'react'

type CrawlPhase = 'idle' | 'discovering' | 'mapping' | 'extracting' | 'building' | 'done' | 'error'

interface WebsiteStepState {
  url: string
  crawlPhase: CrawlPhase
  error: string | null
  pageCount: number
  jobId: string | null
}

type WebsiteStepAction =
  | { type: 'SET_URL'; payload: string }
  | { type: 'SET_CRAWL_PHASE'; payload: CrawlPhase }
  | { type: 'SET_ERROR'; payload: string | null }
  | { type: 'SET_JOB_ID'; payload: string | null }
  | { type: 'START_CRAWL' }
  | { type: 'CRAWL_ERROR' }

const initialState: WebsiteStepState = {
  url: '',
  crawlPhase: 'idle',
  error: null,
  pageCount: 0,
  jobId: null,
}

function reducer(state: WebsiteStepState, action: WebsiteStepAction): WebsiteStepState {
  switch (action.type) {
    case 'SET_URL':
      return { ...state, url: action.payload }
    case 'SET_CRAWL_PHASE':
      return { ...state, crawlPhase: action.payload }
    case 'SET_ERROR':
      return { ...state, error: action.payload }
    case 'SET_JOB_ID':
      return { ...state, jobId: action.payload }
    case 'START_CRAWL':
      return { ...state, crawlPhase: 'discovering', error: null, pageCount: 0 }
    case 'CRAWL_ERROR':
      return { ...state, crawlPhase: 'error' }
    default:
      return state
  }
}

export function useWebsiteStepState() {
  return useReducer(reducer, initialState)
}

export type { WebsiteStepState, WebsiteStepAction, CrawlPhase }
