'use client';

import { type DragEvent, useMemo, useState } from 'react';
import {
  ChevronRight,
  Clock3,
  FileText,
  FolderKanban,
  GripVertical,
  PencilLine,
  Plus,
  RotateCcw,
  Search,
  Shapes,
  Star,
  Trash2,
  Users,
  LockKeyhole,
} from 'lucide-react';
import type {
  PlannerDocument,
  PlannerDocumentSpace,
  PlannerDocumentUpdateInput,
} from './services/planner-document-service';

type TreeNode = PlannerDocument & { children: TreeNode[] };

interface PlannerSidebarProps {
  documents: PlannerDocument[];
  activeDocId: string | null;
  isLoading: boolean;
  isBusy?: boolean;
  onSelect: (documentId: string) => void;
  onCreate: (input?: { parentDocumentId?: string | null; space?: PlannerDocumentSpace }) => Promise<unknown> | void;
  onUpdateDocument: (documentId: string, patch: PlannerDocumentUpdateInput) => Promise<unknown>;
  onArchive: (documentId: string) => Promise<unknown>;
  onRestore: (documentId: string) => Promise<unknown>;
}

const SECTION_META: Record<PlannerDocumentSpace, { label: string; description: string; Icon: typeof LockKeyhole }> = {
  private: {
    label: 'Private',
    description: 'Personal pages and nested drafts.',
    Icon: LockKeyhole,
  },
  shared: {
    label: 'Shared',
    description: 'Pages prepared for team-facing work.',
    Icon: Users,
  },
  collection: {
    label: 'Collections',
    description: 'Grouped knowledge and longer-running bodies of work.',
    Icon: Shapes,
  },
};

function formatShortDate(timestamp: number | undefined) {
  if (!timestamp) {
    return 'Recently';
  }

  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
  }).format(timestamp);
}

function sortDocuments(left: PlannerDocument, right: PlannerDocument) {
  if (left.isFavorite !== right.isFavorite) {
    return Number(right.isFavorite) - Number(left.isFavorite);
  }

  if (left.updatedAt !== right.updatedAt) {
    return right.updatedAt - left.updatedAt;
  }

  return left.title.localeCompare(right.title);
}

function buildTree(documents: PlannerDocument[]) {
  const visibleIds = new Set(documents.map((document) => document.id));
  const childrenByParent = new Map<string, PlannerDocument[]>();

  for (const document of documents) {
    if (!document.parentDocumentId || !visibleIds.has(document.parentDocumentId)) {
      continue;
    }

    const siblings = childrenByParent.get(document.parentDocumentId) ?? [];
    siblings.push(document);
    childrenByParent.set(document.parentDocumentId, siblings);
  }

  const rootDocuments = documents
    .filter((document) => !document.parentDocumentId || !visibleIds.has(document.parentDocumentId))
    .sort(sortDocuments);

  const inflate = (document: PlannerDocument): TreeNode => ({
    ...document,
    children: (childrenByParent.get(document.id) ?? []).sort(sortDocuments).map(inflate),
  });

  return rootDocuments.map(inflate);
}

export function PlannerSidebar({
  documents,
  activeDocId,
  isLoading,
  isBusy = false,
  onSelect,
  onCreate,
  onUpdateDocument,
  onArchive,
  onRestore,
}: PlannerSidebarProps) {
  const [query, setQuery] = useState('');
  const [editingDocId, setEditingDocId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState('');
  const [collapsedSections, setCollapsedSections] = useState<string[]>(['trash']);
  const [collapsedNodes, setCollapsedNodes] = useState<string[]>([]);
  const [draggedDocumentId, setDraggedDocumentId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

  const activeDocuments = useMemo(
    () => documents.filter((document) => !document.archivedAt),
    [documents]
  );
  const trashedDocuments = useMemo(
    () =>
      documents
        .filter((document) => Boolean(document.archivedAt))
        .sort((left, right) => (right.archivedAt ?? right.updatedAt) - (left.archivedAt ?? left.updatedAt)),
    [documents]
  );
  const documentsById = useMemo(
    () => new Map(documents.map((document) => [document.id, document] as const)),
    [documents]
  );

  const activeAncestorIds = useMemo(() => {
    if (!activeDocId) {
      return new Set<string>();
    }

    const expandedNodeIds = new Set<string>();
    let current = documentsById.get(activeDocId);

    while (current?.parentDocumentId) {
      expandedNodeIds.add(current.parentDocumentId);
      current = documentsById.get(current.parentDocumentId);
    }

    return expandedNodeIds;
  }, [activeDocId, documentsById]);

  const favoriteDocuments = useMemo(
    () =>
      activeDocuments
        .filter((document) => document.isFavorite)
        .sort((left, right) => right.lastViewedAt - left.lastViewedAt)
        .slice(0, 4),
    [activeDocuments]
  );
  const recentDocuments = useMemo(
    () =>
      [...activeDocuments]
        .sort((left, right) => right.lastViewedAt - left.lastViewedAt)
        .slice(0, 5),
    [activeDocuments]
  );
  const sectionTrees = useMemo(
    () => ({
      private: buildTree(activeDocuments.filter((document) => document.space === 'private')),
      shared: buildTree(activeDocuments.filter((document) => document.space === 'shared')),
      collection: buildTree(activeDocuments.filter((document) => document.space === 'collection')),
    }),
    [activeDocuments]
  );
  const searchResults = useMemo(() => {
    const normalized = query.trim().toLowerCase();

    if (!normalized) {
      return [];
    }

    return [...documents]
      .filter((document) => document.title.toLowerCase().includes(normalized))
      .sort((left, right) => {
        if (Boolean(left.archivedAt) !== Boolean(right.archivedAt)) {
          return Number(Boolean(left.archivedAt)) - Number(Boolean(right.archivedAt));
        }

        return sortDocuments(left, right);
      });
  }, [documents, query]);

  function toggleSection(sectionId: string) {
    setCollapsedSections((current) =>
      current.includes(sectionId)
        ? current.filter((value) => value !== sectionId)
        : [...current, sectionId]
    );
  }

  function toggleNode(documentId: string) {
    setCollapsedNodes((current) =>
      current.includes(documentId)
        ? current.filter((value) => value !== documentId)
        : [...current, documentId]
    );
  }

  function clearDragState() {
    setDraggedDocumentId(null);
    setDropTargetId(null);
  }

  function resolveDraggedDocumentId(event: DragEvent<HTMLElement>) {
    return event.dataTransfer.getData('text/plain') || draggedDocumentId;
  }

  function isDescendantDocument(ancestorDocumentId: string, candidateDocumentId: string) {
    let current = documentsById.get(candidateDocumentId);

    while (current?.parentDocumentId) {
      if (current.parentDocumentId === ancestorDocumentId) {
        return true;
      }

      current = documentsById.get(current.parentDocumentId);
    }

    return false;
  }

  function canDropOnDocument(targetDocumentId: string, sourceDocumentId: string | null) {
    if (!sourceDocumentId || sourceDocumentId === targetDocumentId) {
      return false;
    }

    return !isDescendantDocument(sourceDocumentId, targetDocumentId);
  }

  function handleDragStart(event: DragEvent<HTMLDivElement>, documentId: string) {
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('text/plain', documentId);
    setDraggedDocumentId(documentId);
    setDropTargetId(null);
  }

  function handleDragEnd() {
    clearDragState();
  }

  function handleDragOverDocument(event: DragEvent<HTMLDivElement>, documentId: string) {
    const sourceDocumentId = resolveDraggedDocumentId(event);

    if (!canDropOnDocument(documentId, sourceDocumentId)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = 'move';
    setDropTargetId(`doc:${documentId}`);
  }

  async function handleDropOnDocument(event: DragEvent<HTMLDivElement>, documentId: string) {
    const sourceDocumentId = resolveDraggedDocumentId(event);
    event.preventDefault();
    event.stopPropagation();

    if (!canDropOnDocument(documentId, sourceDocumentId) || !sourceDocumentId) {
      clearDragState();
      return;
    }

    const targetDocument = documentsById.get(documentId);
    const sourceDocument = documentsById.get(sourceDocumentId);

    if (!targetDocument || !sourceDocument) {
      clearDragState();
      return;
    }

    if (
      sourceDocument.parentDocumentId === targetDocument.id &&
      sourceDocument.space === targetDocument.space
    ) {
      clearDragState();
      return;
    }

    await onUpdateDocument(sourceDocumentId, {
      parentDocumentId: targetDocument.id,
      space: targetDocument.space,
    });
    clearDragState();
  }

  function handleDragOverSection(event: DragEvent<HTMLElement>, space: PlannerDocumentSpace) {
    const sourceDocumentId = resolveDraggedDocumentId(event);
    const sourceDocument = sourceDocumentId ? documentsById.get(sourceDocumentId) : null;

    if (!sourceDocument) {
      return;
    }

    if ((sourceDocument.parentDocumentId ?? null) === null && sourceDocument.space === space) {
      return;
    }

    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    setDropTargetId(`section:${space}`);
  }

  async function handleDropOnSection(event: DragEvent<HTMLElement>, space: PlannerDocumentSpace) {
    const sourceDocumentId = resolveDraggedDocumentId(event);
    const sourceDocument = sourceDocumentId ? documentsById.get(sourceDocumentId) : null;
    event.preventDefault();

    if (!sourceDocumentId || !sourceDocument) {
      clearDragState();
      return;
    }

    if ((sourceDocument.parentDocumentId ?? null) === null && sourceDocument.space === space) {
      clearDragState();
      return;
    }

    await onUpdateDocument(sourceDocumentId, {
      parentDocumentId: null,
      space,
    });
    clearDragState();
  }

  async function commitRename(documentId: string) {
    const nextTitle = draftTitle.trim();
    if (!nextTitle) {
      setEditingDocId(null);
      return;
    }

    await onUpdateDocument(documentId, { title: nextTitle });
    setEditingDocId(null);
  }

  function renderQuickShelf(title: string, items: PlannerDocument[], icon: 'favorite' | 'recent') {
    if (items.length === 0) {
      return null;
    }

    return (
      <section className="mb-5 px-2">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-[11px] font-semibold uppercase tracking-[0.24em] text-stone-500">
            {title}
          </h3>
          <span className="text-[11px] text-stone-400">{items.length}</span>
        </div>
        <div className="space-y-2">
          {items.map((document) => {
            const isActive = document.id === activeDocId;

            return (
              <button
                key={`${title}-${document.id}`}
                type="button"
                onClick={() => onSelect(document.id)}
                className={`flex w-full items-center gap-3 rounded-2xl border px-3 py-2 text-left transition ${
                  isActive
                    ? 'border-stone-300 bg-white text-stone-900 shadow-[0_12px_35px_rgba(120,94,62,0.08)]'
                    : 'border-transparent bg-white/55 text-stone-700 hover:border-stone-200/80 hover:bg-white/80'
                }`}
              >
                <span className={`rounded-xl p-2 ${icon === 'favorite' ? 'bg-amber-50 text-amber-700' : 'bg-stone-100 text-stone-600'}`}>
                  {icon === 'favorite' ? (
                    <Star className="h-3.5 w-3.5 fill-current" />
                  ) : (
                    <Clock3 className="h-3.5 w-3.5" />
                  )}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium">{document.title}</span>
                  <span className="block text-[11px] text-stone-500">
                    {icon === 'favorite' ? 'Viewed' : 'Recent'} {formatShortDate(document.lastViewedAt)}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      </section>
    );
  }

  function renderDocumentRow(
    document: PlannerDocument,
    options?: {
      depth?: number;
      children?: TreeNode[];
      archived?: boolean;
      forceFlat?: boolean;
    }
  ) {
    const depth = options?.depth ?? 0;
    const children = options?.children ?? [];
    const archived = options?.archived ?? false;
    const isEditing = document.id === editingDocId;
    const isActive = document.id === activeDocId;
    const isCollapsed = collapsedNodes.includes(document.id) && !activeAncestorIds.has(document.id);
    const isDragged = document.id === draggedDocumentId;
    const isDropTarget = dropTargetId === `doc:${document.id}`;

    return (
      <div key={document.id} className="space-y-1">
        <div
          draggable={!archived && !isEditing}
          onDragStart={(event) => handleDragStart(event, document.id)}
          onDragEnd={handleDragEnd}
          onDragOver={(event) => handleDragOverDocument(event, document.id)}
          onDrop={(event) => void handleDropOnDocument(event, document.id)}
          className={`group rounded-[22px] border px-3 py-2.5 transition ${
            isActive
              ? 'border-stone-300 bg-white text-stone-900 shadow-[0_16px_40px_rgba(120,94,62,0.10)]'
              : 'border-transparent bg-white/55 text-stone-700 hover:border-stone-200/80 hover:bg-white/80'
          } ${isDropTarget ? 'border-stone-400 bg-white shadow-[0_18px_40px_rgba(120,94,62,0.14)]' : ''} ${isDragged ? 'opacity-55' : ''}`}
          style={{ marginLeft: depth > 0 ? `${depth * 16}px` : undefined }}
        >
          <div className="flex items-start gap-2">
            <div className="mt-1 flex h-8 w-8 shrink-0 items-center justify-center">
              {!archived && children.length > 0 && !options?.forceFlat ? (
                <button
                  type="button"
                  onClick={() => toggleNode(document.id)}
                  className="rounded-full p-1 text-stone-500 transition hover:bg-stone-100 hover:text-stone-900"
                  title={isCollapsed ? 'Expand child pages' : 'Collapse child pages'}
                >
                  <ChevronRight className={`h-4 w-4 transition ${isCollapsed ? '' : 'rotate-90'}`} />
                </button>
              ) : (
                <span className="inline-flex h-7 w-7 items-center justify-center rounded-full bg-stone-100/70 text-stone-400">
                  <FileText className="h-3.5 w-3.5" />
                </span>
              )}
            </div>

            <button
              type="button"
              disabled={archived}
              onClick={() => onSelect(document.id)}
              className="flex min-w-0 flex-1 items-start gap-3 text-left disabled:cursor-default"
            >
              <span className={`mt-0.5 rounded-2xl p-2 ${isActive ? 'bg-stone-100' : 'bg-stone-100/70'}`}>
                <FileText className="h-4 w-4" />
              </span>

              <span className="min-w-0 flex-1">
                {isEditing ? (
                  <input
                    autoFocus
                    value={draftTitle}
                    onChange={(event) => setDraftTitle(event.target.value)}
                    onBlur={() => void commitRename(document.id)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') {
                        event.preventDefault();
                        void commitRename(document.id);
                      }

                      if (event.key === 'Escape') {
                        setEditingDocId(null);
                      }
                    }}
                    className="w-full rounded-md border border-stone-300 bg-white px-2 py-1 text-sm font-medium text-stone-900 outline-none"
                  />
                ) : (
                  <>
                    <span className="flex items-center gap-2 text-sm font-medium">
                      <span className="truncate">{document.title}</span>
                      {document.isFavorite ? (
                        <Star className="h-3 w-3 shrink-0 fill-amber-400 text-amber-500" />
                      ) : null}
                    </span>
                    <span className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-stone-500">
                      <span>{SECTION_META[document.space].label}</span>
                      <span>Viewed {formatShortDate(document.lastViewedAt)}</span>
                      <span>
                        {archived
                          ? `Trashed ${formatShortDate(document.archivedAt)}`
                          : `Edited ${formatShortDate(document.updatedAt)}`}
                      </span>
                    </span>
                  </>
                )}
              </span>
            </button>

            <div className="flex items-center gap-1 opacity-100 md:opacity-0 md:transition md:group-hover:opacity-100">
              {archived ? (
                <button
                  type="button"
                  onClick={() => void onRestore(document.id)}
                  className="rounded-full p-2 text-stone-500 transition hover:bg-stone-100 hover:text-emerald-700"
                  title="Restore page"
                >
                  <RotateCcw className="h-3.5 w-3.5" />
                </button>
              ) : (
                <>
                  <span
                    className="hidden cursor-grab rounded-full p-2 text-stone-400 transition group-hover:text-stone-600 md:inline-flex"
                    title="Drag to nest under another page or move to a different section"
                  >
                    <GripVertical className="h-3.5 w-3.5" />
                  </span>
                  <button
                    type="button"
                    onClick={() => void onCreate({ parentDocumentId: document.id, space: document.space })}
                    className="rounded-full p-2 text-stone-500 transition hover:bg-stone-100 hover:text-stone-900"
                    title="Create child page"
                  >
                    <Plus className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => void onUpdateDocument(document.id, { isFavorite: !document.isFavorite })}
                    className={`rounded-full p-2 transition hover:bg-stone-100 ${
                      document.isFavorite ? 'text-amber-500' : 'text-stone-500 hover:text-amber-500'
                    }`}
                    title={document.isFavorite ? 'Remove from favorites' : 'Add to favorites'}
                  >
                    <Star className={`h-3.5 w-3.5 ${document.isFavorite ? 'fill-current' : ''}`} />
                  </button>
                  <select
                    value={document.space}
                    onChange={(event) => {
                      const nextSpace = event.target.value as PlannerDocumentSpace;

                      if (nextSpace === document.space && !document.parentDocumentId) {
                        return;
                      }

                      void onUpdateDocument(document.id, {
                        parentDocumentId: null,
                        space: nextSpace,
                      });
                    }}
                    className="rounded-full border border-stone-200 bg-white/90 px-3 py-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-stone-500 outline-none transition hover:border-stone-300 hover:text-stone-900"
                    title="Move page to section root"
                  >
                    {(Object.keys(SECTION_META) as PlannerDocumentSpace[]).map((space) => (
                      <option key={`${document.id}-${space}`} value={space}>
                        {SECTION_META[space].label}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    onClick={() => {
                      setEditingDocId(document.id);
                      setDraftTitle(document.title);
                    }}
                    className="rounded-full p-2 text-stone-500 transition hover:bg-stone-100 hover:text-stone-900"
                    title="Rename page"
                  >
                    <PencilLine className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => void onArchive(document.id)}
                    className="rounded-full p-2 text-stone-500 transition hover:bg-stone-100 hover:text-red-600"
                    title="Move to trash"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </>
              )}
            </div>
          </div>
        </div>

        {!archived && !options?.forceFlat && children.length > 0 && !isCollapsed ? (
          <div className="space-y-1">
            {children.map((child) => renderDocumentRow(child, { depth: depth + 1, children: child.children }))}
          </div>
        ) : null}
      </div>
    );
  }

  function renderSection(space: PlannerDocumentSpace) {
    const { Icon, label, description } = SECTION_META[space];
    const tree = sectionTrees[space];
    const sectionId = `section:${space}`;
    const isCollapsed = collapsedSections.includes(sectionId);
    const isDropTarget = dropTargetId === sectionId;

    return (
      <section
        key={space}
        onDragOver={(event) => handleDragOverSection(event, space)}
        onDrop={(event) => void handleDropOnSection(event, space)}
        className={`mb-4 rounded-[28px] border border-stone-200/80 bg-white/45 px-3 py-3 shadow-[0_16px_40px_rgba(120,94,62,0.04)] ${
          isDropTarget ? 'border-stone-400 bg-white/80 shadow-[0_18px_44px_rgba(120,94,62,0.10)]' : ''
        }`}
      >
        <div className="flex items-start justify-between gap-3 px-2 pb-2">
          <button
            type="button"
            onClick={() => toggleSection(sectionId)}
            className="flex min-w-0 items-start gap-3 text-left"
          >
            <span className="rounded-2xl bg-stone-100 p-2 text-stone-700">
              <Icon className="h-4 w-4" />
            </span>
            <span className="min-w-0">
              <span className="flex items-center gap-2 text-sm font-semibold text-stone-900">
                {label}
                <span className="rounded-full bg-stone-100 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-stone-500">
                  {tree.length}
                </span>
              </span>
              <span className="mt-1 block text-xs leading-5 text-stone-500">{description}</span>
            </span>
          </button>

          <button
            type="button"
            onClick={() => void onCreate({ space })}
            className="rounded-full border border-stone-200 bg-white p-2 text-stone-600 transition hover:border-stone-300 hover:text-stone-900"
            title={`Create ${label} page`}
          >
            <Plus className="h-4 w-4" />
          </button>
        </div>

        {!isCollapsed ? (
          tree.length > 0 ? (
            <div className={`space-y-2 rounded-[24px] transition ${isDropTarget ? 'bg-stone-100/70 p-2' : ''}`}>
              {tree.map((document) => renderDocumentRow(document, { children: document.children }))}
            </div>
          ) : (
            <p className="px-2 pb-2 text-sm text-stone-500">
              {draggedDocumentId
                ? `Drop here to move a page to ${label.toLowerCase()}.`
                : `No pages in ${label.toLowerCase()} yet.`}
            </p>
          )
        ) : null}
      </section>
    );
  }

  const showingSearch = query.trim().length > 0;

  return (
    <aside className="flex h-full w-[380px] shrink-0 flex-col overflow-hidden rounded-[32px] border border-stone-200/80 bg-[linear-gradient(180deg,rgba(249,244,236,0.98),rgba(245,238,228,0.94))] text-stone-900 shadow-[0_28px_90px_rgba(110,90,60,0.08)]">
      <div className="border-b border-stone-200/80 px-5 pb-4 pt-5">
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-[11px] uppercase tracking-[0.28em] text-stone-500">Workspace</p>
            <h2 className="mt-2 text-xl font-semibold tracking-tight text-stone-900">Field Notes</h2>
            <p className="mt-1 max-w-[20rem] text-sm leading-5 text-stone-500">
              AFFiNE-style page explorer backed by the planner model, with nested pages,
              persisted favorites, recent history, and trash.
            </p>
          </div>

          <button
            type="button"
            onClick={() => void onCreate({ space: 'private' })}
            disabled={isBusy}
            className="inline-flex h-10 w-10 items-center justify-center rounded-full border border-stone-300/80 bg-white/80 text-stone-700 shadow-[0_8px_30px_rgba(120,94,62,0.08)] transition hover:border-stone-400 hover:bg-white disabled:cursor-not-allowed disabled:opacity-60"
            title="Create a new page"
          >
            <Plus className="h-4 w-4" />
          </button>
        </div>

        <label className="mt-4 flex items-center gap-3 rounded-full border border-stone-300/80 bg-white/70 px-4 py-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.7)]">
          <Search className="h-4 w-4 text-stone-400" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search pages"
            className="w-full bg-transparent text-sm text-stone-700 outline-none placeholder:text-stone-400"
          />
        </label>

        <div className="mt-4 grid grid-cols-3 gap-2">
          {(Object.keys(SECTION_META) as PlannerDocumentSpace[]).map((space) => {
            const { Icon, label } = SECTION_META[space];
            const count = activeDocuments.filter((document) => document.space === space).length;

            return (
              <button
                key={space}
                type="button"
                onClick={() => void onCreate({ space })}
                className="rounded-2xl border border-stone-200/70 bg-white/55 px-3 py-3 text-left text-stone-600 transition hover:bg-white/80"
                title={`Create ${label} page`}
              >
                <Icon className="h-4 w-4" />
                <span className="mt-2 block text-xs font-medium">{label}</span>
                <span className="mt-1 block text-[11px] text-stone-500">{count}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-3 py-4">
        {isLoading ? (
          <div className="space-y-3 px-2">
            {Array.from({ length: 5 }).map((_, index) => (
              <div
                key={index}
                className="h-16 animate-pulse rounded-2xl bg-white/55 shadow-[0_10px_25px_rgba(120,94,62,0.05)]"
              />
            ))}
          </div>
        ) : showingSearch ? (
          <section>
            <div className="mb-3 px-2">
              <h3 className="text-[11px] font-semibold uppercase tracking-[0.24em] text-stone-500">
                Search results
              </h3>
              <p className="mt-1 text-xs leading-5 text-stone-500">
                Matching pages across the planner tree and trash.
              </p>
            </div>

            {searchResults.length > 0 ? (
              <div className="space-y-2">
                {searchResults.map((document) =>
                  renderDocumentRow(document, {
                    archived: Boolean(document.archivedAt),
                    forceFlat: true,
                  })
                )}
              </div>
            ) : (
              <div className="rounded-[28px] border border-dashed border-stone-300/90 bg-white/55 px-5 py-8 text-center shadow-[0_16px_40px_rgba(120,94,62,0.05)]">
                <p className="text-sm font-medium text-stone-700">No matching pages</p>
                <p className="mt-2 text-sm leading-6 text-stone-500">
                  Try another title or create a fresh page in one of the planner sections.
                </p>
              </div>
            )}
          </section>
        ) : documents.length === 0 ? (
          <div className="rounded-[28px] border border-dashed border-stone-300/90 bg-white/55 px-5 py-8 text-center shadow-[0_16px_40px_rgba(120,94,62,0.05)]">
            <p className="text-sm font-medium text-stone-700">No pages yet</p>
            <p className="mt-2 text-sm leading-6 text-stone-500">
              Create the first planner page and it will appear inside the nested workspace tree.
            </p>
            <button
              type="button"
              onClick={() => void onCreate({ space: 'private' })}
              className="mt-5 inline-flex items-center gap-2 rounded-full bg-stone-900 px-4 py-2 text-sm font-medium text-stone-50 transition hover:bg-stone-700"
            >
              <Plus className="h-4 w-4" />
              New page
            </button>
          </div>
        ) : (
          <>
            {renderQuickShelf('Favorites', favoriteDocuments, 'favorite')}
            {renderQuickShelf('Recently viewed', recentDocuments, 'recent')}

            {(Object.keys(SECTION_META) as PlannerDocumentSpace[]).map((space) => renderSection(space))}

            <section className="rounded-[28px] border border-stone-200/80 bg-white/45 px-3 py-3 shadow-[0_16px_40px_rgba(120,94,62,0.04)]">
              <div className="flex items-start justify-between gap-3 px-2 pb-2">
                <button
                  type="button"
                  onClick={() => toggleSection('trash')}
                  className="flex min-w-0 items-start gap-3 text-left"
                >
                  <span className="rounded-2xl bg-stone-100 p-2 text-stone-700">
                    <Trash2 className="h-4 w-4" />
                  </span>
                  <span className="min-w-0">
                    <span className="flex items-center gap-2 text-sm font-semibold text-stone-900">
                      Trash
                      <span className="rounded-full bg-stone-100 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-[0.18em] text-stone-500">
                        {trashedDocuments.length}
                      </span>
                    </span>
                    <span className="mt-1 block text-xs leading-5 text-stone-500">
                      Archived pages remain in the planner model until they are restored.
                    </span>
                  </span>
                </button>
              </div>

              {!collapsedSections.includes('trash') ? (
                trashedDocuments.length > 0 ? (
                  <div className="space-y-2">
                    {trashedDocuments.map((document) =>
                      renderDocumentRow(document, { archived: true, forceFlat: true })
                    )}
                  </div>
                ) : (
                  <p className="px-2 pb-2 text-sm text-stone-500">Trash is empty.</p>
                )
              ) : null}
            </section>
          </>
        )}
      </div>

      <div className="border-t border-stone-200/80 px-5 py-4 text-xs text-stone-500">
        <div className="flex items-center gap-2">
          <FolderKanban className="h-3.5 w-3.5" />
          <span>
            Shared, private, and collection sections are planner metadata buckets now. Drag pages into
            another page to nest them, or drop them onto a section to promote them at the root.
          </span>
        </div>
      </div>
    </aside>
  );
}