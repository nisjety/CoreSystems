'use client'

import React, { useEffect, useState, useCallback } from 'react'
import { usePathname } from 'next/navigation'
import { Loader2, CheckCircle2, X, Globe } from 'lucide-react'
import { useCrawlProgress } from '../CrawlProgressContext'

export function CrawlProgressModal() {
  const { crawlPhase, pageCount, isActive, url, reset } = useCrawlProgress()
  const pathname = usePathname()
  const [visible, setVisible] = useState(false)
  const [dismissed, setDismissed] = useState(false)

  const isOnWebsitePage = pathname === '/onboarding/website'
  const shouldShow = isActive && crawlPhase !== 'idle' && !isOnWebsitePage && !dismissed

  useEffect(() => {
    if (shouldShow) {
      const t = setTimeout(() => setVisible(true), 100)
      return () => clearTimeout(t)
    }
    setVisible(false)
  }, [shouldShow])

  // Auto-dismiss 4s after done
  useEffect(() => {
    if (crawlPhase === 'done' && !isOnWebsitePage) {
      const t = setTimeout(() => {
        setDismissed(true)
        reset()
      }, 4000)
      return () => clearTimeout(t)
    }
  }, [crawlPhase, isOnWebsitePage, reset])

  // Reset dismissed state when a new crawl starts
  useEffect(() => {
    if (isActive) setDismissed(false)
  }, [isActive])

  const handleDismiss = useCallback(() => {
    setDismissed(true)
  }, [])

  if (!shouldShow) return null

  const phaseLabel = (() => {
    switch (crawlPhase) {
      case 'discovering': return 'Oppdager sider'
      case 'mapping': return 'Kartlegger innhold'
      case 'extracting': return 'Henter innhold'
      case 'building': return 'Bygger kunnskapsbase'
      case 'done': return 'Ferdig'
      case 'error': return 'Feil under crawling'
      default: return ''
    }
  })()

  const isDone = crawlPhase === 'done'
  const isError = crawlPhase === 'error'

  return (
    <div
      className={`fixed bottom-4 left-4 z-50 transition-all duration-300 ease-out ${
        visible ? 'translate-y-0 opacity-100' : 'translate-y-4 opacity-0'
      }`}
    >
      <div className="flex items-center gap-3 rounded-xl border border-[#D8D2C6] bg-white px-4 py-3 shadow-lg min-w-[280px]">
        {/* Icon */}
        <div className="flex-shrink-0">
          {isDone ? (
            <CheckCircle2 className="h-5 w-5 text-green-600" />
          ) : isError ? (
            <X className="h-5 w-5 text-red-500" />
          ) : (
            <Loader2 className="h-5 w-5 animate-spin text-[#FF2E63]" />
          )}
        </div>

        {/* Text */}
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-[#111111] truncate">
            {phaseLabel}
          </p>
          <p className="text-xs text-[#A09890] truncate">
            {isDone
              ? `${pageCount} sider indeksert`
              : isError
                ? (url ?? 'Ukjent nettsted')
                : `${pageCount} sider oppdaget…`}
          </p>
        </div>

        {/* URL indicator */}
        {url && !isDone && !isError && (
          <Globe className="h-3.5 w-3.5 text-[#A09890] flex-shrink-0" />
        )}

        {/* Close button */}
        <button
          onClick={handleDismiss}
          className="flex-shrink-0 rounded-md p-1 text-[#A09890] hover:bg-[#F4F1EB] hover:text-[#111111] transition-colors"
          aria-label="Lukk"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  )
}
