'use client';

import { startTransition, useCallback, useEffect, useRef } from 'react';
import { Loader2, Plus } from 'lucide-react';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { AFFINE_DEFAULT_WORKSPACE_ID } from '@/lib/affine/config';
import { PlannerEditor } from './PlannerEditor';
import { PlannerLoadingState } from './PlannerLoadingState';
import { PlannerSidebar } from './PlannerSidebar';
import { PlannerToolbar } from './PlannerToolbar';
import { usePlannerDocuments } from './hooks/usePlannerDocuments';
import { PlannerProvider, usePlanner } from './providers/PlannerProvider';
import type { PlannerDocumentSpace, PlannerDocumentUpdateInput } from './services/planner-document-service';

function PlannerCanvas({ selectedDocId, selectedDocTitle }: { selectedDocId: string; selectedDocTitle: string }) {
  const { activeDocId, setActiveDocId, setDocumentTitle } = usePlanner();
  const previousDocIdRef = useRef(selectedDocId);

  useEffect(() => {
    if (selectedDocId !== activeDocId) {
      setActiveDocId(selectedDocId);
    }
  }, [activeDocId, selectedDocId, setActiveDocId]);

  useEffect(() => {
    if (previousDocIdRef.current === selectedDocId) {
      return;
    }

    previousDocIdRef.current = selectedDocId;
    setDocumentTitle(selectedDocTitle);
  }, [selectedDocId, selectedDocTitle, setDocumentTitle]);

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-hidden rounded-[32px] border border-stone-200/80 bg-[rgba(252,249,244,0.97)] shadow-[0_28px_90px_rgba(110,90,60,0.10)]">
      <PlannerToolbar />
      <PlannerLoadingState />
      <PlannerEditor />
    </div>
  );
}

export function PlannerWorkspaceClient() {
  const workspaceId = AFFINE_DEFAULT_WORKSPACE_ID;
  const seededInitialDocRef = useRef(false);
  const pathname = usePathname();
  const router = useRouter();
  const searchParams = useSearchParams();
  const {
    documents,
    isLoading,
    isFetching,
    createDocument,
    isCreating,
    updateDocument,
    archiveDocument,
    restoreDocument,
  } = usePlannerDocuments(workspaceId);

  const activeDocuments = documents.filter((document) => !document.archivedAt);

  const requestedDocId = searchParams.get('doc');
  const selectedDocId = activeDocuments.some((document) => document.id === requestedDocId)
    ? requestedDocId
    : activeDocuments[0]?.id ?? null;

  const selectedDocument = activeDocuments.find((document) => document.id === selectedDocId) ?? null;

  const buildPlannerHref = useCallback(
    (documentId: string | null) => {
      const nextParams = new URLSearchParams(searchParams.toString());

      if (documentId) {
        nextParams.set('doc', documentId);
      } else {
        nextParams.delete('doc');
      }

      const query = nextParams.toString();
      return query.length > 0 ? `${pathname}?${query}` : pathname;
    },
    [pathname, searchParams]
  );

  const navigateToDocument = useCallback(
    (documentId: string | null, history: 'push' | 'replace' = 'push') => {
      const href = buildPlannerHref(documentId);

      startTransition(() => {
        if (history === 'replace') {
          router.replace(href, { scroll: false });
          return;
        }

        router.push(href, { scroll: false });
      });
    },
    [buildPlannerHref, router]
  );

  useEffect(() => {
    if (activeDocuments.length === 0) {
      if (requestedDocId) {
        navigateToDocument(null, 'replace');
      }
      return;
    }

    if (requestedDocId !== selectedDocId) {
      navigateToDocument(selectedDocId, 'replace');
    }
  }, [activeDocuments.length, navigateToDocument, requestedDocId, selectedDocId]);

  useEffect(() => {
    if (seededInitialDocRef.current || isLoading || activeDocuments.length > 0 || isCreating) {
      return;
    }

    seededInitialDocRef.current = true;
    void createDocument({ title: 'Field note', space: 'private' }).then((document) => {
      navigateToDocument(document.id, 'replace');
    });
  }, [activeDocuments.length, createDocument, isCreating, isLoading, navigateToDocument]);

  useEffect(() => {
    if (!selectedDocId) {
      return;
    }

    void updateDocument({
      documentId: selectedDocId,
      patch: { lastViewedAt: Date.now() },
    });
  }, [selectedDocId, updateDocument]);

  async function handleCreateDocument(input?: {
    parentDocumentId?: string | null;
    space?: PlannerDocumentSpace;
  }) {
    const document = await createDocument({
      title: `Untitled ${activeDocuments.length + 1}`,
      parentDocumentId: input?.parentDocumentId,
      space: input?.space ?? 'private',
    });
    navigateToDocument(document.id);
    return document;
  }

  async function handleArchiveDocument(documentId: string) {
    await archiveDocument(documentId);

    if (selectedDocId === documentId) {
      const nextDocument = activeDocuments.find((document) => document.id !== documentId);
      navigateToDocument(nextDocument?.id ?? null, 'replace');
    }
  }

  async function handleRestoreDocument(documentId: string) {
    const document = await restoreDocument(documentId);

    if (!selectedDocId && document) {
      navigateToDocument(document.id, 'replace');
    }

    return document;
  }

  async function handleUpdateDocument(documentId: string, patch: PlannerDocumentUpdateInput) {
    return updateDocument({ documentId, patch });
  }

  const showCanvas = Boolean(selectedDocId);

  return (
    <div className="flex h-full min-h-0 bg-[radial-gradient(circle_at_top,rgba(245,231,214,0.9),rgba(239,236,229,0.45)_40%,rgba(233,231,226,0.8)_100%)] p-4 text-stone-900 md:p-6">
      <div className="flex h-full min-h-0 w-full gap-4 lg:gap-6">
        <PlannerSidebar
          documents={documents}
          activeDocId={selectedDocId}
          isLoading={isLoading}
          isBusy={isFetching || isCreating}
          onSelect={(documentId) => navigateToDocument(documentId)}
          onCreate={(input) => handleCreateDocument(input)}
          onUpdateDocument={handleUpdateDocument}
          onArchive={(documentId) => handleArchiveDocument(documentId)}
          onRestore={(documentId) => handleRestoreDocument(documentId)}
        />

        <div className="flex min-h-0 flex-1 flex-col">
          {showCanvas ? (
            <PlannerProvider
              key={selectedDocId}
              workspaceId={workspaceId}
              initialDocId={selectedDocId!}
              initialDocTitle={selectedDocument?.title ?? 'Untitled note'}
            >
              <PlannerCanvas
                selectedDocId={selectedDocId!}
                selectedDocTitle={selectedDocument?.title ?? 'Untitled note'}
              />
            </PlannerProvider>
          ) : (
            <div className="flex h-full min-h-0 flex-1 items-center justify-center rounded-[32px] border border-dashed border-stone-300 bg-[rgba(252,249,244,0.88)] px-8 text-center shadow-[0_28px_90px_rgba(110,90,60,0.08)]">
              {isLoading || isCreating ? (
                <div className="flex flex-col items-center gap-4 text-stone-500">
                  <Loader2 className="h-7 w-7 animate-spin" />
                  <p className="text-sm">Preparing your first planner surface…</p>
                </div>
              ) : (
                <div className="max-w-md">
                  <p className="text-xs uppercase tracking-[0.3em] text-stone-500">Planner</p>
                  <h3 className="mt-3 text-3xl font-semibold tracking-tight text-stone-900">
                    Start with a clean document shell.
                  </h3>
                  <p className="mt-4 text-sm leading-6 text-stone-500">
                    Metadata and document state are owned by this app, with BlockSuite running in
                    the frontend, live Yjs sync over the planner transport, and snapshots persisted
                    through the planner API.
                  </p>
                  <button
                    type="button"
                    onClick={() => void handleCreateDocument()}
                    className="mt-6 inline-flex items-center gap-2 rounded-full bg-stone-900 px-5 py-3 text-sm font-medium text-stone-50 transition hover:bg-stone-700"
                  >
                    <Plus className="h-4 w-4" />
                    Create first note
                  </button>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}