'use client';

import { ExternalLink, FileText, Globe, Mail, MessageSquare, Share2, type LucideIcon } from 'lucide-react';
import type { SearchSource } from '@/lib/api/search-api';

const TYPE_ICONS: Record<SearchSource['type'], LucideIcon> = {
  website: Globe,
  document: FileText,
  sharepoint: Share2,
  teams: MessageSquare,
  email: Mail,
};

const TYPE_LABELS: Record<SearchSource['type'], string> = {
  website: 'Nettside',
  document: 'Dokument',
  sharepoint: 'SharePoint',
  teams: 'Teams',
  email: 'E-post',
};

interface CitationCardProps {
  source: SearchSource;
  index: number;
}

export function CitationCard({ source, index }: CitationCardProps) {
  const Icon: LucideIcon = TYPE_ICONS[source.type] ?? Globe;
  const label = TYPE_LABELS[source.type] ?? source.type;

  const scorePercent = source.score !== undefined ? Math.round(source.score * 100) : null;

  return (
    <a
      href={source.url}
      target="_blank"
      rel="noopener noreferrer"
      className="group flex flex-col gap-2 border border-[#D8D2C6] bg-[#F4F1EB] p-4 transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
    >
      {/* Header */}
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
            <span className="shrink-0 flex h-5 w-5 items-center justify-center border border-[#D8D2C6] bg-white text-[#4A4A48]">
              <span className="font-inter text-[10px] font-medium text-[#A09890]">{index}</span>
            </span>
            <Icon size={13} strokeWidth={1.5} className="shrink-0 text-[#A09890]" />
            <span className="font-inter text-[10px] uppercase tracking-widest text-[#A09890]">{label}</span>
          </div>
          <div className="flex items-center gap-2 shrink-0">
          {scorePercent !== null && (
            <span className="font-inter text-[10px] text-[#C8C1B3]">{scorePercent}%</span>
          )}
          <ExternalLink size={11} strokeWidth={1.5} className="text-[#C8C1B3] transition-colors group-hover:text-[#2B2B2B]" />
        </div>
      </div>

      {/* Title */}
      <h4
        className="text-[15px] font-normal leading-snug text-[#2B2B2B] line-clamp-1"
        style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
      >
        {source.title}
      </h4>

      {/* Snippet */}
      <p className="font-inter text-[12px] leading-relaxed text-[#A09890] line-clamp-2">
        {source.snippet}
      </p>

      {/* URL strip */}
      <p className="font-inter text-[10px] text-[#C8C1B3] truncate">
        {source.url}
      </p>
    </a>
  );
}
