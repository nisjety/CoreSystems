import type {
  LiveKnowledgeCollection,
  LiveKnowledgePayload,
  LiveKnowledgeSource,
} from "@/features/knowledge-v2/lib/knowledge-live";

export type KnowledgeSidebarFolderNode = {
  id: string;
  label: string;
  count: number;
  providerKey: string;
  children?: KnowledgeSidebarFolderNode[];
  sourceIds?: string[];
};

export type KnowledgeSidebarTag = {
  label: string;
  count: number;
};

export type KnowledgeSidebarLiveData = {
  folders: KnowledgeSidebarFolderNode[];
  sources: LiveKnowledgeSource[];
  tags: KnowledgeSidebarTag[];
};

export function buildKnowledgeSidebarLiveData(payload: LiveKnowledgePayload): KnowledgeSidebarLiveData {
  const collections = payload.collections.filter((collection) => collection.id !== "all");
  const collectionNodes = collections.map((collection) => buildCollectionNode(collection, payload));
  const allCollection = payload.collections.find((collection) => collection.id === "all");

  return {
    folders: [
      buildGeneralKnowledgeNode(payload, allCollection, collectionNodes),
      buildRagOperationsNode(payload),
    ],
    sources: payload.sources,
    tags: buildKnowledgeTags(payload.sources),
  };
}

export function filterKnowledgeFolders(
  folders: readonly KnowledgeSidebarFolderNode[],
  normalizedSearch: string,
): readonly KnowledgeSidebarFolderNode[] {
  if (!normalizedSearch) {
    return folders;
  }

  return folders.reduce<KnowledgeSidebarFolderNode[]>((matches, folder) => {
    const children = folder.children ? [...filterKnowledgeFolders(folder.children, normalizedSearch)] : [];
    const folderMatches = folder.label.toLowerCase().includes(normalizedSearch);

    if (!folderMatches && children.length === 0) {
      return matches;
    }

    return [
      ...matches,
      {
        ...folder,
        children: children.length ? children : folder.children ? [...folder.children] : undefined,
      },
    ];
  }, []);
}

export function filterKnowledgeSources(
  sources: readonly LiveKnowledgeSource[],
  normalizedSearch: string,
): LiveKnowledgeSource[] {
  return sources.filter((source) => (
    !normalizedSearch ||
    source.title.toLowerCase().includes(normalizedSearch) ||
    source.type.toLowerCase().includes(normalizedSearch) ||
    source.category.toLowerCase().includes(normalizedSearch) ||
    source.tags.some((tag) => tag.toLowerCase().includes(normalizedSearch))
  ));
}

export function getVisibleKnowledgeTags(
  sources: readonly LiveKnowledgeSource[],
  normalizedSearch: string,
): KnowledgeSidebarTag[] {
  return buildKnowledgeTags(sources).filter((tag) => (
    !normalizedSearch || tag.label.toLowerCase().includes(normalizedSearch)
  ));
}

export function getKnowledgeFolderSourceIds(
  folders: readonly KnowledgeSidebarFolderNode[],
  folderId: string,
): string[] | null {
  for (const folder of folders) {
    if (folder.id === folderId) {
      return folder.sourceIds ?? null;
    }

    const childMatch = getKnowledgeFolderSourceIds(folder.children ?? [], folderId);
    if (childMatch !== null) {
      return childMatch;
    }
  }

  return null;
}

function buildGeneralKnowledgeNode(
  payload: LiveKnowledgePayload,
  allCollection: LiveKnowledgeCollection | undefined,
  collectionNodes: KnowledgeSidebarFolderNode[],
): KnowledgeSidebarFolderNode {
  const onboardingSources = payload.sources.filter(matchesOnboardingSource);
  const documentSources = payload.sources.filter((source) => !matchesOnboardingSource(source));
  const integrationChildren = collectionNodes.length > 0 ? collectionNodes : buildFallbackNodes(payload.sources);

  return {
    id: allCollection?.id ?? "all",
    label: allCollection?.label ?? "General Knowledge",
    count: allCollection?.count ?? payload.sources.length + payload.webSources.length,
    providerKey: "all",
    sourceIds: payload.sources.map((source) => source.id),
    children: [
      {
        id: "general:onboarding",
        label: "Onboarding",
        count: onboardingSources.length,
        providerKey: "onboarding",
        sourceIds: onboardingSources.map((source) => source.id),
        children: onboardingSources.slice(0, 6).map((source) => buildSourceLeafNode(source, "onboarding", "general:onboarding")),
      },
      {
        id: "general:integrations",
        label: "Integrations",
        count: integrationChildren.reduce((sum, child) => sum + child.count, 0),
        providerKey: "integrations",
        sourceIds: uniqueStringIds(integrationChildren.flatMap((child) => child.sourceIds ?? [])),
        children: integrationChildren,
      },
      {
        id: "general:documents",
        label: "Documents",
        count: documentSources.length,
        providerKey: "documents",
        sourceIds: documentSources.map((source) => source.id),
        children: documentSources.slice(0, 6).map((source) => buildSourceLeafNode(source, "documents", "general:documents")),
      },
    ],
  };
}

function buildCollectionNode(
  collection: LiveKnowledgeCollection,
  payload: LiveKnowledgePayload,
): KnowledgeSidebarFolderNode {
  const providerKey = providerKeyFromCollectionId(collection.id);
  const sourceIds = payload.sources
    .filter((source) => source.providerKey === providerKey || (providerKey === "web" && source.type === "URL"))
    .map((source) => source.id);

  if (providerKey === "web") {
    return {
      id: collection.id,
      label: collection.label,
      count: collection.count,
      providerKey,
      sourceIds,
      children: payload.webSources.map((source) => ({
        id: `web:${source.id}`,
        label: source.name,
        count: 1,
        providerKey: "web",
        sourceIds,
      })),
    };
  }

  const sourceChildren = payload.sources
    .filter((source) => source.providerKey === providerKey)
    .slice(0, 6)
    .map((source) => buildSourceLeafNode(source, providerKey, collection.id));

  return {
    id: collection.id,
    label: collection.label,
    count: collection.count,
    providerKey,
    sourceIds,
    children: sourceChildren,
  };
}

function buildFallbackNodes(sources: readonly LiveKnowledgeSource[]): KnowledgeSidebarFolderNode[] {
  const byProvider = new Map<string, LiveKnowledgeSource[]>();

  for (const source of sources) {
    const current = byProvider.get(source.providerKey) ?? [];
    byProvider.set(source.providerKey, [...current, source]);
  }

  return Array.from(byProvider.entries()).map(([providerKey, providerSources]) => ({
    id: `provider:${providerKey}`,
    label: providerSources[0]?.provider || providerKey,
    count: providerSources.length,
    providerKey,
    sourceIds: providerSources.map((source) => source.id),
    children: providerSources.slice(0, 6).map((source) => buildSourceLeafNode(source, providerKey, `provider:${providerKey}`)),
  }));
}

function buildKnowledgeTags(sources: readonly LiveKnowledgeSource[]): KnowledgeSidebarTag[] {
  const tagCounts = new Map<string, number>();

  for (const source of sources) {
    for (const tag of source.tags) {
      tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
    }
  }

  return Array.from(tagCounts, ([label, count]) => ({ label, count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label));
}

function buildRagOperationsNode(payload: LiveKnowledgePayload): KnowledgeSidebarFolderNode {
  const chunkQualitySources = payload.sources.filter((source) => source.chunks > 0);
  const retrievalEvalSources = payload.sources.filter((source) => (
    source.hitRate.length > 0 ||
    source.coverage.length > 0 ||
    source.similarity.length > 0
  ));

  return {
    id: "rag",
    label: "RAG Operations",
    count: Math.max(chunkQualitySources.length, retrievalEvalSources.length),
    providerKey: "rag",
    sourceIds: uniqueStringIds([
      ...chunkQualitySources.map((source) => source.id),
      ...retrievalEvalSources.map((source) => source.id),
    ]),
    children: [
      {
        id: "rag:chunk-quality",
        label: "Chunk quality",
        count: chunkQualitySources.length,
        providerKey: "rag",
        sourceIds: chunkQualitySources.map((source) => source.id),
      },
      {
        id: "rag:retrieval-evals",
        label: "Retrieval evals",
        count: retrievalEvalSources.length,
        providerKey: "rag",
        sourceIds: retrievalEvalSources.map((source) => source.id),
      },
    ],
  };
}

function buildSourceLeafNode(
  source: LiveKnowledgeSource,
  providerKey: string,
  parentId: string,
): KnowledgeSidebarFolderNode {
  return {
    id: `${parentId}:${source.id}`,
    label: source.title,
    count: source.chunks,
    providerKey,
    sourceIds: [source.id],
  };
}

function matchesOnboardingSource(source: LiveKnowledgeSource) {
  const searchable = [
    source.title,
    source.description,
    source.category,
    ...source.tags,
    ...source.related,
  ].join(" ").toLowerCase();

  return /(onboard|setup|activation|implement|launch|handoff|get started|getting started)/.test(searchable);
}

function uniqueStringIds(ids: readonly string[]) {
  return Array.from(new Set(ids.filter(Boolean)));
}

function providerKeyFromCollectionId(collectionId: string) {
  if (collectionId.startsWith("provider:")) {
    return collectionId.slice("provider:".length);
  }
  return collectionId;
}
