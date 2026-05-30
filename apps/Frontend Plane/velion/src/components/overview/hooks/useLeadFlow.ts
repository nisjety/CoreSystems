'use client';

import { useEffect, useState } from 'react';
import type { LeadOpportunity } from '../lib/overview-types';
import { mockLeads } from '../lib/mock-data';

interface UseLeadFlowReturn {
  leads: LeadOpportunity[] | null;
  isLoading: boolean;
  error: Error | null;
}

export function useLeadFlow(_userId: string): UseLeadFlowReturn {
  const [leads, setLeads] = useState<LeadOpportunity[] | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);

  useEffect(() => {
    const timer = setTimeout(() => {
      try {
        setLeads(mockLeads);
        setIsLoading(false);
      } catch (err) {
        setError(err instanceof Error ? err : new Error('Failed to fetch leads'));
        setIsLoading(false);
      }
    }, 500);

    return () => clearTimeout(timer);
  }, [_userId]);

  return { leads, isLoading, error };
}
