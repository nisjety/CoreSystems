'use client';

import { CitationCard } from './CitationCard';
import type { SearchSource } from '@/lib/api/search-api';

interface SearchResultsProps {
  sources: SearchSource[];
}

export function SearchResults({ sources }: SearchResultsProps) {
  if (sources.length === 0) return null;

  return (
    <div>
      {/* Section label */}
      <p className="mb-3 font-inter text-[11px] uppercase tracking-widest text-[#C8C1B3]">
        Kilder — {sources.length}
      </p>

      <div className="grid grid-cols-1 gap-px border border-[#D8D2C6] bg-[#D8D2C6] sm:grid-cols-2">
        {sources.map((src, i) => (
          <CitationCard key={src.id} source={src} index={i + 1} />
        ))}
      </div>
    </div>
  );
}
