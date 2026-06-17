'use client';

import { useCallback, useState } from 'react';

import type { KnowledgeSource, PersistedAgent } from '../types';

/**
 * U3-7 (ui-ux-velion-gap.md §14): per-agent knowledge sources.
 *
 * Agents own a `knowledgeSources` array on their Convex record. Each
 * entry is a `{ type, name, id? }` reference — files uploaded into the
 * Data Plane (`documents-api`), URLs to crawl, or pointers to existing
 * knowledge indexes.
 *
 * This hook handles two operations:
 *   1. **Upload a file** — POSTs to `/api/chat/upload` (the existing
 *      proxy that talks to `documents-api:8010/v1/documents`), then
 *      appends a `{type: 'file', name, id}` entry to the agent's list
 *      via `PATCH /api/agents/{id}`.
 *   2. **Add a URL** — appends a `{type: 'url', name: url}` entry
 *      (without scraping it; the gateway's browse tool handles fetch
 *      at invoke-time when the URL is referenced).
 *
 * Retrieval (RAG over these sources during a /v1/invoke call) is wired
 * separately at the gateway layer; this hook owns the CRUD-on-the-agent
 * side only. That's intentional: the "what does this agent know about"
 * answer needs to be persisted independently of any one query.
 */

/** Re-export so consumers can import the type from this hook directly. */
export type { KnowledgeSource };

export type KnowledgeSourceType = 'file' | 'url' | 'integration';

type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

interface UseAgentKnowledgeReturn {
  sources: KnowledgeSource[];
  uploadFile: (file: File) => Promise<void>;
  addUrl: (url: string) => Promise<void>;
  remove: (index: number) => Promise<void>;
  status: SaveStatus;
  error: string | null;
}

export function useAgentKnowledge(
  agent: PersistedAgent | null,
): UseAgentKnowledgeReturn {
  const [sources, setSources] = useState<KnowledgeSource[]>(
    () => agent?.knowledgeSources ?? [],
  );
  const [status, setStatus] = useState<SaveStatus>('idle');
  const [error, setError] = useState<string | null>(null);

  const persist = useCallback(
    async (next: KnowledgeSource[]): Promise<void> => {
      if (!agent) return;
      setStatus('saving');
      setError(null);
      try {
        const res = await fetch(`/api/agents/${agent.id}`, {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ knowledgeSources: next }),
        });
        if (!res.ok) {
          throw new Error(`save failed (${res.status})`);
        }
        setSources(next);
        setStatus('saved');
        setTimeout(() => setStatus('idle'), 2000);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'unknown error');
        setStatus('error');
        setTimeout(() => setStatus('idle'), 3000);
      }
    },
    [agent],
  );

  const uploadFile = useCallback(
    async (file: File): Promise<void> => {
      if (!agent) return;
      setStatus('saving');
      setError(null);
      try {
        const fd = new FormData();
        fd.append('file', file);
        // Tag the upload so documents-api can scope retrieval per-agent.
        fd.append('agentId', agent.id);
        const res = await fetch('/api/chat/upload', {
          method: 'POST',
          body: fd,
        });
        if (!res.ok) {
          const detail = await res.text().catch(() => '');
          throw new Error(`upload failed (${res.status}): ${detail.slice(0, 120)}`);
        }
        const payload = (await res.json()) as { id?: string; documentId?: string };
        const docId = payload.id ?? payload.documentId ?? '';
        const next: KnowledgeSource[] = [
          ...sources,
          { type: 'file', name: file.name, id: docId || undefined },
        ];
        await persist(next);
      } catch (e: unknown) {
        setError(e instanceof Error ? e.message : 'upload failed');
        setStatus('error');
        setTimeout(() => setStatus('idle'), 3000);
      }
    },
    [agent, sources, persist],
  );

  const addUrl = useCallback(
    async (url: string): Promise<void> => {
      const trimmed = url.trim();
      if (!agent || !trimmed) return;
      try {
        // Validate URL shape before persisting — invalid URLs would
        // silently fail at fetch-time during a conversation and be hard
        // to debug from the UI.
        new URL(trimmed);
      } catch {
        setError('Invalid URL');
        setStatus('error');
        setTimeout(() => setStatus('idle'), 3000);
        return;
      }
      const next: KnowledgeSource[] = [
        ...sources,
        { type: 'url', name: trimmed },
      ];
      await persist(next);
    },
    [agent, sources, persist],
  );

  const remove = useCallback(
    async (index: number): Promise<void> => {
      if (!agent || index < 0 || index >= sources.length) return;
      const next = sources.filter((_, i) => i !== index);
      await persist(next);
    },
    [agent, sources, persist],
  );

  return { sources, uploadFile, addUrl, remove, status, error };
}
