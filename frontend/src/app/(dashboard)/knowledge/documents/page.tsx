'use client';

import { useState } from 'react';
import { FileText, Search, Filter, ExternalLink } from 'lucide-react';

interface Document {
  id: string;
  title: string;
  source: string;
  type: string;
  pages?: number;
  size?: string;
  indexedAt: string;
  url?: string;
}

const MOCK_DOCUMENTS: Document[] = [];

export default function DocumentsPage() {
  const [query, setQuery] = useState('');

  const filtered = MOCK_DOCUMENTS.filter(
    (d) =>
      !query ||
      d.title.toLowerCase().includes(query.toLowerCase()) ||
      d.source.toLowerCase().includes(query.toLowerCase()),
  );

  return (
    <div className="h-full overflow-y-auto bg-[#F4F1EB]">
      {/* Header */}
      <div className="border-b border-[#D8D2C6] px-6 py-8 md:px-10">
        <div className="mx-auto max-w-4xl">
          <p className="mb-1 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
            Kunnskapsbase
          </p>
          <h1
            className="text-[36px] font-normal text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Dokumenter
          </h1>
        </div>
      </div>

      <div className="px-6 py-10 md:px-10">
        <div className="mx-auto max-w-4xl space-y-6">

          {/* Search bar */}
          <div className="relative">
            <Search size={14} strokeWidth={1.5} className="pointer-events-none absolute left-4 top-1/2 -translate-y-1/2 text-[#C8C1B3]" />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Søk i dokumenter…"
              className="w-full border border-[#D8D2C6] bg-white py-3 pl-10 pr-4 font-inter text-[13px] text-[#2B2B2B] outline-none transition-colors placeholder:text-[#C8C1B3] focus:border-[#2B2B2B]"
            />
          </div>

          {/* Table / empty state */}
          {filtered.length === 0 ? (
            <div className="border border-dashed border-[#D8D2C6] py-20 text-center">
              <FileText size={24} strokeWidth={1} className="mx-auto mb-4 text-[#D8D2C6]" />
              <p
                className="mb-2 text-[20px] font-normal text-[#2B2B2B]"
                style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
              >
                Ingen dokumenter enda
              </p>
              <p className="font-inter text-[13px] text-[#A09890]">
                Koble til en datakilde eller last opp filer for å komme i gang.
              </p>
            </div>
          ) : (
            <div className="border border-[#D8D2C6]">
              {/* Header row */}
              <div className="grid grid-cols-[minmax(0,1fr)_120px_80px_100px_32px] gap-4 border-b border-[#D8D2C6] px-5 py-2.5">
                {['Tittel', 'Kilde', 'Sider', 'Indeksert', ''].map((col) => (
                  <span key={col} className="font-inter text-[10px] uppercase tracking-widest text-[#C8C1B3]">
                    {col}
                  </span>
                ))}
              </div>
              {filtered.map((doc, i) => (
                <div
                  key={doc.id}
                  className={`grid grid-cols-[minmax(0,1fr)_120px_80px_100px_32px] items-center gap-4 px-5 py-3.5 ${i > 0 ? 'border-t border-[#D8D2C6]' : ''} hover:bg-[#EAE6DF]`}
                >
                  <div className="flex items-center gap-2.5 min-w-0">
                    <FileText size={13} strokeWidth={1.5} className="flex-shrink-0 text-[#A09890]" />
                    <span className="font-inter text-[13px] text-[#2B2B2B] truncate">{doc.title}</span>
                  </div>
                  <span className="font-inter text-[12px] text-[#A09890] truncate">{doc.source}</span>
                  <span className="font-inter text-[12px] text-[#A09890]">{doc.pages ?? '—'}</span>
                  <span className="font-inter text-[11px] text-[#C8C1B3]">
                    {new Date(doc.indexedAt).toLocaleDateString('nb-NO')}
                  </span>
                  {doc.url ? (
                    <a href={doc.url} target="_blank" rel="noopener noreferrer" className="text-[#C8C1B3] hover:text-[#2B2B2B]">
                      <ExternalLink size={12} strokeWidth={1.5} />
                    </a>
                  ) : (
                    <span />
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
