'use client';

import { CheckCircle2, AlertCircle, Clock, Loader2 } from 'lucide-react';

interface IndexStatusData {
  totalSources: number;
  activeSources: number;
  totalPages: number;
  lastIndexed?: string;
  isIndexing: boolean;
  errorCount: number;
}

interface IndexStatusProps {
  data?: IndexStatusData;
  isLoading?: boolean;
}

const PLACEHOLDER: IndexStatusData = {
  totalSources: 0,
  activeSources: 0,
  totalPages: 0,
  isIndexing: false,
  errorCount: 0,
};

export function IndexStatus({ data = PLACEHOLDER, isLoading = false }: IndexStatusProps) {
  if (isLoading) {
    return (
      <div className="flex items-center gap-2 py-6 text-[#A09890]">
        <Loader2 size={14} strokeWidth={1.5} className="animate-spin" />
        <span className="font-inter text-sm">Laster status…</span>
      </div>
    );
  }

  const stats = [
    { label: 'Datakilder', value: data.totalSources, sub: `${data.activeSources} aktive` },
    { label: 'Indekserte sider', value: data.totalPages.toLocaleString('nb-NO'), sub: data.lastIndexed ? `Sist: ${new Date(data.lastIndexed).toLocaleDateString('nb-NO')}` : '—' },
    { label: 'Feil', value: data.errorCount, sub: data.errorCount === 0 ? 'Alt OK' : 'Krever tilsyn' },
  ];

  return (
    <div className="border border-[#D8D2C6]">
      {/* Status bar */}
      <div className="flex items-center justify-between border-b border-[#D8D2C6] px-5 py-3">
        <div className="flex items-center gap-2">
          {data.isIndexing ? (
            <>
              <Loader2 size={13} strokeWidth={1.5} className="animate-spin text-[#C8A87A]" />
              <span className="font-inter text-[11px] uppercase tracking-widest text-[#C8A87A]">Indekserer…</span>
            </>
          ) : data.errorCount > 0 ? (
            <>
              <AlertCircle size={13} strokeWidth={1.5} className="text-[#FF2E63]" />
              <span className="font-inter text-[11px] uppercase tracking-widest text-[#FF2E63]">Feil oppdaget</span>
            </>
          ) : (
            <>
              <CheckCircle2 size={13} strokeWidth={1.5} className="text-[#4A9B6F]" />
              <span className="font-inter text-[11px] uppercase tracking-widest text-[#4A9B6F]">Oppdatert</span>
            </>
          )}
        </div>
        <Clock size={13} strokeWidth={1.5} className="text-[#C8C1B3]" />
      </div>

      {/* Stat cells */}
      <div className="grid grid-cols-3 divide-x divide-[#D8D2C6]">
        {stats.map((stat) => (
          <div key={stat.label} className="px-5 py-5">
            <p
              className="text-[28px] font-normal leading-none text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              {stat.value}
            </p>
            <p className="mt-1.5 font-inter text-[11px] uppercase tracking-widest text-[#A09890]">
              {stat.label}
            </p>
            <p className="mt-0.5 font-inter text-[11px] text-[#C8C1B3]">{stat.sub}</p>
          </div>
        ))}
      </div>
    </div>
  );
}
