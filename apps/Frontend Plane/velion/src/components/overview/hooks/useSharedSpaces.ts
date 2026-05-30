'use client';

import { useEffect, useState } from 'react';
import type { SharedSpace } from '../lib/overview-types';
import { mockSharedSpaces } from '../lib/mock-data';

interface UseSharedSpacesReturn {
  spaces: SharedSpace[] | null;
  isLoading: boolean;
  error: Error | null;
}

export function useSharedSpaces(_userId: string): UseSharedSpacesReturn {
  const [spaces, setSpaces] = useState<SharedSpace[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        setSpaces(mockSharedSpaces);
        setIsLoading(false);
      } catch (err) {
        setError(err instanceof Error ? err : new Error('Failed to fetch shared spaces'));
        setIsLoading(false);
      }
    }, 450);

    return () => clearTimeout(timer);
  }, [_userId]);

  return { spaces, isLoading, error };
}
