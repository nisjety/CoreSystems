'use client';

/**
 * PlannerEditor
 *
 * Mounts the BlockSuite AffineEditorContainer Lit custom element inside React.
 * All BlockSuite / LitElement code is lazily imported inside a useEffect so
 * it never runs during SSR.
 *
 * Key requirements for BlockSuite 0.19.5:
 *  1. Call the BlockSuite effects registries once so editor-host, gfx-viewport,
 *     affine-editor-container, page-editor, and related elements exist
 *  2. Import the AFFiNE theme CSS from the package export
 *  3. Set doc and mode via Lit accessor assignment (not constructor args)
 *  4. Append the element to DOM so Lit can call connectedCallback()
 */

import { useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { AFFINE_DEFAULT_WORKSPACE_ID } from '@/lib/affine/config';
import {
  connectPlannerRealtime,
  countLiveEditors,
  type PlannerRealtimeConnection,
} from '@/lib/planner/realtime';
import { plannerDocumentKeys } from './hooks/usePlannerDocuments';
import { plannerDocumentService, type PlannerDocument } from './services/planner-document-service';
import { usePlanner } from './providers/PlannerProvider';

// Track whether effects() has been called (only needed once per page load)
let effectsRegistered = false;

const FALLBACK_DOCUMENT_TITLE = 'Untitled note';

type RootTitleModel = {
  title: {
    clear: () => void;
    insert: (content: string, index: number) => void;
    toString: () => string;
    yText: {
      observe: (handler: () => void) => void;
      unobserve: (handler: () => void) => void;
    };
  };
};

function normalizeDocumentTitle(title: string | null | undefined) {
  const normalized = title?.trim();
  return normalized && normalized.length > 0 ? normalized : FALLBACK_DOCUMENT_TITLE;
}

function getRootTitleModel(doc: { root?: unknown } | null) {
  const root = doc?.root as RootTitleModel | null | undefined;
  return root?.title ? root : null;
}

function syncRootTitle(root: RootTitleModel, title: string) {
  const normalizedTitle = normalizeDocumentTitle(title);

  if (root.title.toString() === normalizedTitle) {
    return normalizedTitle;
  }

  root.title.clear();
  root.title.insert(normalizedTitle, 0);
  return normalizedTitle;
}

export function PlannerEditor() {
  const {
    collection,
    activeDocId,
    documentTitle,
    mode,
    setDocumentTitle,
    setLastSavedAt,
    setLiveEditors,
    setPresenceScope,
    setSyncState,
  } = usePlanner();
  const containerRef = useRef<HTMLDivElement>(null);
  const editorRef = useRef<Element | null>(null);
  const queryClient = useQueryClient();
  const documentTitleRef = useRef(documentTitle);

  useEffect(() => {
    documentTitleRef.current = documentTitle;
  }, [documentTitle]);

  useEffect(() => {
    if (!collection) return;

    let cancelled = false;
    let saveTimer: ReturnType<typeof setTimeout> | null = null;
    let titleTimer: ReturnType<typeof setTimeout> | null = null;
    let realtimeConnection: PlannerRealtimeConnection | null = null;
    let cleanup: (() => void) | null = null;

    const syncDocumentState = async () => {
      try {
        const [
          { getOrCreateDoc },
          { plannerDocumentStateService },
          { applyUpdate, encodeStateAsUpdate },
        ] = await Promise.all([
          import('@/lib/affine/workspace'),
          import('./services/planner-document-state-service'),
          import('yjs'),
        ]);

        const doc = await getOrCreateDoc(collection, activeDocId);
        const workspaceId = (collection as { id?: string }).id ?? AFFINE_DEFAULT_WORKSPACE_ID;
        const remoteOrigin = 'planner-convex-remote';
        const root = getRootTitleModel(doc);
        let lastCommittedTitle = normalizeDocumentTitle(documentTitleRef.current);
        let realtimeConnected = false;
        let realtimeSynced = false;

        const reflectRealtimeState = (errorMessage: string | null = null) => {
          if (cancelled) {
            return;
          }

          if (realtimeConnected && realtimeSynced) {
            setPresenceScope('realtime');
            setSyncState('synced');
            return;
          }

          if (realtimeConnected) {
            setPresenceScope('realtime');
            setSyncState('syncing');
            return;
          }

          setPresenceScope('local');

          if (errorMessage) {
            setSyncState('error', errorMessage);
            return;
          }

          setSyncState('persisted');
        };

        const remoteState = await plannerDocumentStateService.getState(workspaceId, activeDocId);
        if (!cancelled && remoteState) {
          applyUpdate(doc.spaceDoc, new Uint8Array(remoteState), remoteOrigin);
        }

        if (root && !cancelled) {
          const seededTitle = syncRootTitle(root, documentTitleRef.current);
          doc.collection.setDocMeta(doc.id, { title: seededTitle });
          setDocumentTitle(seededTitle);
          lastCommittedTitle = seededTitle;
        }

        const persistState = () => {
          if (saveTimer) {
            clearTimeout(saveTimer);
          }

          setSyncState('syncing');

          saveTimer = setTimeout(async () => {
            try {
              const snapshot = encodeStateAsUpdate(doc.spaceDoc);
              const buffer = snapshot.slice().buffer;

              const response = await plannerDocumentStateService.saveState(
                workspaceId,
                activeDocId,
                buffer,
              );

              if (cancelled) {
                return;
              }

              setLastSavedAt(response.updatedAt);
              reflectRealtimeState();
            } catch (error) {
              if (cancelled) {
                return;
              }

              console.warn('[PlannerEditor] state persistence failed', error);
              setSyncState('error', 'Failed to persist planner state');
            }
          }, 900);
        };

        const persistTitle = (nextTitle: string) => {
          if (titleTimer) {
            clearTimeout(titleTimer);
          }

          titleTimer = setTimeout(async () => {
            if (cancelled || nextTitle === lastCommittedTitle) {
              return;
            }

            setSyncState('syncing');

            try {
              const updated = await plannerDocumentService.renameDocument(
                workspaceId,
                activeDocId,
                nextTitle,
              );

              if (cancelled) {
                return;
              }

              lastCommittedTitle = updated.title;
              setDocumentTitle(updated.title);
              setLastSavedAt(updated.updatedAt);
              reflectRealtimeState();

              queryClient.setQueryData<PlannerDocument[]>(
                plannerDocumentKeys.workspace(workspaceId),
                (current = []) =>
                  current
                    .map((document) =>
                      document.id === updated.id ? updated : document
                    )
                    .sort((left, right) => right.updatedAt - left.updatedAt)
              );
            } catch (error) {
              if (cancelled) {
                return;
              }

              console.warn('[PlannerEditor] title persistence failed', error);
              setSyncState('error', 'Failed to persist document title');
            }
          }, 700);
        };

        const handleTitleChanged = () => {
          if (!root || cancelled) {
            return;
          }

          const nextTitle = normalizeDocumentTitle(root.title.toString());

          doc.collection.setDocMeta(doc.id, { title: nextTitle });
          setDocumentTitle(nextTitle);
          persistTitle(nextTitle);
        };

        realtimeConnection = connectPlannerRealtime({
          documentId: activeDocId,
          workspaceId,
          yDoc: doc.spaceDoc,
        });

        const { provider } = realtimeConnection;

        const syncLiveEditors = () => {
          setLiveEditors(countLiveEditors(provider));
          setPresenceScope(provider.wsconnected ? 'realtime' : 'local');
        };

        const handleRealtimeStatus = ({ status }: { status: 'connected' | 'connecting' | 'disconnected' }) => {
          realtimeConnected = status === 'connected';
          if (status !== 'connected') {
            realtimeSynced = false;
          }

          if (status === 'disconnected') {
            reflectRealtimeState('Realtime collaboration unavailable. Working from saved state.');
          } else {
            reflectRealtimeState();
          }

          syncLiveEditors();
        };

        const handleRealtimeSync = (isSynced: boolean) => {
          realtimeSynced = isSynced;
          reflectRealtimeState();
        };

        const handleRealtimeError = () => {
          reflectRealtimeState('Realtime collaboration unavailable. Working from saved state.');
          syncLiveEditors();
        };

        const handleAwarenessChange = () => {
          syncLiveEditors();
        };

        const handleUpdate = (_update: Uint8Array, origin: unknown) => {
          if (origin === remoteOrigin) {
            return;
          }
          persistState();
        };

        doc.spaceDoc.on('update', handleUpdate);
        provider.on('status', handleRealtimeStatus);
        provider.on('sync', handleRealtimeSync);
        provider.on('connection-error', handleRealtimeError);
        provider.on('connection-close', handleRealtimeError);
        provider.awareness.on('change', handleAwarenessChange);

        if (root) {
          root.title.yText.observe(handleTitleChanged);
        }

        setLiveEditors(countLiveEditors(provider));
        provider.connect();

        if (!remoteState) {
          persistState();
        } else {
          reflectRealtimeState();
        }

        cleanup = () => {
          doc.spaceDoc.off('update', handleUpdate);
          provider.off('status', handleRealtimeStatus);
          provider.off('sync', handleRealtimeSync);
          provider.off('connection-error', handleRealtimeError);
          provider.off('connection-close', handleRealtimeError);
          provider.awareness.off('change', handleAwarenessChange);
          if (root) {
            root.title.yText.unobserve(handleTitleChanged);
          }
          if (saveTimer) {
            clearTimeout(saveTimer);
          }
          if (titleTimer) {
            clearTimeout(titleTimer);
          }
          realtimeConnection?.destroy();
        };
      } catch (error) {
        if (!cancelled) {
          console.warn('[PlannerEditor] state sync initialisation failed', error);
          setSyncState('error', 'Failed to initialise planner state');
        }
      }
    };

    void syncDocumentState();

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [
    activeDocId,
    collection,
    queryClient,
    setDocumentTitle,
    setLastSavedAt,
    setLiveEditors,
    setPresenceScope,
    setSyncState,
  ]);

  useEffect(() => {
    if (!collection || !containerRef.current) return;

    let cancelled = false;
    const container = containerRef.current;

    const mount = async () => {
      try {
        // ── 1. Register all BlockSuite custom elements (once) ──────────────
        if (!effectsRegistered) {
          const [{ effects: registerBlockEffects }, { effects: registerPresetEffects }] = await Promise.all([
            import('@blocksuite/blocks/effects'),
            import('@blocksuite/presets/effects'),
          ]);

          if (!customElements.get('editor-host')) {
            registerBlockEffects();
          }

          if (!customElements.get('affine-editor-container')) {
            registerPresetEffects();
          }

          effectsRegistered = true;
        }

        // ── 2. Import the AFFiNE base theme ───────────────────────────────
        // @toeverything/theme provides the CSS variables the editor relies on
        await import('@toeverything/theme/style.css');

        if (cancelled) return;

        // ── 3. Get / create the target document ───────────────────────────
        const { getOrCreateDoc } = await import('@/lib/affine/workspace');
        const doc = await getOrCreateDoc(collection, activeDocId);

        if (cancelled) return;

        // ── 4. Build and mount the editor element ─────────────────────────
        // AffineEditorContainer is a Lit ShadowlessElement registered as
        // 'affine-editor-container'. Create via document.createElement so
        // the custom element registry resolves the constructor correctly.
        const editor = document.createElement(
          'affine-editor-container'
        ) as HTMLElement & {
          doc: typeof doc;
          mode: 'page' | 'edgeless';
          autofocus: boolean;
        };

        editor.doc = doc;
        editor.mode = mode as 'page' | 'edgeless';
        editor.autofocus = true;
        editor.style.cssText = 'width:100%;height:100%;display:block;';

        // Tear down any previous editor before appending the new one
        if (editorRef.current && container.contains(editorRef.current)) {
          container.removeChild(editorRef.current);
        }
        container.appendChild(editor);
        editorRef.current = editor;
      } catch (err) {
        if (!cancelled) {
          console.error('[PlannerEditor] mount failed', err);
        }
      }
    };

    mount();

    return () => {
      cancelled = true;
    };
  }, [collection, activeDocId, mode]);

  // Separate cleanup effect — runs when component actually unmounts
  useEffect(() => {
    const container = containerRef.current;
    return () => {
      if (container && editorRef.current && container.contains(editorRef.current)) {
        container.removeChild(editorRef.current);
      }
      editorRef.current = null;
    };
  }, []);

  return (
    <div
      ref={containerRef}
      className="relative w-full flex-1 overflow-hidden"
      style={{ minHeight: 0 }}
      tabIndex={-1}
    />
  );
}
