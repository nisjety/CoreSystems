'use client';

import { Globe, FileText, Share2, MessageSquare, Mail, CheckCircle2, AlertCircle, Clock, type LucideIcon } from 'lucide-react';

export type SourceStatus = 'active' | 'error' | 'indexing' | 'pending';

export interface KnowledgeSource {
  id: string;
  name: string;
  type: 'website' | 'document' | 'sharepoint' | 'teams' | 'email';
  url?: string;
  status: SourceStatus;
  pageCount?: number;
  lastIndexed?: string;
}

const TYPE_ICONS: Record<KnowledgeSource['type'], LucideIcon> = {
  website: Globe,
  document: FileText,
  sharepoint: Share2,
  teams: MessageSquare,
  email: Mail,
};

const STATUS_CONFIG: Record<SourceStatus, { icon: LucideIcon; label: string; color: string }> = {
  active:   { icon: CheckCircle2, label: 'Aktiv',       color: '#4A9B6F' },
  error:    { icon: AlertCircle,  label: 'Feil',        color: '#FF2E63' },
  indexing: { icon: Clock,        label: 'Indekserer',  color: '#C8A87A' },
  pending:  { icon: Clock,        label: 'Venter',      color: '#C8C1B3' },
};

interface SourcesListProps {
  sources: KnowledgeSource[];
  onAdd?: () => void;
}

export function SourcesList({ sources, onAdd }: SourcesListProps) {
  return (
    <div>
      {/* Header row */}
      <div className="mb-4 flex items-center justify-between">
        <p className="font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
          Kilder — {sources.length}
        </p>
        {onAdd && (
          <button
            onClick={onAdd}
            className="border border-[#D8D2C6] px-4 py-1.5 font-inter text-[11px] uppercase tracking-widest text-[#4A4A48] transition-colors hover:border-[#2B2B2B] hover:bg-[#EAE6DF]"
          >
            + Legg til
          </button>
        )}
      </div>

      {sources.length === 0 ? (
        <div className="border border-dashed border-[#D8D2C6] py-16 text-center">
          <p
            className="mb-2 text-[20px] font-normal text-[#2B2B2B]"
            style={{ fontFamily: 'var(--font-cormorant-garamond), Georgia, serif' }}
          >
            Ingen kilder enda
          </p>
          <p className="font-inter text-[13px] text-[#A09890]">
            Start en Quarry-crawl for selskapets nettside for å fylle denne listen.
          </p>
        </div>
      ) : (
        <div className="border border-[#D8D2C6]">
          {sources.map((src, i) => {
            const TypeIcon = TYPE_ICONS[src.type] ?? Globe;
            const { icon: StatusIcon, label: statusLabel, color } = STATUS_CONFIG[src.status];
            return (
              <div
                key={src.id}
                className={`flex items-center gap-4 px-5 py-4 ${i > 0 ? 'border-t border-[#D8D2C6]' : ''}`}
              >
                <TypeIcon size={15} strokeWidth={1.5} className="shrink-0 text-[#A09890]" />

                <div className="flex-1 min-w-0">
                  <p className="font-inter text-[13px] font-medium text-[#2B2B2B] truncate">{src.name}</p>
                  {src.url && (
                    <p className="font-inter text-[11px] text-[#C8C1B3] truncate">{src.url}</p>
                  )}
                </div>

                {src.pageCount !== undefined && (
                  <p className="shrink-0 font-inter text-[11px] text-[#A09890]">
                    {src.pageCount.toLocaleString('nb-NO')} sider
                  </p>
                )}

                {src.lastIndexed && (
                  <p className="shrink-0 hidden font-inter text-[11px] text-[#C8C1B3] sm:block">
                    {new Date(src.lastIndexed).toLocaleDateString('nb-NO')}
                  </p>
                )}

                <div className="shrink-0 flex items-center gap-1.5" style={{ color }}>
                  <StatusIcon size={12} strokeWidth={1.5} />
                  <span className="font-inter text-[11px]">{statusLabel}</span>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
