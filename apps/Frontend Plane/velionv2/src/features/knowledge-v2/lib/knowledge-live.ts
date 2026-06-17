export type KnowledgeMetricTone = "good" | "warn";

export type LiveKnowledgeMetric = {
  label: string;
  value: string;
  delta: string;
  tone: KnowledgeMetricTone;
};

export type LiveKnowledgeCollection = {
  id: string;
  label: string;
  count: number;
};

export type LiveKnowledgeFolder = {
  id: string;
  title: string;
  subtitle: string;
  providerKey: string;
  primaryValue: string;
  primaryLabel: string;
  secondaryValue: string;
  secondaryLabel: string;
  connections: string[];
  tone: "warm" | "green" | "blue" | "gray";
};

export type LiveKnowledgeIntegration = {
  id: string;
  name: string;
  providerKey: string;
  status: "Connected" | "Syncing" | "Review";
  documents: string;
  freshness: string;
  detail?: string;
};

export type LiveKnowledgeChunk = {
  id: string;
  title: string;
  score: string;
  text: string;
};

export type LiveKnowledgeSourceType = "PDF" | "Notion" | "Docs" | "URL";
export type LiveKnowledgeSourceStatus = "Indexed" | "Pending review" | "Re-indexing";

export type LiveKnowledgeSource = {
  id: string;
  title: string;
  description: string;
  type: LiveKnowledgeSourceType;
  provider: string;
  providerKey: string;
  category: string;
  owner: string;
  updated: string;
  size: string;
  status: LiveKnowledgeSourceStatus;
  chunks: number;
  hitRate: string;
  coverage: string;
  similarity: string;
  tags: string[];
  related: string[];
  chunksPreview: LiveKnowledgeChunk[];
};

export type LiveKnowledgeFile = {
  id: string;
  name: string;
  addedBy: string;
  source: string;
  providerKey: string;
  updated: string;
  type: LiveKnowledgeSourceType;
};

export type LiveKnowledgeGraphNode = {
  id: string;
  label: string;
  group: string;
  tone: "core" | "support" | "policy" | "product" | "risk";
  x: number;
  y: number;
  radius: number;
  sourceRefs: string[];
  sourceIds: string[];
};

export type LiveKnowledgeGraphLink = {
  from: string;
  to: string;
  label: string;
  strength: number;
  sourceRefs: string[];
};

export type LiveKnowledgeGraph = {
  available: boolean;
  edgeCount: number;
  groups: string[];
  nodeCount: number;
  nodes: LiveKnowledgeGraphNode[];
  links: LiveKnowledgeGraphLink[];
  truncated: boolean;
};

export type LiveKnowledgeFinspo = {
  available: boolean;
  sourceCount: number;
  largestCount: number;
  inactiveCount: number;
  duplicateGroups: number;
  recommendationCount: number;
  reclaimableBytes: number;
};

export type LiveKnowledgeSyncMetrics = {
  connected: number;
  failed: number;
  syncing: number;
};

export type LiveKnowledgeWebSource = {
  id: string;
  kind: string;
  name: string;
  providerKey: string;
  status: string;
  updated: string;
  url: string;
};

export type LiveKnowledgeDiagnosticTone = "bad" | "good" | "neutral" | "warn";

export type LiveKnowledgeDiagnosticItem = {
  id: string;
  label: string;
  status: string;
  tone: LiveKnowledgeDiagnosticTone;
  detail: string;
  meta?: string;
};

export type LiveKnowledgeDiagnostics = {
  available: boolean;
  sparseBackend: string | null;
  vectorCollections: string[];
  quickwitIndexes: string[];
  services: LiveKnowledgeDiagnosticItem[];
  storage: LiveKnowledgeDiagnosticItem[];
  capabilities: LiveKnowledgeDiagnosticItem[];
};

export type LiveKnowledgePayload = {
  generatedAt: string;
  orgId: string | null;
  collections: LiveKnowledgeCollection[];
  dataPlane: {
    available: boolean;
    documentCount: number;
    indexedCount: number;
  };
  graph: LiveKnowledgeGraph;
  metrics: LiveKnowledgeSyncMetrics;
  metricCards: LiveKnowledgeMetric[];
  folders: LiveKnowledgeFolder[];
  integrations: LiveKnowledgeIntegration[];
  files: LiveKnowledgeFile[];
  sources: LiveKnowledgeSource[];
  webSources: LiveKnowledgeWebSource[];
  diagnostics: LiveKnowledgeDiagnostics;
  finspo: LiveKnowledgeFinspo;
};
