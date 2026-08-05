'use client';

import { useState, useEffect, useCallback } from 'react';
import type { Agent } from '../types';
import type { AgentWithMeta } from '../data';

// Wave 8 (ui-ux-verevon-gap.md §18): no MOCK_AGENTS fallback.
// Previously a failing `/api/agents` call silently swapped in the demo
// fixtures, which made the UI look healthy while the backend was
// broken — a classic "looks-like-it-works-but-doesn't" gap. Now we
// surface the failure as an explicit empty-state + `isError: true`
// so the caller can render an honest retry affordance.

export type UseAgentsResult = {
  data: (Agent | AgentWithMeta)[];
  isLoading: boolean;
  isError: boolean;
  refetch: () => void;
};

interface RawAgent {
  id: string;
  name: string;
  description?: string;
  useCase: Agent['useCase'];
  status: Agent['status'];
  model: string;
  createdAt: number;
  tools?: string[];
}

function mapToAgent(raw: RawAgent): Agent {
  return {
    id: raw.id,
    name: raw.name,
    description: raw.description,
    useCase: raw.useCase,
    status: raw.status,
    model: raw.model,
    createdAt: new Date(raw.createdAt).toISOString(),
    tools: raw.tools ?? [],
  };
}

export function useAgents(): UseAgentsResult {
  const [data, setData] = useState<(Agent | AgentWithMeta)[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isError, setIsError] = useState(false);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setIsLoading(true);
      setIsError(false);
      try {
        const res = await fetch('/api/agents');
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const json: unknown = await res.json();
        if (cancelled) return;
        // /api/agents wraps the list in `{success, data}` (since Wave 8).
        // Accept both shapes so existing callers (and a one-off bare-array
        // response from the seed endpoint) keep working without surprises.
        const raw = Array.isArray(json)
          ? (json as RawAgent[])
          : Array.isArray((json as { data?: unknown }).data)
            ? ((json as { data: RawAgent[] }).data)
            : [];
        setData(raw.map(mapToAgent));
      } catch {
        if (cancelled) return;
        // No silent mock fallback (Wave 8). Surface the failure so the
        // UI renders its real empty-state + retry, not fixtures that
        // disguise a broken backend.
        setData([]);
        setIsError(true);
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [version]);

  const refetch = useCallback(() => setVersion((v) => v + 1), []);

  return { data, isLoading, isError, refetch };
}
