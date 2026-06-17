'use client';

import { useEffect, useState } from 'react';
import type { RecentItem } from '../lib/overview-types';
import { mockRecentItems } from '../lib/mock-data';

interface UseRecentItemsReturn {
  items: RecentItem[] | null;
  isLoading: boolean;
  error: Error | null;
}

export function useRecentItems(_userId: string): UseRecentItemsReturn {
  const [items, setItems] = useState<RecentItem[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        setItems(mockRecentItems);
        setIsLoading(false);
      } catch (err) {
        setError(err instanceof Error ? err : new Error('Failed to fetch recent items'));
        setIsLoading(false);
      }
    }, 450);

    return () => clearTimeout(timer);
  }, [_userId]);

  return { items, isLoading, error };
}
