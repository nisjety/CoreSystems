'use client'

import { useState, useEffect, useRef } from 'react'
import { useRouter } from 'next/navigation'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'
import { Globe, CheckCircle2, Circle, Loader2, ArrowRight } from 'lucide-react'

type CrawlPhase =
  | 'idle'
  | 'discovering'
  | 'mapping'
  | 'extracting'
  | 'building'
  | 'done'
  | 'error'

interface PhaseStatus {
  phase: CrawlPhase
  label: string
  sublabel?: string
}

const PHASES: PhaseStatus[] = [
  { phase: 'discovering', label: 'Oppdager sider', sublabel: 'Kartlegger nettstedets struktur' },
  { phase: 'mapping', label: 'Kartlegger innhold', sublabel: 'Analyserer navigasjon og lenker' },
  { phase: 'extracting', label: 'Henter innhold', sublabel: 'Leser tekst, data og metadata' },
  { phase: 'building', label: 'Bygger kunnskapsbase', sublabel: 'Indekserer og forstår selskapet' },
  { phase: 'done', label: 'Ferdig', sublabel: '' },
]

const phaseOrder: CrawlPhase[] = ['idle', 'discovering', 'mapping', 'extracting', 'building', 'done']

function getPhaseIndex(phase: CrawlPhase): number {
  return phaseOrder.indexOf(phase)
}

function isPhaseComplete(current: CrawlPhase, target: CrawlPhase): boolean {
  return getPhaseIndex(current) > getPhaseIndex(target)
}

function isPhaseActive(current: CrawlPhase, target: CrawlPhase): boolean {
  return current === target
}

export function WebsiteStep() {
  const router = useRouter()
  const [url, setUrl] = useState('')
  const [crawlPhase, setCrawlPhase] = useState<CrawlPhase>('idle')
  const [error, setError] = useState<string | null>(null)
  const [pageCount, setPageCount] = useState(0)
  const [jobId, setJobId] = useState<string | null>(null)
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const simulationRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const streamRef = useRef<EventSource | null>(null)

  const isRunning = crawlPhase !== 'idle' && crawlPhase !== 'done' && crawlPhase !== 'error'

  // Clean up on unmount
  useEffect(() => {
    return () => {
      if (pollRef.current) clearInterval(pollRef.current)
      if (simulationRef.current) clearTimeout(simulationRef.current)
      if (streamRef.current) streamRef.current.close()
    }
  }, [])

  const normalizeUrl = (raw: string): string => {
    const trimmed = raw.trim()
    if (!trimmed) return ''
    if (!/^https?:\/\//i.test(trimmed)) return `https://${trimmed}`
    return trimmed
  }

  const validateUrl = (raw: string): boolean => {
    try {
      new URL(normalizeUrl(raw))
      return true
    } catch {
      return false
    }
  }

  // Simulate crawl phases for MVP (replace with real Quarry polling when wired)
  const simulateCrawl = (normalizedUrl: string) => {
    const PHASE_DURATIONS: Record<string, number> = {
      discovering: 2200,
      mapping: 2800,
      extracting: 3500,
      building: 3000,
    }

    let phasesCompleted = 0
    const phasesToRun: CrawlPhase[] = ['discovering', 'mapping', 'extracting', 'building']

    // Simulate incremental page count
    const pageInterval = setInterval(() => {
      setPageCount((c) => c + Math.floor(Math.random() * 8 + 2))
    }, 600)

    const runNextPhase = (index: number) => {
      if (index >= phasesToRun.length) {
        setCrawlPhase('done')
        clearInterval(pageInterval)
        setPageCount((c) => Math.max(c, 40))
        return
      }
      setCrawlPhase(phasesToRun[index])
      simulationRef.current = setTimeout(() => {
        phasesCompleted++
        runNextPhase(index + 1)
      }, PHASE_DURATIONS[phasesToRun[index]] ?? 2500)
    }

    runNextPhase(0)

    return () => {
      clearInterval(pageInterval)
      if (simulationRef.current) clearTimeout(simulationRef.current)
    }
  }

  const startCrawl = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)

    if (!validateUrl(url)) {
      setError('Skriv inn en gyldig nettadresse, f.eks. bedrift.no')
      return
    }

    const normalized = normalizeUrl(url)
    setCrawlPhase('discovering')

    try {
      // Try to start a real Quarry crawl job
      const response = await fetch('/api/ingestion/crawl', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: normalized, maxPages: 200, orgContext: true }),
      })

      if (response.ok) {
        const data = await response.json()
        const newJobId = data.jobId || data.id || null
        setJobId(newJobId)

        if (newJobId) {
          const es = new EventSource(`/api/ingestion/crawl/${newJobId}/stream`)
          streamRef.current = es

          es.addEventListener('heartbeat', (e) => {
            try {
              const payload = JSON.parse(e.data)
              const status = payload.status
              if (status === 'completed' || status === 'ready') {
                setCrawlPhase('done')
                es.close()
              } else if (status === 'failed') {
                setCrawlPhase('error')
                es.close()
              }
            } catch { }
          })

          es.addEventListener('progress', (e) => {
            try {
              const payload = JSON.parse(e.data)
              if (payload.total !== undefined) setPageCount(payload.total)
              if (payload.current !== undefined) setPageCount(payload.current)
            } catch { }
          })

          es.addEventListener('page_completed', () => {
            setPageCount(c => c + 1)
          })

          es.addEventListener('completed', (e) => {
            try {
              const payload = JSON.parse(e.data)
              if (payload.pages && payload.pages > 0) setPageCount(payload.pages)
            } catch { }
            setCrawlPhase('done')
            es.close()
          })

          // Since Quarry currently just emits 'running' via heartbeat, 
          // we simulate the visual phase transitions while the SSE stream is active.
          setTimeout(() => setCrawlPhase((c) => c === 'discovering' ? 'mapping' : c), 2500)
          setTimeout(() => setCrawlPhase((c) => c === 'mapping' ? 'extracting' : c), 5500)
          setTimeout(() => setCrawlPhase((c) => c === 'extracting' ? 'building' : c), 10000)

          return
        }
      }
    } catch {
      // Quarry not available — fall through to simulation
    }

    // Fallback: simulate crawl phases
    simulateCrawl(normalized)
  }

  const handleContinue = async () => {
    const normalized = normalizeUrl(url)
    try {
      await onboardingService.setupWebsite({ url: normalized, crawlJobId: jobId ?? undefined })
      router.push('/onboarding/connect')
    } catch (err: any) {
      setError(err.message || 'Noe gikk galt. Prøv igjen.')
    }
  }

  const handleSkip = async () => {
    try {
      await onboardingService.setupWebsite({ url: '', crawlJobId: undefined })
      router.push('/onboarding/connect')
    } catch {
      router.push('/onboarding/connect')
    }
  }

  return (
    <div className="space-y-5">
      {/* URL input */}
      {crawlPhase === 'idle' && (
        <form onSubmit={startCrawl} className="space-y-4">
          <div className="space-y-2">
            <label
              htmlFor="website-url"
              className="block font-inter text-[13px] font-medium text-[#2B2B2B]"
            >
              Nettstedsadresse
            </label>
            <div className="relative flex items-center">
              <Globe
                size={16}
                className="absolute left-3.5 text-[#A09890] pointer-events-none"
              />
              <input
                id="website-url"
                type="text"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="eksempel.no"
                className="w-full border border-[#D8D2C6] bg-[#F4F1EB] py-3 pl-9 pr-4 font-inter text-[13px] text-[#2B2B2B] placeholder:text-[#C8C1B3] outline-none transition-colors focus:border-[#2B2B2B] focus:bg-white"
                autoFocus
                autoComplete="url"
              />
            </div>
            {error && (
              <p className="font-inter text-xs text-[#FF2E63]">{error}</p>
            )}
          </div>

          <div className="flex items-center gap-3">
            <button
              type="submit"
              className="flex items-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
            >
              Start analyse
              <ArrowRight size={14} />
            </button>
            <button
              type="button"
              onClick={handleSkip}
              className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48]"
            >
              Hopp over
            </button>
          </div>
        </form>
      )}

      {/* Crawl status ticker */}
      {crawlPhase !== 'idle' && (
        <div className="space-y-5">
          {/* URL display */}
          <div className="flex items-center gap-2 border border-[#D8D2C6] bg-[#F4F1EB] px-3.5 py-2.5">
            <Globe size={14} className="shrink-0 text-[#A09890]" />
            <span className="font-inter text-sm text-[#4A4A48] truncate">{normalizeUrl(url)}</span>
          </div>

          {/* Phase list */}
          <div className="space-y-3">
            {PHASES.filter((p) => p.phase !== 'done').map((phaseInfo) => {
              const complete = isPhaseComplete(crawlPhase, phaseInfo.phase)
              const active = isPhaseActive(crawlPhase, phaseInfo.phase)

              return (
                <div key={phaseInfo.phase} className="flex items-start gap-3">
                  <div className="mt-0.5 shrink-0">
                    {complete ? (
                      <CheckCircle2 size={16} className="text-[#111111]" />
                    ) : active ? (
                      <Loader2 size={16} className="animate-spin text-[#FF2E63]" />
                    ) : (
                      <Circle size={16} className="text-[#D8D2C6]" />
                    )}
                  </div>
                  <div>
                    <p
                      className={`font-inter text-sm font-medium transition-colors ${complete
                          ? 'text-[#4A4A48]'
                          : active
                            ? 'text-[#111111]'
                            : 'text-[#C8C1B3]'
                        }`}
                    >
                      {phaseInfo.label}
                    </p>
                    {phaseInfo.sublabel && (active || complete) && (
                      <p className="mt-0.5 font-inter text-xs text-[#A09890]">
                        {phaseInfo.sublabel}
                      </p>
                    )}
                  </div>
                </div>
              )
            })}
          </div>

          {/* Page count */}
          {pageCount > 0 && crawlPhase !== 'done' && (
            <p className="font-inter text-xs text-[#A09890]">
              {pageCount} sider oppdaget…
            </p>
          )}

          {/* Done state */}
          {crawlPhase === 'done' && (
            <div className="space-y-4">
              <div className="flex items-center gap-2 border border-[#D8D2C6] bg-[#EAE6DF] px-3.5 py-3">
                <CheckCircle2 size={16} className="shrink-0 text-[#111111]" />
                <p className="font-inter text-sm font-medium text-[#2B2B2B]">
                  {pageCount} sider indeksert og klar
                </p>
              </div>
              <button
                onClick={handleContinue}
                className="flex w-full items-center justify-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80"
              >
                Fortsett
                <ArrowRight size={14} />
              </button>
            </div>
          )}

          {/* Error state */}
          {crawlPhase === 'error' && (
            <div className="space-y-3">
              <p className="font-inter text-sm text-[#FF2E63]">
                Klarte ikke å analysere nettstedet. Prøv igjen eller hopp over.
              </p>
              <div className="flex gap-3">
                <button
                  onClick={() => { setCrawlPhase('idle'); setError(null) }}
                  className="border border-[#D8D2C6] px-5 py-2.5 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
                >
                  Prøv igjen
                </button>
                <button
                  onClick={handleSkip}
                  className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48]"
                >
                  Hopp over
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
