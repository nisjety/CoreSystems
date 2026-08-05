'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';

import type { PersistedAgent } from '../types';

/**
 * U3-8 (ui-ux-verevon-gap.md §14): unified tool catalog for the agent
 * workspace's "Tools" tab.
 *
 * The catalog merges two sources:
 *   1. **Skills** registered in capability-core (`/api/skills`, populated
 *      from the `agent_skills` Postgres table per U2-14). These are the
 *      org-defined skills the operator wrote — e.g. "Code reviewer",
 *      "Document summarizer".
 *   2. **Built-in tools** the Model Plane gateway exposes natively
 *      (Browse Web, Deep Research, Fetch URL, Image generation). These
 *      are gateway-side capabilities that any agent can opt into without
 *      a corresponding capability-core row.
 *
 * Built-ins are intentionally hardcoded here (and only here) because
 * they map 1:1 to gateway HTTP routes — the gateway is the source of
 * truth for what's wired. When a new gateway route ships, add an entry
 * with the same `id` the agent's `tools` array uses.
 *
 * The hook returns:
 *   - `tools` — merged catalog with `enabled: boolean` reflecting the
 *     agent's current `tools` list
 *   - `toggle(id)` — flips a tool on/off in local state
 *   - `save()` — persists the resulting list via `PATCH /api/agents/{id}`
 *   - `dirty` — true when local state diverges from the saved list
 *   - `saveStatus` — idle/saving/saved/error for UI feedback
 */

export type AgentToolKind = 'builtin' | 'skill';

export interface AgentTool {
  id: string;
  name: string;
  description: string;
  kind: AgentToolKind;
  enabled: boolean;
}

interface SkillsResponse {
  skills?: Array<{ id: string; name: string; description: string }>;
}

const BUILTIN_TOOLS: ReadonlyArray<Omit<AgentTool, 'enabled'>> = [
  {
    id: 'browse_web',
    name: 'Browse Web',
    description:
      'Run a Brave search before answering and ground the reply in live results. Backed by Model Plane /v1/ai/web/search.',
    kind: 'builtin',
  },
  {
    id: 'deep_research',
    name: 'Deep Research',
    description:
      'Run a multi-step research loop (plan → fetch → synthesize) via /v1/research. Best for questions that need 5-15 sources stitched together.',
    kind: 'builtin',
  },
  {
    id: 'fetch_url',
    name: 'Fetch URL',
    description:
      'Pull and parse a single URL into the prompt. Backed by Model Plane /v1/ai/web/fetch (Quarry + reqwest fallback).',
    kind: 'builtin',
  },
  {
    id: 'image_generate',
    name: 'Image generation',
    description:
      'Generate images from text via /v1/ai/images (Azure gpt-image-1). The agent can call this as a tool inside the conversation.',
    kind: 'builtin',
  },
];

type SaveStatus = 'idle' | 'saving' | 'saved' | 'error';

interface UseAgentToolsReturn {
  tools: AgentTool[];
  toggle: (id: string) => void;
  save: () => Promise<void>;
  reload: () => Promise<void>;
  dirty: boolean;
  saveStatus: SaveStatus;
  isLoading: boolean;
  unavailable: boolean;
}

export function useAgentTools(agent: PersistedAgent | null): UseAgentToolsReturn {
  const [enabledIds, setEnabledIds] = useState<Set<string>>(
    () => new Set(agent?.tools ?? []),
  );
  const [skillCatalog, setSkillCatalog] = useState<AgentTool[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(true);
  const [unavailable, setUnavailable] = useState<boolean>(false);
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle');
  const [savedSnapshot, setSavedSnapshot] = useState<Set<string>>(
    () => new Set(agent?.tools ?? []),
  );

  // Reset local state when the agent prop changes.
  useEffect(() => {
    const next = new Set(agent?.tools ?? []);
    setEnabledIds(next);
    setSavedSnapshot(next);
  }, [agent?.id, agent?.tools]);

  const reload = useCallback(async (): Promise<void> => {
    try {
      const res = await fetch('/api/skills', { cache: 'no-store' });
      if (!res.ok) {
        setUnavailable(true);
        setSkillCatalog([]);
        return;
      }
      const payload = (await res.json()) as SkillsResponse;
      const skills: AgentTool[] = (payload.skills ?? []).map((s) => ({
        id: `skill:${s.id}`,
        name: s.name,
        description: s.description,
        kind: 'skill' as const,
        enabled: false,
      }));
      setSkillCatalog(skills);
      setUnavailable(false);
    } catch {
      setUnavailable(true);
      setSkillCatalog([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const tools = useMemo<AgentTool[]>(() => {
    const merged: AgentTool[] = [
      ...BUILTIN_TOOLS.map((t) => ({ ...t, enabled: enabledIds.has(t.id) })),
      ...skillCatalog.map((s) => ({ ...s, enabled: enabledIds.has(s.id) })),
    ];
    return merged;
  }, [enabledIds, skillCatalog]);

  const dirty = useMemo<boolean>(() => {
    if (enabledIds.size !== savedSnapshot.size) return true;
    for (const id of enabledIds) {
      if (!savedSnapshot.has(id)) return true;
    }
    return false;
  }, [enabledIds, savedSnapshot]);

  const toggle = useCallback((id: string): void => {
    setEnabledIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const save = useCallback(async (): Promise<void> => {
    if (!agent) return;
    setSaveStatus('saving');
    try {
      const res = await fetch(`/api/agents/${agent.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tools: Array.from(enabledIds) }),
      });
      if (!res.ok) {
        throw new Error(`save failed (${res.status})`);
      }
      setSavedSnapshot(new Set(enabledIds));
      setSaveStatus('saved');
      setTimeout(() => setSaveStatus('idle'), 2000);
    } catch {
      setSaveStatus('error');
      setTimeout(() => setSaveStatus('idle'), 3000);
    }
  }, [agent, enabledIds]);

  return {
    tools,
    toggle,
    save,
    reload,
    dirty,
    saveStatus,
    isLoading,
    unavailable,
  };
}
