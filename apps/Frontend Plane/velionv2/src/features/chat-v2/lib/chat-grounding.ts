export type ChatGroundingSource = {
  id: string;
  kind: "knowledge";
  title: string;
  snippet: string;
  provider: string;
  sourceType: string;
  documentId: string;
  href: string;
  score: number;
};

export type ChatGroundingFact = {
  knowledgeId: string;
  documentId: string;
  text: string;
  score: number;
  sourceTitle: string;
  sourceType: string;
  provider: string;
  chunkIndex: number;
};

export type ChatGroundingGraphNode = {
  id: string;
  label: string;
  kind: string;
};

export type ChatGroundingGraph = {
  traceId?: string;
  communitySummaries: string[];
  edgeCount: number;
  nodes: ChatGroundingGraphNode[];
};

export type ChatKnowledgeGrounding = {
  mode: "retrieve" | "hybrid";
  query: string;
  traceId?: string;
  lowConfidence: boolean;
  factCount: number;
  sourceCount: number;
  facts: ChatGroundingFact[];
  sources: ChatGroundingSource[];
  graph?: ChatGroundingGraph;
};
