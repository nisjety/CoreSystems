import { apiClient } from '@/lib/api-client';

export type PlannerDocumentSpace = 'private' | 'shared' | 'collection';

export interface PlannerDocumentUpdateInput {
  title?: string;
  parentDocumentId?: string | null;
  isFavorite?: boolean;
  lastViewedAt?: number;
  restore?: boolean;
  space?: PlannerDocumentSpace;
}

export interface PlannerDocument {
  id: string;
  title: string;
  workspaceId: string;
  ownerExternalAuthId?: string;
  parentDocumentId?: string;
  isFavorite: boolean;
  lastViewedAt: number;
  space: PlannerDocumentSpace;
  createdAt: number;
  updatedAt: number;
  archivedAt?: number;
}

interface PlannerDocumentResponse {
  _id: string;
  workspaceId: string;
  documentId: string;
  title: string;
  ownerExternalAuthId?: string;
  parentDocumentId?: string;
  isFavorite?: boolean;
  lastViewedAt?: number;
  space?: PlannerDocumentSpace;
  createdAt: number;
  updatedAt: number;
  archivedAt?: number;
}

function mapPlannerDocument(document: PlannerDocumentResponse): PlannerDocument {
  return {
    id: document.documentId,
    title: document.title,
    workspaceId: document.workspaceId,
    ownerExternalAuthId: document.ownerExternalAuthId,
    parentDocumentId: document.parentDocumentId,
    isFavorite: document.isFavorite ?? false,
    lastViewedAt: document.lastViewedAt ?? document.updatedAt,
    space: document.space ?? 'private',
    createdAt: document.createdAt,
    updatedAt: document.updatedAt,
    archivedAt: document.archivedAt,
  };
}

export const plannerDocumentService = {
  async listDocuments(workspaceId: string, options?: { includeArchived?: boolean }) {
    const searchParams = new URLSearchParams({ workspaceId });
    if (options?.includeArchived) {
      searchParams.set('includeArchived', 'true');
    }

    const response = await apiClient.get<{ documents: PlannerDocumentResponse[] }>(
      `/api/planner/documents?${searchParams.toString()}`
    );
    return response.documents.map(mapPlannerDocument);
  },

  async createDocument(
    workspaceId: string,
    input: { title: string; parentDocumentId?: string | null; space?: PlannerDocumentSpace }
  ) {
    const response = await apiClient.post<{ document: PlannerDocumentResponse }>(
      '/api/planner/documents',
      {
        workspaceId,
        title: input.title,
        parentDocumentId: input.parentDocumentId,
        space: input.space,
      }
    );
    return mapPlannerDocument(response.document);
  },

  async updateDocument(
    workspaceId: string,
    documentId: string,
    input: PlannerDocumentUpdateInput
  ) {
    const response = await apiClient.patch<{ document: PlannerDocumentResponse }>(
      `/api/planner/documents/${encodeURIComponent(documentId)}`,
      {
        workspaceId,
        ...input,
      }
    );
    return mapPlannerDocument(response.document);
  },

  async renameDocument(workspaceId: string, documentId: string, title: string) {
    return this.updateDocument(workspaceId, documentId, { title });
  },

  async archiveDocument(workspaceId: string, documentId: string) {
    const response = await apiClient.delete<{ document: PlannerDocumentResponse | null }>(
      `/api/planner/documents/${encodeURIComponent(documentId)}?workspaceId=${encodeURIComponent(workspaceId)}`
    );

    return response.document ? mapPlannerDocument(response.document) : null;
  },

  async restoreDocument(workspaceId: string, documentId: string) {
    return this.updateDocument(workspaceId, documentId, { restore: true });
  },
};