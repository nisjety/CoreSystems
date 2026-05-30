'use client';

import { useEffect, useState } from 'react';
import type { ActivityEvent } from '../lib/overview-types';
import { mockActivityEvents } from '../lib/mock-data';

interface UseActivityStreamReturn {
  events: ActivityEvent[] | null;
  isLoading: boolean;
  error: Error | null;
}

export function useActivityStream(_userId: string): UseActivityStreamReturn {
  const [events, setEvents] = useState<ActivityEvent[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        setEvents(mockActivityEvents);
        setIsLoading(false);
      } catch (err) {
        setError(err instanceof Error ? err : new Error('Failed to fetch activity stream'));
        setIsLoading(false);
      }
    }, 500);

    return () => clearTimeout(timer);
  }, [_userId]);

  return { events, isLoading, error };
}