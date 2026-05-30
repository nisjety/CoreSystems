'use client'

import { useRouter } from 'next/navigation'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'
import { Globe, CheckCircle2, Circle, Loader2, ArrowRight } from 'lucide-react'
import { useWebsiteStepState, type CrawlPhase } from './use-website-step-state'
import { useCrawlProgress } from '../CrawlProgressContext'

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

function UrlInputForm({
  url,
  error,
  onUrlChange,
  onSubmit,
  onSkip,
  onBack,
}: {
  url: string
  error: string | null
  onUrlChange: (value: string) => void
  onSubmit: (e: React.FormEvent) => void
  onSkip: () => void
  onBack: () => void
}) {
  return (
    <form onSubmit={onSubmit} data-testid="onboarding-website-form" className="space-y-4">
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
            data-testid="onboarding-website-url"
            type="text"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            placeholder="eksempel.no"
            className="w-full border border-[#D8D2C6] bg-[#F4F1EB] py-3 pl-9 pr-4 font-inter text-[13px] text-[#2B2B2B] placeholder:text-[#C8C1B3] outline-none transition-colors focus:border-[#2B2B2B] focus:bg-white"
            autoComplete="url"
          />
        </div>
        {error && (
          <p data-testid="onboarding-website-error" className="font-inter text-xs text-[#FF2E63]">{error}</p>
        )}
      </div>

      <div className="flex items-center justify-between">
        <button
          type="button"
          data-testid="onboarding-website-back"
          onClick={onBack}
          className="border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
        >
          Tilbake
        </button>
        <div className="flex items-center gap-3">
          <button
            type="submit"
            data-testid="onboarding-website-submit"
            className="flex items-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80 disabled:opacity-40"
          >
            Start analyse
            <ArrowRight size={14} />
          </button>
          <button
            type="button"
            data-testid="onboarding-website-skip"
            onClick={onSkip}
            className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48]"
          >
            Hopp over
          </button>
        </div>
      </div>
    </form>
  )
}

function CrawlStatusView({
  displayUrl,
  crawlPhase,
  pageCount,
  isRunning,
  onContinue,
  onRetry,
  onSkip,
}: {
  displayUrl: string
  crawlPhase: CrawlPhase
  pageCount: number
  isRunning: boolean
  onContinue: () => void
  onRetry: () => void
  onSkip: () => void
}) {
  return (
    <div className="space-y-5">
      <div data-testid="onboarding-website-crawl-url" className="flex items-center gap-2 border border-[#D8D2C6] bg-[#F4F1EB] px-3.5 py-2.5">
        <Globe size={14} className="shrink-0 text-[#A09890]" />
        <span className="font-inter text-sm text-[#4A4A48] truncate">{displayUrl}</span>
      </div>

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

      {pageCount > 0 && crawlPhase !== 'done' && (
        <p className="font-inter text-xs text-[#A09890]">
          {pageCount} sider oppdaget…
        </p>
      )}

      {isRunning && (
        <button
          data-testid="onboarding-website-continue-early"
          onClick={onContinue}
          className="flex w-full items-center justify-center gap-2 border border-[#D8D2C6] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
        >
          Fortsett mens vi jobber
          <ArrowRight size={14} />
        </button>
      )}

      {crawlPhase === 'done' && (
        <div className="space-y-4">
          <div className="flex items-center gap-2 border border-[#D8D2C6] bg-[#EAE6DF] px-3.5 py-3">
            <CheckCircle2 size={16} className="shrink-0 text-[#111111]" />
            <p className="font-inter text-sm font-medium text-[#2B2B2B]">
              {pageCount} sider indeksert og klar
            </p>
          </div>
          <button
            data-testid="onboarding-website-continue"
            onClick={onContinue}
            className="flex w-full items-center justify-center gap-2 bg-[#111111] px-6 py-3 font-inter text-[11px] uppercase tracking-widest text-white transition-opacity hover:opacity-80"
          >
            Fortsett
            <ArrowRight size={14} />
          </button>
        </div>
      )}

      {crawlPhase === 'error' && (
        <div className="space-y-3">
          <p className="font-inter text-sm text-[#FF2E63]">
            Klarte ikke å analysere nettstedet. Prøv igjen eller hopp over.
          </p>
          <div className="flex gap-3">
            <button
              data-testid="onboarding-website-retry"
              onClick={onRetry}
              className="border border-[#D8D2C6] px-5 py-2.5 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
            >
              Prøv igjen
            </button>
            <button
              data-testid="onboarding-website-crawl-skip"
              onClick={onSkip}
              className="font-inter text-sm text-[#A09890] transition-colors hover:text-[#4A4A48]"
            >
              Hopp over
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

export function WebsiteStep() {
  const router = useRouter()
  const [state, dispatch] = useWebsiteStepState()
  const { url, crawlPhase, error, pageCount, jobId } = state
  const ctx = useCrawlProgress()

  const effectivePhase: CrawlPhase = ctx.isActive ? ctx.crawlPhase : crawlPhase
  const effectivePages = ctx.isActive ? ctx.pageCount : pageCount
  const isRunning = effectivePhase !== 'idle' && effectivePhase !== 'done' && effectivePhase !== 'error'

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

  const startCrawl = async (e: React.FormEvent) => {
    e.preventDefault()
    dispatch({ type: 'SET_ERROR', payload: null })

    if (!validateUrl(url)) {
      dispatch({ type: 'SET_ERROR', payload: 'Skriv inn en gyldig nettadresse, f.eks. bedrift.no' })
      return
    }

    const normalized = normalizeUrl(url)
    dispatch({ type: 'START_CRAWL' })

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
        dispatch({ type: 'SET_JOB_ID', payload: newJobId })

        if (newJobId) {
          ctx.startCrawl(newJobId, normalized)
          return
        }
      }
    } catch {
      dispatch({ type: 'CRAWL_ERROR' })
    }
  }

  const handleContinue = async () => {
    const normalized = normalizeUrl(url)
    try {
      await onboardingService.setupWebsite({ url: normalized, crawlJobId: jobId ?? undefined })
      router.push('/onboarding/connect')
    } catch (err: any) {
      dispatch({ type: 'SET_ERROR', payload: err.message || 'Noe gikk galt. Prøv igjen.' })
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
      {effectivePhase === 'idle' && (
        <UrlInputForm
          url={url}
          error={error}
          onUrlChange={(v) => dispatch({ type: 'SET_URL', payload: v })}
          onSubmit={startCrawl}
          onSkip={handleSkip}
          onBack={async () => { await onboardingService.saveCurrentStep('organization'); router.back() }}
        />
      )}

      {effectivePhase !== 'idle' && (
        <CrawlStatusView
          displayUrl={normalizeUrl(url)}
          crawlPhase={effectivePhase}
          pageCount={effectivePages}
          isRunning={isRunning}
          onContinue={handleContinue}
          onRetry={() => { ctx.reset(); dispatch({ type: 'SET_CRAWL_PHASE', payload: 'idle' }); dispatch({ type: 'SET_ERROR', payload: null }) }}
          onSkip={handleSkip}
        />
      )}
    </div>
  )
}
