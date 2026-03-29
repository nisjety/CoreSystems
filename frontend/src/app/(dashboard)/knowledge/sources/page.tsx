'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { SourcesList, type KnowledgeSource } from '@/components/knowledge/SourcesList';
import { Plus, Globe, Share2, FolderOpen } from 'lucide-react';

const MOCK_SOURCES: KnowledgeSource[] = [];

const CONNECTOR_TYPES = [
  { id: 'website',    icon: Globe,       label: 'Nettside',   description: 'Crawl en nettside og indekser alt innhold.' },
  { id: 'sharepoint', icon: Share2,      label: 'SharePoint', description: 'Koble til Microsoft SharePoint-biblioteker.' },
  { id: 'files',      icon: FolderOpen,  label: 'Filer',      description: 'Last opp PDF, Word og andre dokumenter direkte.' },
];

export default function SourcesPage() {
  const router = useRouter();
  const [showAdd, setShowAdd] = useState(false);

  return (
    <div className="h-full overflow-y-auto bg-[#F4F1EB]">
      {/* Header */}
      <div className="border-b border-[#D8D2C6] px-6 py-8 md:px-10">
        <div className="mx-auto max-w-4xl flex items-end justify-between">
          <div>
            <p className="mb-1 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
              Kunnskapsbase
            </p>
            <h1
              className="text-[36px] font-normal text-[#2B2B2B]"
              style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
            >
              Datakilder
            </h1>
          </div>
          <button
            onClick={() => setShowAdd(true)}
            className="flex items-center gap-2 border border-[#D8D2C6] bg-[#EAE6DF] px-5 py-2.5 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#2B2B2B] hover:text-white"
          >
            <Plus size={12} strokeWidth={1.5} />
            Legg til kilde
          </button>
        </div>
      </div>

      <div className="px-6 py-10 md:px-10">
        <div className="mx-auto max-w-4xl space-y-10">

          <SourcesList
            sources={MOCK_SOURCES}
            onAdd={() => setShowAdd(true)}
          />

          {/* Add source panel */}
          {showAdd && (
            <div>
              <p className="mb-4 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
                Velg connector
              </p>
              <div className="grid gap-px border border-[#D8D2C6] bg-[#D8D2C6] sm:grid-cols-3">
                {CONNECTOR_TYPES.map((ct) => {
                  const Icon = ct.icon;
                  return (
                    <button
                      key={ct.id}
                      onClick={() => {
                        if (ct.id === 'website') router.push('/onboarding/website');
                        else if (ct.id === 'sharepoint') router.push('/onboarding/connect');
                      }}
                      className="group flex flex-col gap-3 bg-[#F4F1EB] p-6 text-left transition-colors hover:bg-[#EAE6DF]"
                    >
                      <Icon size={16} strokeWidth={1.25} className="text-[#A09890] group-hover:text-[#2B2B2B]" />
                      <div>
                        <p
                          className="text-[18px] font-normal text-[#2B2B2B]"
                          style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
                        >
                          {ct.label}
                        </p>
                        <p className="mt-1 font-inter text-[12px] text-[#A09890]">{ct.description}</p>
                      </div>
                    </button>
                  );
                })}
              </div>
              <button
                onClick={() => setShowAdd(false)}
                className="mt-3 font-inter text-[11px] text-[#C8C1B3] hover:text-[#2B2B2B]"
              >
                Avbryt
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
