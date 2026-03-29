'use client';

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';
import type { DocCollection } from '@blocksuite/store';
import { AFFINE_DEFAULT_WORKSPACE_ID } from '@/lib/affine/config';

// ── Types ──────────────────────────────────────────────────────────────────

export type PlannerSyncStatus = 'initialising' | 'persisted' | 'syncing' | 'synced' | 'error';

export type EditorMode = 'edgeless' | 'page';

export type PlannerPresenceScope = 'realtime' | 'local';

interface PlannerState {
  /** Current BlockSuite DocCollection (workspace) */
  collection: DocCollection | null;
  /** Active document ID */
  activeDocId: string;
  /** Active document title reflected in planner chrome */
  documentTitle: string;
  /** How the editor is being rendered */
  mode: EditorMode;
  /** Persistence status for the planner workspace */
  syncStatus: PlannerSyncStatus;
  /** Human-readable error if syncStatus === 'error' */
  syncError: string | null;
  /** Most recent successful persistence timestamp */
  lastSavedAt: number | null;
  /** Same-browser editors currently active for this document */
  liveEditors: number;
  /** How presence is being detected */
  presenceScope: PlannerPresenceScope;
}

interface PlannerActions {
  setMode: (mode: EditorMode) => void;
  setActiveDocId: (id: string) => void;
  setDocumentTitle: (title: string) => void;
  setSyncState: (status: PlannerSyncStatus, error?: string | null) => void;
  setLastSavedAt: (timestamp: number | null) => void;
  setLiveEditors: (count: number) => void;
  setPresenceScope: (scope: PlannerPresenceScope) => void;
  /** Reload / reconnect to the workspace */
  reinitialise: () => void;
}

type PlannerContext = PlannerState & PlannerActions;

// ── Context ────────────────────────────────────────────────────────────────

const Context = createContext<PlannerContext | null>(null);

export function usePlanner(): PlannerContext {
  const ctx = useContext(Context);
  if (!ctx) throw new Error('usePlanner must be used inside <PlannerProvider>');
  return ctx;
}

// ── Provider ───────────────────────────────────────────────────────────────

interface PlannerProviderProps {
  children: React.ReactNode;
  /** Override the default workspace ID (useful for multi-workspace setups) */
  workspaceId?: string;
  /** Override the initial active doc ID */
  initialDocId?: string;
  /** Initial title for the active document */
  initialDocTitle?: string;
}

export function PlannerProvider({
  children,
  workspaceId = AFFINE_DEFAULT_WORKSPACE_ID,
  initialDocId = 'planner-main',
  initialDocTitle = 'Untitled note',
}: PlannerProviderProps) {
  const [state, setState] = useState<PlannerState>({
    collection: null,
    activeDocId: initialDocId,
    documentTitle: initialDocTitle,
    mode: 'edgeless',
    syncStatus: 'initialising',
    syncError: null,
    lastSavedAt: null,
    liveEditors: 1,
    presenceScope: 'local',
  });

  // Track the workspace dispose function so we can clean up on unmount
  const disposeRef = useRef<(() => void) | null>(null);
  const previousDocIdRef = useRef(initialDocId);

  const init = useCallback(async () => {
    setState((s) => ({ ...s, syncStatus: 'initialising', syncError: null, collection: null }));

    try {
      // Dynamic import — BlockSuite must not run on the server
      const { getOrCreateWorkspace } = await import('@/lib/affine/workspace');
      const handle = await getOrCreateWorkspace(workspaceId);

      disposeRef.current = handle.dispose;

      setState((s) => ({
        ...s,
        collection: handle.collection,
        syncStatus: 'persisted',
        syncError: null,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error';
      setState((s) => ({ ...s, syncStatus: 'error', syncError: message }));
    }
  }, [workspaceId]);

  // Init on mount and when workspaceId changes
  useEffect(() => {
    init();
    return () => {
      disposeRef.current?.();
    };
  }, [init]);

  useEffect(() => {
    if (previousDocIdRef.current === initialDocId) {
      return;
    }

    previousDocIdRef.current = initialDocId;
    setState((s) => ({
      ...s,
      activeDocId: initialDocId,
      documentTitle: initialDocTitle,
      syncStatus: 'initialising',
      syncError: null,
      lastSavedAt: null,
      liveEditors: 1,
      presenceScope: 'local',
    }));
  }, [initialDocId, initialDocTitle]);

  const setMode = useCallback((mode: EditorMode) => {
    setState((s) => ({ ...s, mode }));
  }, []);

  const setActiveDocId = useCallback((id: string) => {
    setState((s) => ({ ...s, activeDocId: id }));
  }, []);

  const setDocumentTitle = useCallback((title: string) => {
    setState((s) => ({ ...s, documentTitle: title }));
  }, []);

  const setSyncState = useCallback((status: PlannerSyncStatus, error: string | null = null) => {
    setState((s) => ({ ...s, syncStatus: status, syncError: error }));
  }, []);

  const setLastSavedAt = useCallback((timestamp: number | null) => {
    setState((s) => ({ ...s, lastSavedAt: timestamp }));
  }, []);

  const setLiveEditors = useCallback((count: number) => {
    setState((s) => ({ ...s, liveEditors: Math.max(1, count) }));
  }, []);

  const setPresenceScope = useCallback((scope: PlannerPresenceScope) => {
    setState((s) => ({ ...s, presenceScope: scope }));
  }, []);

  const value: PlannerContext = {
    ...state,
    setMode,
    setActiveDocId,
    setDocumentTitle,
    setSyncState,
    setLastSavedAt,
    setLiveEditors,
    setPresenceScope,
    reinitialise: init,
  };

  return <Context.Provider value={value}>{children}</Context.Provider>;
}
