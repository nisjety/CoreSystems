'use client';

import { useState, useCallback } from 'react';
import { Search, Loader2, X } from 'lucide-react';
import { noteServerService, SemanticSearchResult } from '@/components/notes/services/note-server-service';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';

interface SemanticSearchProps {
  onResultClick?: (result: SemanticSearchResult) => void;
  className?: string;
}

export function SemanticSearch({ onResultClick, className }: SemanticSearchProps) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SemanticSearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSearch = useCallback(async () => {
    if (!query.trim()) return;

    setIsSearching(true);
    setError(null);

    try {
      const searchResults = await noteServerService.semanticSearch(query.trim(), 10);
      setResults(searchResults);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Search failed');
      setResults([]);
    } finally {
      setIsSearching(false);
    }
  }, [query]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSearch();
    }
  };

  const clearSearch = () => {
    setQuery('');
    setResults([]);
    setError(null);
  };

  return (
    <div className={className}>
      {/* Search Input */}
      <div className="relative mb-4">
        <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
        <Input
          type="text"
          placeholder="Search across all transcripts..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKeyDown}
          className="pl-10 pr-20"
        />
        <div className="absolute right-2 top-1/2 flex -translate-y-1/2 gap-1">
          {query && (
            <Button
              size="sm"
              variant="ghost"
              onClick={clearSearch}
              className="h-7 w-7 p-0"
            >
              <X className="h-4 w-4" />
            </Button>
          )}
          <Button
            size="sm"
            onClick={handleSearch}
            disabled={isSearching || !query.trim()}
            className="h-7"
          >
            {isSearching ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              'Search'
            )}
          </Button>
        </div>
      </div>

      {/* Error Message */}
      {error && (
        <div className="mb-4 rounded-lg bg-red-50 p-3 text-sm text-red-600">
          {error}
        </div>
      )}

      {/* Search Results */}
      {results.length > 0 && (
        <div className="space-y-2">
          <p className="text-sm text-slate-600">
            Found {results.length} result{results.length !== 1 ? 's' : ''}
          </p>
          <div className="max-h-96 space-y-2 overflow-y-auto">
            {results.map((result) => (
              <Card
                key={result.id}
                className="cursor-pointer p-4 transition-colors hover:bg-slate-50"
                onClick={() => onResultClick?.(result)}
              >
                <div className="mb-2 flex items-start justify-between">
                  <span className="text-xs font-medium text-slate-500">
                    Meeting ID: {result.meeting_id || 'Unknown'}
                  </span>
                  <span className="text-xs text-slate-400">
                    Score: {(result.score * 100).toFixed(1)}%
                  </span>
                </div>
                <div
                  className="text-sm text-slate-700"
                  dangerouslySetInnerHTML={{ __html: result.snippet }}
                />
              </Card>
            ))}
          </div>
        </div>
      )}

      {/* Empty State */}
      {!isSearching && !error && results.length === 0 && query && (
        <div className="flex flex-col items-center justify-center py-8 text-center">
          <Search className="mb-2 h-12 w-12 text-slate-300" />
          <p className="text-sm text-slate-500">
            No results found. Try a different search query.
          </p>
        </div>
      )}
    </div>
  );
}
