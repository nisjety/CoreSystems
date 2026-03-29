'use client'

import Link from 'next/link'
import { ArrowRight, Loader2 } from 'lucide-react'
import type { DashboardStats } from '@/app/api/dashboard/stats/route'

interface Props {
  stats: DashboardStats | null
}

const STATUS_LABELS: Record<string, string> = {
  idle: 'Venter',
  running: 'Kjører nå',
  done: 'Fullført',
  error: 'Feil',
  discovering: 'Oppdager sider',
  mapping: 'Kartlegger',
  extracting: 'Henter innhold',
  building: 'Bygger base',
}

const RUNNING_STATES = new Set(['running', 'discovering', 'mapping', 'extracting', 'building'])

export function CrawlStatusCard({ stats }: Props) {
  const status = stats?.crawlStatus as string | null | undefined
  const pages = stats?.crawledPages
  const isRunning = status != null && RUNNING_STATES.has(status)
  const isDone = status === 'done'
  const isError = status === 'error'
  const hasActivity = status != null

  return (
    <Link
      href="/knowledge"
      className="group flex flex-col border border-[#D8D2C6] bg-white transition-colors hover:bg-[#F4F1EB]"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Indeksering
        </span>
        {isRunning ? (
          <Loader2 size={11} className="animate-spin text-[#FF2E63]" />
        ) : (
          <span
            className={`h-1.5 w-1.5 rounded-full ${
              isDone ? 'bg-[#111111]' : isError ? 'bg-[#FF2E63]' : 'bg-[#D8D2C6]'
            }`}
          />
        )}
      </div>

      {/* Body */}
      <div className="flex flex-1 flex-col justify-between px-5 py-5">
        {hasActivity ? (
          <div>
            <p
              className={`text-[22px] font-normal leading-snug ${
                isError ? 'text-[#FF2E63]' : 'text-[#2B2B2B]'
              }`}
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              {STATUS_LABELS[status!] ?? status}
            </p>
            {pages != null && (
              <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
                {pages.toLocaleString('nb-NO')} sider{isRunning ? ' oppdaget…' : ' indeksert'}
              </p>
            )}
          </div>
        ) : (
          <div>
            <p
              className="text-[22px] font-normal leading-snug text-[#C8C1B3]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              Ingen indeksering ennå
            </p>
            <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
              Start ved å koble til en nettside
            </p>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between border-t border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          {isRunning ? 'Se fremgang' : 'Detaljer'}
        </span>
        <ArrowRight
          size={13}
          className="text-[#C8C1B3] transition-transform group-hover:translate-x-1 group-hover:text-[#2B2B2B]"
        />
      </div>
    </Link>
  )
}
