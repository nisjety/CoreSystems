'use client';

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  plannerDocumentService,
  type PlannerDocument,
  type PlannerDocumentUpdateInput,
} from '../services/planner-document-service';

export const plannerDocumentKeys = {
  all: ['planner-documents'] as const,
  workspace: (workspaceId: string) => [...plannerDocumentKeys.all, workspaceId] as const,
};

export function usePlannerDocuments(workspaceId: string) {
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: plannerDocumentKeys.workspace(workspaceId),
    queryFn: () => plannerDocumentService.listDocuments(workspaceId, { includeArchived: true }),
    staleTime: 30_000,
    retry: 1,
    enabled: workspaceId.length > 0,
  });

  const createMutation = useMutation({
    mutationFn: (input: { title: string; parentDocumentId?: string | null; space?: 'private' | 'shared' | 'collection' }) =>
      plannerDocumentService.createDocument(workspaceId, input),
    onSuccess: (document) => {
      queryClient.setQueryData<PlannerDocument[]>(
        plannerDocumentKeys.workspace(workspaceId),
        (current = []) => [document, ...current]
      );
    },
  });

  const renameMutation = useMutation({
    mutationFn: ({ documentId, patch }: { documentId: string; patch: PlannerDocumentUpdateInput }) =>
      plannerDocumentService.updateDocument(workspaceId, documentId, patch),
    onSuccess: (updated) => {
      queryClient.setQueryData<PlannerDocument[]>(
        plannerDocumentKeys.workspace(workspaceId),
        (current = []) =>
          current
            .map((document) => (document.id === updated.id ? updated : document))
            .sort((left, right) => right.updatedAt - left.updatedAt)
      );
    },
  });

  const archiveMutation = useMutation({
    mutationFn: (documentId: string) =>
      plannerDocumentService.archiveDocument(workspaceId, documentId),
    onSuccess: (updated, documentId) => {
      queryClient.setQueryData<PlannerDocument[]>(
        plannerDocumentKeys.workspace(workspaceId),
        (current = []) =>
          updated
            ? current.map((document) => (document.id === documentId ? updated : document))
            : current
      );
    },
  });

  const restoreMutation = useMutation({
    mutationFn: (documentId: string) => plannerDocumentService.restoreDocument(workspaceId, documentId),
    onSuccess: (updated) => {
      queryClient.setQueryData<PlannerDocument[]>(
        plannerDocumentKeys.workspace(workspaceId),
        (current = []) =>
          current
            .map((document) => (document.id === updated.id ? updated : document))
            .sort((left, right) => right.updatedAt - left.updatedAt)
      );
    },
  });

  return {
    documents: query.data ?? [],
    isLoading: query.isLoading,
    isFetching: query.isFetching,
    error: query.error,
    refetch: query.refetch,
    createDocument: createMutation.mutateAsync,
    isCreating: createMutation.isPending,
    updateDocument: renameMutation.mutateAsync,
    renameDocument: ({ documentId, title }: { documentId: string; title: string }) =>
      renameMutation.mutateAsync({ documentId, patch: { title } }),
    isRenaming: renameMutation.isPending,
    archiveDocument: archiveMutation.mutateAsync,
    isArchiving: archiveMutation.isPending,
    restoreDocument: restoreMutation.mutateAsync,
    isRestoring: restoreMutation.isPending,
  };
}