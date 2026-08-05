import {
  BookOpen,
  FileText,
  Folder,
  Globe2,
  Link2,
  NotepadText,
  ShieldCheck,
  type LucideIcon,
} from "lucide-react";

export type KnowledgeStatus = "Indexed" | "Pending review" | "Re-indexing";
export type KnowledgeSourceType = "PDF" | "Notion" | "Docs" | "URL";

export type KnowledgeSource = {
  id: string;
  title: string;
  description: string;
  type: KnowledgeSourceType;
  icon: LucideIcon;
  provider: string;
  category: string;
  owner: string;
  updated: string;
  size: string;
  status: KnowledgeStatus;
  chunks: number;
  hitRate: string;
  coverage: string;
  similarity: string;
  tags: string[];
  backlinks: string[];
  outgoing: string[];
  chunksPreview: Array<{
    id: string;
    title: string;
    score: string;
    text: string;
  }>;
};

export type KnowledgeFolder = {
  id: string;
  title: string;
  subtitle: string;
  fileCount: number;
  noteCount: number;
  connections: string[];
  tone: "warm" | "green" | "blue" | "gray";
};

export type KnowledgeFile = {
  id: string;
  name: string;
  addedBy: string;
  source: string;
  updated: string;
  type: KnowledgeSourceType;
};

export type KnowledgeIntegration = {
  id: string;
  name: string;
  status: "Connected" | "Syncing" | "Review";
  documents: string;
  freshness: string;
};

export type GraphNode = {
  id: string;
  label: string;
  x: number;
  y: number;
  radius: number;
  tone: "core" | "support" | "policy" | "product" | "risk";
};

export type GraphLink = {
  from: string;
  to: string;
  strength: number;
};

export const knowledgeMetrics = [
  {
    label: "Indexed documents",
    value: "94,201",
    delta: "+3,841",
    tone: "good",
  },
  {
    label: "Average retrieval",
    value: "0.9s",
    delta: "-0.2s",
    tone: "good",
  },
  {
    label: "Hit rate",
    value: "87%",
    delta: "-4%",
    tone: "warn",
  },
  {
    label: "Graph coverage",
    value: "95%",
    delta: "+7%",
    tone: "good",
  },
] as const;

export const knowledgeCollections = [
  {
    id: "general",
    label: "General Knowledge",
    count: 142,
    icon: Folder,
    children: [
      { id: "onboarding", label: "Onboarding", count: 15 },
      { id: "integrations", label: "Integrations", count: 29 },
      { id: "support", label: "Support policies", count: 41 },
    ],
  },
  {
    id: "rag",
    label: "RAG Operations",
    count: 32,
    icon: ShieldCheck,
    children: [
      { id: "chunks", label: "Chunk quality", count: 18 },
      { id: "evals", label: "Retrieval evals", count: 14 },
    ],
  },
] as const;

export const knowledgeFolderCards: KnowledgeFolder[] = [
  {
    id: "onboarding",
    title: "Onboarding",
    subtitle: "Guides & activation",
    fileCount: 15,
    noteCount: 1270,
    connections: ["Drive", "Notion"],
    tone: "warm",
  },
  {
    id: "integrations",
    title: "Integrations",
    subtitle: "Sources & sync",
    fileCount: 5,
    noteCount: 840,
    connections: ["Notion", "SharePoint", "Drive"],
    tone: "green",
  },
  {
    id: "documents",
    title: "Documents",
    subtitle: "Files & references",
    fileCount: 10,
    noteCount: 1134,
    connections: ["Word", "PowerPoint"],
    tone: "blue",
  },
  {
    id: "team-interviews",
    title: "Team Interviews",
    subtitle: "Voice of customer",
    fileCount: 8,
    noteCount: 462,
    connections: ["Docs", "Web"],
    tone: "gray",
  },
];

export const knowledgeFiles: KnowledgeFile[] = [
  {
    id: "onboarding-guide",
    name: "Onboarding-Guide.pdf",
    addedBy: "kevin@mail.com",
    source: "Google Drive",
    updated: "48m ago",
    type: "PDF",
  },
  {
    id: "product-roadmap",
    name: "Product-Roadmap.docx",
    addedBy: "antomwe@gmail.com",
    source: "Microsoft 365",
    updated: "1h ago",
    type: "Docs",
  },
  {
    id: "shipping-faq",
    name: "Shipping FAQ",
    addedBy: "maria@verevon.ai",
    source: "Notion",
    updated: "2h ago",
    type: "Notion",
  },
  {
    id: "refund-sop",
    name: "Refund SOP.pdf",
    addedBy: "jane@verevon.ai",
    source: "Upload",
    updated: "3h ago",
    type: "PDF",
  },
];

export const knowledgeIntegrations: KnowledgeIntegration[] = [
  {
    id: "notion",
    name: "Notion",
    status: "Connected",
    documents: "28 docs",
    freshness: "2m ago",
  },
  {
    id: "drive",
    name: "Google Drive",
    status: "Connected",
    documents: "41 docs",
    freshness: "6m ago",
  },
  {
    id: "sharepoint",
    name: "SharePoint",
    status: "Syncing",
    documents: "17 docs",
    freshness: "Indexing",
  },
  {
    id: "web",
    name: "Website crawler",
    status: "Review",
    documents: "56 pages",
    freshness: "Needs review",
  },
];

export const knowledgeSources: KnowledgeSource[] = [
  {
    id: "shipping-faq",
    title: "Shipping FAQ",
    description: "Common delivery timelines, costs, tracking states, and exception language.",
    type: "Notion",
    icon: NotepadText,
    provider: "Notion",
    category: "Shipping",
    owner: "kevin@mail.com",
    updated: "48m ago",
    size: "84 KB",
    status: "Indexed",
    chunks: 22,
    hitRate: "91%",
    coverage: "95%",
    similarity: "0.87",
    tags: ["Shipping", "Policy", "Digital"],
    backlinks: ["Refund SOP", "Product Setup Guide", "International Shipping"],
    outgoing: ["Order Tracking Guide", "Carrier Exceptions"],
    chunksPreview: [
      {
        id: "chunk-03",
        title: "Carrier delay escalation",
        score: "0.94",
        text: "Escalate delayed parcels after the promised delivery window closes and include the carrier scan ID in the customer reply.",
      },
      {
        id: "chunk-09",
        title: "Tracking language",
        score: "0.91",
        text: "When a parcel has no scan update for two business days, use the pending-carrier template and offer proactive follow-up.",
      },
    ],
  },
  {
    id: "returns-policy",
    title: "Returns Policy v4",
    description: "Updated return window, eligibility criteria, and packaging requirements.",
    type: "Docs",
    icon: FileText,
    provider: "Google Docs",
    category: "Returns",
    owner: "antomwe@gmail.com",
    updated: "1m ago",
    size: "126 KB",
    status: "Indexed",
    chunks: 14,
    hitRate: "88%",
    coverage: "97%",
    similarity: "0.91",
    tags: ["Returns", "Refunds", "Policy"],
    backlinks: ["Refund SOP", "Warranty Terms", "B2B Account Policy"],
    outgoing: ["Return Window", "Restocking Exceptions"],
    chunksPreview: [
      {
        id: "chunk-01",
        title: "Return window",
        score: "0.96",
        text: "Items must be returned within 30 days of delivery in original packaging with all accessories included.",
      },
      {
        id: "chunk-05",
        title: "Refund timing",
        score: "0.89",
        text: "Approved refunds are processed after warehouse intake and quality review, with payment timing varying by provider.",
      },
    ],
  },
  {
    id: "refund-sop",
    title: "Refund SOP",
    description: "Agent workflow for refunds, partial credits, fraud checks, and approvals.",
    type: "PDF",
    icon: FileText,
    provider: "Upload",
    category: "Finance",
    owner: "maria@verevon.ai",
    updated: "1h ago",
    size: "240 KB",
    status: "Pending review",
    chunks: 17,
    hitRate: "76%",
    coverage: "82%",
    similarity: "0.79",
    tags: ["Refunds", "Risk", "Support"],
    backlinks: ["Returns Policy v4", "Shipping FAQ"],
    outgoing: ["Escalation Matrix", "Billing Exceptions"],
    chunksPreview: [
      {
        id: "chunk-06",
        title: "Approval threshold",
        score: "0.84",
        text: "Refunds above the policy threshold require supervisor approval before customer confirmation.",
      },
      {
        id: "chunk-11",
        title: "Fraud review",
        score: "0.78",
        text: "Repeated claims across the same account should be routed to the risk queue with order and shipment evidence.",
      },
    ],
  },
  {
    id: "setup-guide",
    title: "Product Setup Guide",
    description: "Internal product activation guide with required details and configuration steps.",
    type: "URL",
    icon: Globe2,
    provider: "Website",
    category: "Setup",
    owner: "jane@verevon.ai",
    updated: "2h ago",
    size: "98 KB",
    status: "Re-indexing",
    chunks: 11,
    hitRate: "83%",
    coverage: "71%",
    similarity: "0.81",
    tags: ["Setup", "Onboarding", "Product"],
    backlinks: ["Shipping FAQ", "Warranty Terms"],
    outgoing: ["Integrations Checklist", "Agent Handoff"],
    chunksPreview: [
      {
        id: "chunk-02",
        title: "Required account details",
        score: "0.86",
        text: "Collect workspace name, billing country, default language, and support ownership before activation.",
      },
      {
        id: "chunk-08",
        title: "Handoff check",
        score: "0.73",
        text: "Assign the setup record to the support owner once the first successful sync has completed.",
      },
    ],
  },
];

export const graphNodes: GraphNode[] = [
  { id: "shipping-faq", label: "Shipping FAQ", x: 318, y: 168, radius: 34, tone: "core" },
  { id: "returns-policy", label: "Returns Policy", x: 190, y: 226, radius: 29, tone: "policy" },
  { id: "refund-sop", label: "Refund SOP", x: 448, y: 244, radius: 31, tone: "risk" },
  { id: "setup-guide", label: "Setup Guide", x: 322, y: 340, radius: 27, tone: "product" },
  { id: "tracking", label: "Order Tracking", x: 158, y: 128, radius: 18, tone: "support" },
  { id: "warranty", label: "Warranty", x: 510, y: 132, radius: 20, tone: "support" },
  { id: "carrier", label: "Carrier Exceptions", x: 126, y: 334, radius: 17, tone: "risk" },
  { id: "handoff", label: "Agent Handoff", x: 506, y: 360, radius: 17, tone: "product" },
];

export const graphLinks: GraphLink[] = [
  { from: "shipping-faq", to: "returns-policy", strength: 3 },
  { from: "shipping-faq", to: "refund-sop", strength: 4 },
  { from: "shipping-faq", to: "setup-guide", strength: 2 },
  { from: "shipping-faq", to: "tracking", strength: 2 },
  { from: "refund-sop", to: "returns-policy", strength: 3 },
  { from: "refund-sop", to: "warranty", strength: 2 },
  { from: "returns-policy", to: "carrier", strength: 1 },
  { from: "setup-guide", to: "handoff", strength: 2 },
  { from: "setup-guide", to: "warranty", strength: 1 },
];


export const sourceTypeIcon: Record<KnowledgeSourceType, LucideIcon> = {
  PDF: FileText,
  Notion: NotepadText,
  Docs: BookOpen,
  URL: Link2,
};
