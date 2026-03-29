'use client'

import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import type { DashboardStats } from '@/app/api/dashboard/stats/route'

interface Props {
  stats: DashboardStats | null
}

function formatRelativeTime(isoString: string | null): string {
  if (!isoString) return ''
  try {
    const diff = Date.now() - new Date(isoString).getTime()
    const minutes = Math.floor(diff / 60_000)
    if (minutes < 1) return 'nettopp nå'
    if (minutes < 60) return `${minutes} min siden`
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return `${hours} t siden`
    return `${Math.floor(hours / 24)} d siden`
  } catch {
    return ''
  }
}

export function KnowledgeStatusCard({ stats }: Props) {
  const pages = stats?.crawledPages
  const relative = formatRelativeTime(stats?.lastCrawlAt ?? null)
  const hasData = pages != null && pages > 0

  return (
    <Link
      href="/knowledge"
      className="group flex flex-col border border-[#D8D2C6] bg-white transition-colors hover:bg-[#F4F1EB]"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Kunnskapsbase
        </span>
        <span
          className={`h-1.5 w-1.5 rounded-full ${
            hasData ? 'bg-[#111111]' : 'bg-[#D8D2C6]'
          }`}
        />
      </div>

      {/* Body */}
      <div className="flex flex-1 flex-col justify-between px-5 py-5">
        {hasData ? (
          <>
            <div>
              <p
                className="text-[42px] font-normal leading-none text-[#2B2B2B]"
                style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
              >
                {pages!.toLocaleString('nb-NO')}
              </p>
              <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">sider indeksert</p>
              {relative && (
                <p className="mt-3 font-inter text-[11px] text-[#C8C1B3]">
                  Sist oppdatert {relative}
                </p>
              )}
            </div>
          </>
        ) : (
          <div>
            <p
              className="text-[22px] font-normal leading-snug text-[#C8C1B3]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              Ingen innhold ennå
            </p>
            <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
              Koble til en nettside under oppsett
            </p>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between border-t border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Se oversikt
        </span>
        <ArrowRight
          size={13}
          className="text-[#C8C1B3] transition-transform group-hover:translate-x-1 group-hover:text-[#2B2B2B]"
        />
      </div>
    </Link>
  )
}
