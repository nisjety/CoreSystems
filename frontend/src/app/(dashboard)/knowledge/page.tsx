'use client';

import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { ArrowRight } from 'lucide-react';
import { IndexStatus } from '@/components/knowledge/IndexStatus';
import { SourcesList, type KnowledgeSource } from '@/components/knowledge/SourcesList';

// Mock data — replace with real API calls to Quarry
const MOCK_STATUS = {
  totalSources: 0,
  activeSources: 0,
  totalPages: 0,
  isIndexing: false,
  errorCount: 0,
};

const MOCK_SOURCES: KnowledgeSource[] = [];

const SECTIONS = [
  { href: '/knowledge/sources',   label: 'Nettsider og datakilder',   description: 'Administrer tilkoblede nettsider, SharePoint og andre connectors.' },
  { href: '/knowledge/documents', label: 'Dokumenter',                 description: 'Bla gjennom indekserte filer og opplastede dokumenter.' },
];

export default function KnowledgePage() {
  const router = useRouter();

  return (
    <div className="h-full overflow-y-auto bg-[#F4F1EB]">
      {/* Page header */}
      <div className="border-b border-[#D8D2C6] px-6 py-8 md:px-10">
        <div className="mx-auto max-w-4xl">
          <p className="mb-1 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
            Oversikt
          </p>
          <h1
            className="text-[36px] font-normal text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Kunnskapsbase
          </h1>
          <p className="mt-1 font-inter text-[13px] text-[#A09890]">
            Alt AI-assistenten vet om din organisasjon.
          </p>
        </div>
      </div>

      <div className="px-6 py-10 md:px-10">
        <div className="mx-auto max-w-4xl space-y-10">

          {/* Index health */}
          <section>
            <p className="mb-4 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
              Indeksstatus
            </p>
            <IndexStatus data={MOCK_STATUS} />
          </section>

          {/* Recent sources */}
          <section>
            <SourcesList
              sources={MOCK_SOURCES}
              onAdd={() => router.push('/knowledge/sources')}
            />
          </section>

          {/* Sub-section nav */}
          <section>
            <p className="mb-4 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
              Utforsk
            </p>
            <div className="grid gap-px border border-[#D8D2C6] bg-[#D8D2C6] sm:grid-cols-2">
              {SECTIONS.map((s) => (
                <Link
                  key={s.href}
                  href={s.href}
                  className="group flex flex-col justify-between bg-[#F4F1EB] p-6 transition-colors hover:bg-[#EAE6DF]"
                >
                  <h3
                    className="text-[20px] font-normal text-[#2B2B2B]"
                    style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
                  >
                    {s.label}
                  </h3>
                  <div className="mt-3 flex items-end justify-between">
                    <p className="max-w-[36ch] font-inter text-[12px] leading-relaxed text-[#A09890]">
                      {s.description}
                    </p>
                    <ArrowRight
                      size={14}
                      strokeWidth={1.5}
                      className="ml-4 shrink-0 text-[#C8C1B3] transition-transform group-hover:translate-x-1 group-hover:text-[#2B2B2B]"
                    />
                  </div>
                </Link>
              ))}
            </div>
          </section>

        </div>
      </div>
    </div>
  );
}
