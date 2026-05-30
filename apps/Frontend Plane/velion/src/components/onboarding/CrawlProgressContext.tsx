'use client'

import React, { createContext, useContext, useCallback, useRef, useReducer, useEffect } from 'react'
import type { CrawlPhase } from './core/use-website-step-state'

interface CrawlProgressState {
  crawlPhase: CrawlPhase
  pageCount: number
  jobId: string | null
  url: string | null
  isActive: boolean
}

type CrawlProgressAction =
  | { type: 'START'; payload: { jobId: string; url: string } }
  | { type: 'SET_PHASE'; payload: CrawlPhase }
  | { type: 'SET_PAGE_COUNT'; payload: number }
  | { type: 'INCREMENT_PAGES'; payload: number }
  | { type: 'DONE'; payload: number }
  | { type: 'ERROR' }
  | { type: 'RESET' }

const initialState: CrawlProgressState = {
  crawlPhase: 'idle',
  pageCount: 0,
  jobId: null,
  url: null,
  isActive: false,
}

function reducer(state: CrawlProgressState, action: CrawlProgressAction): CrawlProgressState {
  switch (action.type) {
    case 'START':
      return {
        ...state,
        crawlPhase: 'discovering',
        pageCount: 0,
        jobId: action.payload.jobId,
        url: action.payload.url,
        isActive: true,
      }
    case 'SET_PHASE':
      return { ...state, crawlPhase: action.payload }
    case 'SET_PAGE_COUNT':
      return { ...state, pageCount: action.payload }
    case 'INCREMENT_PAGES':
      return { ...state, pageCount: state.pageCount + action.payload }
    case 'DONE':
      return {
        ...state,
        crawlPhase: 'done',
        pageCount: Math.max(state.pageCount, action.payload),
        // Keep isActive true so WebsiteStep reads ctx.crawlPhase ('done')
        // instead of falling back to local crawlPhase ('discovering').
        // CrawlProgressModal resets isActive via reset() after auto-dismiss.
        isActive: true,
      }
    case 'ERROR':
      return { ...state, crawlPhase: 'error', isActive: false }
    case 'RESET':
      return initialState
    default:
      return state
  }
}

interface CrawlProgressContextValue {
  crawlPhase: CrawlPhase
  pageCount: number
  jobId: string | null
  url: string | null
  isActive: boolean
  startCrawl: (jobId: string, url: string) => void
  reset: () => void
}

const CrawlProgressContext = createContext<CrawlProgressContextValue | null>(null)

function derivePhase(pageCount: number): CrawlPhase {
  if (pageCount < 3) return 'discovering'
  if (pageCount < 8) return 'mapping'
  if (pageCount < 15) return 'extracting'
  return 'building'
}

export function CrawlProgressProvider({ children }: { children: React.ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState)
  const esRef = useRef<EventSource | null>(null)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const stopPoll = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current)
      pollRef.current = null
    }
  }, [])

  const closeStream = useCallback(() => {
    if (esRef.current) {
      esRef.current.close()
      esRef.current = null
    }
    stopPoll()
  }, [stopPoll])

  const startCrawl = useCallback((jobId: string, url: string) => {
    closeStream()
    dispatch({ type: 'START', payload: { jobId, url } })

    const es = new EventSource(`/api/ingestion/crawl/${jobId}/stream`)
    esRef.current = es

    es.addEventListener('job:created', () => {
      dispatch({ type: 'SET_PHASE', payload: 'discovering' })
    })

    es.addEventListener('job:started', () => {
      dispatch({ type: 'SET_PHASE', payload: 'discovering' })
    })

    // Poll status every 4 s as a fallback for jobs that complete before the
    // EventSource has a chance to receive a terminal SSE event.
    const TERMINAL = new Set(['ready', 'completed', 'done', 'finished'])
    const FAILED   = new Set(['failed', 'cancelled', 'error'])

    pollRef.current = setInterval(async () => {
      try {
        const res = await fetch(`/api/ingestion/crawl/${jobId}/status`)
        if (!res.ok) return
        const d: { status: string; pages: number } = await res.json()
        if (TERMINAL.has(d.status)) {
          dispatch({ type: 'DONE', payload: d.pages ?? 0 })
          closeStream()
        } else if (FAILED.has(d.status)) {
          dispatch({ type: 'ERROR' })
          closeStream()
        }
      } catch {
        // ignore transient poll errors
      }
    }, 4000)

    es.addEventListener('heartbeat', (e) => {
      try {
        const data = JSON.parse(e.data)
        if (data.status === 'ready' || data.status === 'completed' || data.status === 'done') {
          dispatch({ type: 'DONE', payload: data.pages ?? 0 })
          closeStream()
          return
        }
        if (data.status === 'failed' || data.status === 'cancelled' || data.status === 'error') {
          dispatch({ type: 'ERROR' })
          closeStream()
          return
        }
        if (typeof data.pages === 'number' && data.pages > 0) {
          // pages reflects product count — use as proxy for crawl progress
          dispatch({ type: 'SET_PAGE_COUNT', payload: data.pages })
          dispatch({ type: 'SET_PHASE', payload: derivePhase(data.pages) })
        }
      } catch {
        // ignore malformed heartbeat
      }
    })

    es.addEventListener('progress', (e) => {
      try {
        const data = JSON.parse(e.data)
        const count = data.pages ?? data.total ?? data.current ?? 0
        if (count > 0) {
          dispatch({ type: 'SET_PAGE_COUNT', payload: count })
          dispatch({ type: 'SET_PHASE', payload: derivePhase(count) })
        }
      } catch {
        // ignore
      }
    })

    es.addEventListener('page_completed', (e) => {
      try {
        const data = JSON.parse(e.data)
        const newCount = data.pages ?? data.total ?? 0
        if (newCount > 0) {
          dispatch({ type: 'SET_PAGE_COUNT', payload: newCount })
          dispatch({ type: 'SET_PHASE', payload: derivePhase(newCount) })
        } else {
          dispatch({ type: 'INCREMENT_PAGES', payload: 1 })
        }
      } catch {
        dispatch({ type: 'INCREMENT_PAGES', payload: 1 })
      }
    })

    es.addEventListener('completed', (e) => {
      try {
        const data = JSON.parse(e.data)
        dispatch({ type: 'DONE', payload: data.pages ?? 0 })
      } catch {
        dispatch({ type: 'DONE', payload: 0 })
      }
      closeStream()
    })

    let errorCount = 0
    es.onerror = () => {
      if (esRef.current) {
        errorCount++
        if (errorCount >= 3) {
          dispatch({ type: 'ERROR' })
          closeStream()
        }
        // else: allow EventSource to auto-reconnect on transient errors
      }
    }
  }, [closeStream])

  const reset = useCallback(() => {
    closeStream()
    dispatch({ type: 'RESET' })
  }, [closeStream])

  useEffect(() => {
    return () => closeStream()
  }, [closeStream])

  const value: CrawlProgressContextValue = {
    crawlPhase: state.crawlPhase,
    pageCount: state.pageCount,
    jobId: state.jobId,
    url: state.url,
    isActive: state.isActive,
    startCrawl,
    reset,
  }

  return (
    <CrawlProgressContext.Provider value={value}>
      {children}
    </CrawlProgressContext.Provider>
  )
}

export function useCrawlProgress(): CrawlProgressContextValue {
  const ctx = useContext(CrawlProgressContext)
  if (!ctx) {
    throw new Error('useCrawlProgress must be used within CrawlProgressProvider')
  }
  return ctx
}
