'use client'

import Link from 'next/link'
import { ArrowRight } from 'lucide-react'
import type { DashboardStats } from '@/app/api/dashboard/stats/route'

interface Props {
  stats: DashboardStats | null
}

export function SourcesCard({ stats }: Props) {
  const sources = stats?.sourceCount
  const docs = stats?.documentCount
  const hasData = sources != null && sources > 0

  return (
    <Link
      href="/knowledge/sources"
      className="group flex flex-col border border-[#D8D2C6] bg-white transition-colors hover:bg-[#F4F1EB]"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Datakilder
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
                {sources!.toLocaleString('nb-NO')}
              </p>
              <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
                {sources === 1 ? 'kilde koblet til' : 'kilder koblet til'}
              </p>
              {docs != null && (
                <p className="mt-3 font-inter text-[11px] text-[#C8C1B3]">
                  {docs.toLocaleString('nb-NO')} dokumenter totalt
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
              Ingen datakilder ennå
            </p>
            <p className="mt-1.5 font-inter text-[12px] text-[#A09890]">
              Koble til SharePoint, OneDrive eller nettsider
            </p>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between border-t border-[#D8D2C6] px-5 py-3">
        <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">
          Administrer
        </span>
        <ArrowRight
          size={13}
          className="text-[#C8C1B3] transition-transform group-hover:translate-x-1 group-hover:text-[#2B2B2B]"
        />
      </div>
    </Link>
  )
}
