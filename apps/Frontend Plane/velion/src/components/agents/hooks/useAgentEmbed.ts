'use client';

import { useCallback, useEffect, useState } from 'react';

import type { PersistedAgent } from '../types';

/**
 * Wave 9 (ui-ux-verevon-gap.md §19): per-agent embed enable / disable +
 * snippet builder for the public chat-bubble widget.
 *
 * Reads `publicEnabled` + `publicSecret` from the agent record (passed
 * in). Mutations go through `/api/agents/{id}/embed` which proxies to
 * Convex `agents:enablePublicEmbed` / `agents:disablePublicEmbed`.
 *
 * The hook deliberately does NOT cache the secret across renders —
 * rotating the secret is a deliberate action and the new value comes
 * straight from the mutation response.
 */

type EnableInput = {
  accentColor?: string;
  buttonLabel?: string;
  welcomeMessage?: string;
};

interface UseAgentEmbedReturn {
  publicEnabled: boolean;
  publicSecret: string | null;
  isWorking: boolean;
  error: string | null;
  enable: (theme?: EnableInput) => Promise<void>;
  disable: () => Promise<void>;
  rotateSecret: (theme?: EnableInput) => Promise<void>;
  embedSnippet: string;
}

export function useAgentEmbed(agent: PersistedAgent | null): UseAgentEmbedReturn {
  // Keep a local copy so the hook can mirror mutations without waiting
  // for the parent's Convex subscription to roundtrip.
  const [publicEnabled, setPublicEnabled] = useState<boolean>(
    Boolean(agent?.publicEnabled),
  );
  const [publicSecret, setPublicSecret] = useState<string | null>(
    agent?.publicSecret ?? null,
  );
  const [isWorking, setIsWorking] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setPublicEnabled(Boolean(agent?.publicEnabled));
    setPublicSecret(agent?.publicSecret ?? null);
  }, [agent?.id, agent?.publicEnabled, agent?.publicSecret]);

  const enable = useCallback(
    async (theme?: EnableInput): Promise<void> => {
      if (!agent) return;
      setIsWorking(true);
      setError(null);
      try {
        const res = await fetch(`/api/agents/${agent.id}/embed`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ theme }),
        });
        if (!res.ok) throw new Error(`enable failed (${res.status})`);
        const body = (await res.json()) as { publicSecret?: string };
        if (!body.publicSecret) throw new Error('missing secret in response');
        setPublicEnabled(true);
        setPublicSecret(body.publicSecret);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'enable failed');
      } finally {
        setIsWorking(false);
      }
    },
    [agent],
  );

  const disable = useCallback(async (): Promise<void> => {
    if (!agent) return;
    setIsWorking(true);
    setError(null);
    try {
      const res = await fetch(`/api/agents/${agent.id}/embed`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error(`disable failed (${res.status})`);
      setPublicEnabled(false);
      setPublicSecret(null);
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'disable failed');
    } finally {
      setIsWorking(false);
    }
  }, [agent]);

  // Rotate = enable again. The mutation generates a fresh secret on
  // every call, so the same path serves both "first enable" and
  // "rotate after a leak suspicion".
  const rotateSecret = enable;

  // Build the snippet the operator pastes into their site. We pick the
  // browser-visible origin via `window.location.origin` when running
  // client-side; falls back to a placeholder during SSR.
  const origin =
    typeof window !== 'undefined' ? window.location.origin : 'https://your-verevon-host';
  const embedSnippet =
    agent && publicEnabled && publicSecret
      ? [
          '<!-- Verevon agent embed widget -->',
          `<script`,
          `  src="${origin}/embed.js"`,
          `  data-verevon-agent="${agent.id}"`,
          `  data-verevon-secret="${publicSecret}"`,
          `  defer`,
          `></script>`,
        ].join('\n')
      : '';

  return {
    publicEnabled,
    publicSecret,
    isWorking,
    error,
    enable,
    disable,
    rotateSecret,
    embedSnippet,
  };
}
