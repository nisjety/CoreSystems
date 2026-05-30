'use client';

import { useEffect, useState } from 'react';
import type { OverviewMetrics } from '../lib/overview-types';
import { mockMetrics } from '../lib/mock-data';

interface UseOverviewMetricsReturn {
  metrics: OverviewMetrics | null;
  isLoading: boolean;
  error: Error | null;
}

export function useOverviewMetrics(_userId: string): UseOverviewMetricsReturn {
  const [metrics, setMetrics] = useState<OverviewMetrics | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        setMetrics(mockMetrics);
        setIsLoading(false);
      } catch (err) {
        setError(err instanceof Error ? err : new Error('Failed to fetch metrics'));
        setIsLoading(false);
      }
    }, 400);

    return () => clearTimeout(timer);
  }, [_userId]);

  return { metrics, isLoading, error };
}
