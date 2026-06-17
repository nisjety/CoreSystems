export interface IntegrationConnectionSummary {
  id: string
  provider: string
  label: string
  selectedSources: string[]
  status: string
  linkStatus: string | null
  syncStatus: string | null
  syncError: string | null
  lastSyncedAt: string | null
  createdAt: string | null
  updatedAt: string | null
}

export interface IntegrationProviderLinkSummary {
  id: string
  provider: string
  status: string
  scopesGranted: string[]
  lastSignInAt: string | null
  lastLinkedAt: string | null
  createdAt: string | null
  updatedAt: string | null
}

export interface IntegrationProviderSummary {
  key: string
  label: string
  description: string
  supportedSources: string[]
  defaultSources: string[]
  categories?: string[]
  configured?: boolean
  authExecution?: string | null
  syncExecution?: string | null
  connected: boolean
  signInLinked: boolean
  dataAccessReady: boolean
  providerLink: IntegrationProviderLinkSummary | null
  connection: IntegrationConnectionSummary | null
}

export interface KnowledgeIntegrationsResponse {
  orgId: string
  userId: string
  totalConnected: number
  connections: IntegrationConnectionSummary[]
  providerLinks: IntegrationProviderLinkSummary[]
  providers: IntegrationProviderSummary[]
}

export interface KnowledgeDocumentSummary {
  id: string
  title: string
  source: string
  type: string
  status: string
  createdAt: string
  updatedAt: string
  sourceUrl: string | null
}

export interface KnowledgeDocumentsResponse {
  orgId: string
  total: number
  documents: KnowledgeDocumentSummary[]
}

export type KnowledgeSourceStatus = 'active' | 'error' | 'indexing' | 'pending'

export interface KnowledgeSourceSummary {
  id: string
  name: string
  url: string
  type: 'website'
  status: KnowledgeSourceStatus
  pageCount: number
  documentCount: number
  lastIndexed: string | null
}

export interface KnowledgeSourcesResponse {
  orgId: string
  total: number
  sources: KnowledgeSourceSummary[]
}
