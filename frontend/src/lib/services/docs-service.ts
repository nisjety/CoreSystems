import { apiClient } from '@/lib/api-client'

export interface Document {
    document_id: string
    content?: string
    org_id: string
    status: 'processing' | 'indexed' | 'failed'
}

export interface RetrievalRequest {
    org_id: string
    query: string
    top_k?: number
    document_types?: string[]
    departments?: string[]
}

export interface RetrievalResponse {
    facts: any[]
    sources: any[]
    query: string
    org_id: string
}

class DocsServiceAPI {
    async ingestDocument(content: string, orgId: string): Promise<Document> {
        return apiClient.post<Document>('/api/docs/documents', {
            content,
            org_id: orgId,
        })
    }

    async getDocument(id: string): Promise<Document> {
        return apiClient.get<Document>(`/api/docs/documents/${id}`)
    }

    async retrieve(data: RetrievalRequest): Promise<RetrievalResponse> {
        return apiClient.post<RetrievalResponse>('/api/docs/retrieve', data)
    }
}

export const docsService = new DocsServiceAPI()
