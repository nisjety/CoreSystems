export const plannerDocumentStateService = {
  async getState(workspaceId: string, documentId: string) {
    const response = await fetch(
      `/api/planner/documents/${encodeURIComponent(documentId)}/state?workspaceId=${encodeURIComponent(workspaceId)}`,
      {
        method: 'GET',
      }
    );

    if (response.status === 204) {
      return null;
    }

    if (!response.ok) {
      throw new Error(`Failed to load planner document state: ${response.status}`);
    }

    return response.arrayBuffer();
  },

  async saveState(workspaceId: string, documentId: string, state: ArrayBuffer) {
    const response = await fetch(
      `/api/planner/documents/${encodeURIComponent(documentId)}/state?workspaceId=${encodeURIComponent(workspaceId)}`,
      {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/octet-stream',
        },
        body: state,
      }
    );

    if (!response.ok) {
      throw new Error(`Failed to save planner document state: ${response.status}`);
    }

    return response.json() as Promise<{ updatedAt: number }>;
  },
};