"use client";

import { useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  BookOpen,
  CheckCircle2,
  ChevronDown,
  Database,
  KeyRound,
  Plus,
  type LucideIcon,
} from "lucide-react";
import { knowledgeCollections, knowledgeSources } from "@/features/knowledge-v2/lib/knowledge-data";
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from "@/features/shell-v2/components/VelionSidebarPrimitives";
import { sidebarFocusClass, sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

const knowledgeSidebarTree = [
  {
    id: "general",
    label: "General Knowledge",
    count: 142,
    children: [
      {
        id: "onboarding",
        label: "Onboarding",
        count: 15,
        children: [
          { id: "subfolder-1", label: "Subfolder 1", count: 5 },
          { id: "subfolder-2", label: "Subfolder 2", count: 10 },
        ],
      },
      { id: "integrations", label: "Integrations", count: 29 },
      { id: "documents", label: "Documents", count: 41 },
    ],
  },
  {
    id: "rag",
    label: "RAG Operations",
    count: 32,
    children: [
      { id: "chunk-quality", label: "Chunk quality", count: 18 },
      { id: "retrieval-evals", label: "Retrieval evals", count: 14 },
    ],
  },
] as const;

type KnowledgeSidebarModeId = "folders" | "sources" | "tags";

type KnowledgeSidebarFolder = {
  id: string;
  label: string;
  count: number;
  children?: readonly KnowledgeSidebarFolder[];
};

const knowledgeSidebarModeOptions: Array<{ id: KnowledgeSidebarModeId; label: string; icon: LucideIcon }> = [
  { id: "folders", label: "Knowledge Base", icon: BookOpen },
  { id: "sources", label: "Sources", icon: Database },
  { id: "tags", label: "Tags", icon: KeyRound },
];

export function KnowledgeExpandedSidebarPanel({ onCollapse }: { onCollapse: () => void }) {
  const [activeMode, setActiveMode] = useState<KnowledgeSidebarModeId>("folders");
  const [activeFolderId, setActiveFolderId] = useState<string>(knowledgeSidebarTree[0].id);
  const [activeSourceId, setActiveSourceId] = useState(knowledgeSources[0].id);
  const [searchQuery, setSearchQuery] = useState("");
  const normalizedSearch = searchQuery.trim().toLowerCase();
  const visibleFolders = filterKnowledgeFolders(knowledgeSidebarTree, normalizedSearch);
  const visibleSources = knowledgeSources.filter((source) => (
    !normalizedSearch ||
    source.title.toLowerCase().includes(normalizedSearch) ||
    source.type.toLowerCase().includes(normalizedSearch) ||
    source.tags.some((tag) => tag.toLowerCase().includes(normalizedSearch))
  ));
  const visibleTags = getVisibleKnowledgeTags(normalizedSearch);

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <SidebarPanelTitle onCollapse={onCollapse}>Knowledge</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel="Filter knowledge section"
        className="mb-3"
        value={searchQuery}
        onChange={setSearchQuery}
      />

      <KnowledgeModeSelector value={activeMode} onChange={setActiveMode} />

      <nav className="min-h-0 flex-1 overflow-y-auto pr-1" aria-label="Knowledge navigation">
        {activeMode === "folders" ? (
          <div className="space-y-5">
            <section>
              <div className="mb-3 flex items-center justify-between px-1">
                <h2 className={cn("truncate text-[#1C1C1E] dark:text-white", sidebarType.groupTitle)}>Collections</h2>
                <button
                  type="button"
                  className="grid size-7 place-items-center rounded-full border border-[#DDE1E8] bg-white text-[#111827] shadow-[0_1px_3px_rgba(16,24,40,0.12)] transition-colors hover:bg-[#F8FAFC] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:border-[#303238] dark:bg-[#17181C] dark:text-white"
                  aria-label="Add collection"
                  title="Add collection"
                >
                  <Plus className="size-4" strokeWidth={1.9} />
                </button>
              </div>
              <div className="space-y-1">
                {visibleFolders.length ? (
                  visibleFolders.map((folder) => (
                    <KnowledgeSidebarTreeItem
                      key={folder.id}
                      activeFolderId={activeFolderId}
                      depth={0}
                      folder={folder}
                      onSelect={setActiveFolderId}
                    />
                  ))
                ) : (
                  <KnowledgeSidebarEmptyState label="No collections found." />
                )}
              </div>
            </section>

            <KnowledgeSourcesList
              activeSourceId={activeSourceId}
              limit={4}
              onSelect={setActiveSourceId}
              sources={visibleSources}
              title="Pinned sources"
            />
          </div>
        ) : null}

        {activeMode === "sources" ? (
          <KnowledgeSourcesList
            activeSourceId={activeSourceId}
            onSelect={setActiveSourceId}
            sources={visibleSources}
            title="Sources"
          />
        ) : null}

        {activeMode === "tags" ? (
          <section>
            <div className="mb-3 flex items-center justify-between px-1">
              <h2 className={cn("truncate text-[#1C1C1E] dark:text-white", sidebarType.groupTitle)}>Tags</h2>
              <span className={cn("text-[#8B95A7]", sidebarType.secondary)}>{visibleTags.length}</span>
            </div>
            <div className="space-y-1">
              {visibleTags.length ? (
                visibleTags.map((tag) => (
                  <button
                    key={tag.label}
                    type="button"
                    className={cn(
                      "flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left text-[#3F4652] transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] dark:text-[#B7BEC9] dark:hover:bg-white/5 dark:hover:text-white",
                      sidebarFocusClass,
                    )}
                  >
                    <KeyRound className={cn("shrink-0 text-[#6F7786]", sidebarType.icon)} strokeWidth={1.75} />
                    <span className={cn("min-w-0 flex-1 truncate", sidebarType.rowNormal)}>{tag.label}</span>
                    <span className="grid h-5 min-w-5 place-items-center rounded-full bg-[#ECEFF4] px-1.5 text-[11px] font-semibold text-[#6F7786] dark:bg-[#202229] dark:text-[#B7BEC9]">{tag.count}</span>
                  </button>
                ))
              ) : (
                <KnowledgeSidebarEmptyState label="No tags found." />
              )}
            </div>
          </section>
        ) : null}
      </nav>
    </div>
  );
}

function KnowledgeModeSelector({
  onChange,
  value,
}: {
  onChange: (value: KnowledgeSidebarModeId) => void;
  value: KnowledgeSidebarModeId;
}) {
  const [open, setOpen] = useState(false);
  const listboxId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const selectedIndex = (() => {
    const optionIndex = knowledgeSidebarModeOptions.findIndex((option) => option.id === value);
    return optionIndex >= 0 ? optionIndex : 0;
  })();
  const [activeIndex, setActiveIndex] = useState(selectedIndex);
  const selectedOption = knowledgeSidebarModeOptions[selectedIndex] ?? knowledgeSidebarModeOptions[0];
  const SelectedIcon = selectedOption.icon;

  const selectOptionAtIndex = (nextIndex: number) => {
    const option = knowledgeSidebarModeOptions[nextIndex];
    if (!option) {
      return;
    }

    onChange(option.id);
    setOpen(false);
  };

  const moveActiveOption = (direction: 1 | -1) => {
    setActiveIndex((currentIndex) => {
      const baseIndex = open ? currentIndex : selectedIndex;
      return (baseIndex + direction + knowledgeSidebarModeOptions.length) % knowledgeSidebarModeOptions.length;
    });
    setOpen(true);
  };

  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      moveActiveOption(1);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      moveActiveOption(-1);
      return;
    }

    if (event.key === "Home") {
      event.preventDefault();
      setActiveIndex(0);
      setOpen(true);
      return;
    }

    if (event.key === "End") {
      event.preventDefault();
      setActiveIndex(knowledgeSidebarModeOptions.length - 1);
      setOpen(true);
      return;
    }

    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (open) {
        selectOptionAtIndex(activeIndex);
        return;
      }

      setOpen(true);
      return;
    }

    if (event.key === "Escape") {
      setOpen(false);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }

    const closeOnOutsidePointer = (event: PointerEvent) => {
      const target = event.target as Node | null;
      if (target && rootRef.current?.contains(target)) {
        return;
      }

      setOpen(false);
    };

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
      }
    };

    document.addEventListener("pointerdown", closeOnOutsidePointer, true);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer, true);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative mb-5">
      <span className="pointer-events-none absolute left-2.5 top-1/2 z-10 grid size-[22px] -translate-y-1/2 place-items-center rounded-[7px] bg-[#1D1D1F] text-white dark:bg-white dark:text-[#111111]">
        <SelectedIcon className="size-3" strokeWidth={2.1} />
      </span>
      <button
        type="button"
        aria-label="Select knowledge view"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        onClick={() => {
          setActiveIndex(selectedIndex);
          setOpen((current) => !current);
        }}
        onKeyDown={handleKeyDown}
        className={cn(
          "flex h-9 w-full items-center rounded-[9px] border border-[#E3E5EA] bg-white pl-10 pr-8 text-left text-[#1D1D1F] shadow-[0_1px_2px_rgba(16,24,40,0.04)] outline-none transition-colors hover:bg-[#FAFAFB] focus:ring-2 focus:ring-[#DD7A1F]/20 dark:border-[#2B2D33] dark:bg-[#17181C] dark:text-white dark:hover:bg-[#202228]",
          sidebarType.rowStrong,
          sidebarFocusClass,
        )}
      >
        <span className="min-w-0 flex-1 truncate">{selectedOption.label}</span>
      </button>
      <ChevronDown className={cn("pointer-events-none absolute right-2.5 top-1/2 size-3.5 -translate-y-1/2 text-[#1D1D1F] transition-transform dark:text-white", open ? "rotate-180" : "")} strokeWidth={2.2} />
      {open ? (
        <menu id={listboxId} className="velion-popover absolute left-0 right-0 top-[calc(100%+8px)] z-[90] m-0 list-none p-1" aria-label="Knowledge view options">
          {knowledgeSidebarModeOptions.map((option, index) => {
            const selected = option.id === value;
            const active = activeIndex === index;
            return (
              <li key={option.id} role="presentation">
                <button
                  id={`${listboxId}-${option.id}`}
                  type="button"
                  role="menuitemradio"
                  aria-checked={selected}
                  onMouseEnter={() => setActiveIndex(index)}
                  onClick={() => selectOptionAtIndex(index)}
                  className={cn(
                    "flex h-9 w-full items-center rounded-[10px] px-3 text-left transition-colors",
                    sidebarType.row,
                    selected
                      ? "bg-[#F2F2F2] text-[#111111] dark:bg-[#23252A] dark:text-white"
                      : active
                      ? "bg-[#FAFAFA] text-[#111111] dark:bg-[#191A1F] dark:text-white"
                      : "text-[#555555] hover:bg-[#FAFAFA] hover:text-[#111111] dark:text-[#D0D6E0] dark:hover:bg-[#191A1F] dark:hover:text-white",
                    sidebarFocusClass,
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{option.label}</span>
                  {selected ? <CheckCircle2 className="size-3.5 shrink-0 text-[#12B76A]" strokeWidth={2} /> : null}
                </button>
              </li>
            );
          })}
        </menu>
      ) : null}
    </div>
  );
}

function KnowledgeSourcesList({
  activeSourceId,
  limit,
  onSelect,
  sources,
  title,
}: {
  activeSourceId: string;
  limit?: number;
  onSelect: (sourceId: string) => void;
  sources: typeof knowledgeSources;
  title: string;
}) {
  const visibleSources = limit ? sources.slice(0, limit) : sources;

  return (
    <section>
      <div className="mb-3 flex items-center justify-between px-1">
        <h2 className={cn("truncate text-[#1C1C1E] dark:text-white", sidebarType.groupTitle)}>{title}</h2>
        <span className={cn("text-[#8B95A7]", sidebarType.secondary)}>{visibleSources.length}</span>
      </div>
      <div className="space-y-1">
        {visibleSources.length ? (
          visibleSources.map((source) => {
            const Icon = source.icon;
            const active = activeSourceId === source.id;
            return (
              <button
                key={source.id}
                type="button"
                aria-pressed={active}
                onClick={() => onSelect(source.id)}
                className={cn(
                  "group flex min-h-10 w-full items-start gap-2.5 rounded-[9px] p-2 text-left transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] dark:hover:bg-white/5 dark:hover:text-white",
                  active ? "bg-white text-[#111827] shadow-[0_1px_2px_rgba(16,24,40,0.06)] dark:bg-[#202229] dark:text-white" : "text-[#3F4652] dark:text-[#B7BEC9]",
                  sidebarFocusClass,
                )}
              >
                <Icon className={cn("mt-0.5 shrink-0 text-[#6F7786]", sidebarType.icon)} strokeWidth={1.75} />
                <span className="min-w-0 flex-1">
                  <span className={cn("block truncate", sidebarType.rowNormal)}>{source.title}</span>
                  <span className={cn("mt-0.5 block truncate text-[#8B95A7] dark:text-[#8F96A3]", sidebarType.secondary)}>
                    {source.chunks} chunks · {source.coverage}
                  </span>
                </span>
              </button>
            );
          })
        ) : (
          <KnowledgeSidebarEmptyState label="No sources found." />
        )}
      </div>
    </section>
  );
}

function KnowledgeSidebarEmptyState({ label }: { label: string }) {
  return (
    <p className={cn("rounded-[10px] p-2 text-[#8B95A7]", sidebarType.secondary)}>
      {label}
    </p>
  );
}

function KnowledgeSidebarTreeItem({
  activeFolderId,
  depth,
  folder,
  onSelect,
}: {
  activeFolderId: string;
  depth: number;
  folder: KnowledgeSidebarFolder;
  onSelect: (folderId: string) => void;
}) {
  const Icon = depth === 0 ? knowledgeCollections.find((collection) => collection.id === folder.id)?.icon ?? FolderIcon : FolderIcon;
  const active = activeFolderId === folder.id;

  return (
    <div>
      <button
        type="button"
        aria-pressed={active}
        onClick={() => onSelect(folder.id)}
        className={cn(
          "flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] dark:hover:bg-white/5 dark:hover:text-white",
          active ? "bg-white text-[#111827] shadow-[0_1px_2px_rgba(16,24,40,0.06)] dark:bg-[#202229] dark:text-white" : "text-[#3F4652] dark:text-[#B7BEC9]",
          sidebarFocusClass,
        )}
      >
        <Icon className={cn("shrink-0 text-[#6F7786]", sidebarType.icon)} strokeWidth={1.75} />
        <span className={cn("min-w-0 flex-1 truncate", depth === 0 ? sidebarType.rowStrong : sidebarType.rowNormal)}>{folder.label}</span>
        <span className="grid h-5 min-w-5 place-items-center rounded-full bg-[#ECEFF4] px-1.5 text-[11px] font-semibold text-[#6F7786] dark:bg-[#202229] dark:text-[#B7BEC9]">
          {folder.count}
        </span>
      </button>
      {folder.children?.length ? (
        <div className={cn("relative mb-2 ml-[30px] mt-1.5 space-y-1", depth > 0 ? "ml-6" : "")}>
          <div className="absolute bottom-1 left-0 top-1 w-px bg-[#DDE1E8] dark:bg-[#2A2C31]" />
          {folder.children.map((child) => (
            <div key={child.id} className="pl-3">
              <KnowledgeSidebarTreeItem activeFolderId={activeFolderId} folder={child} depth={depth + 1} onSelect={onSelect} />
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function filterKnowledgeFolders(
  folders: readonly KnowledgeSidebarFolder[],
  normalizedSearch: string,
): readonly KnowledgeSidebarFolder[] {
  if (!normalizedSearch) {
    return folders;
  }

  return folders.reduce<KnowledgeSidebarFolder[]>((matches, folder) => {
    const children = folder.children ? filterKnowledgeFolders(folder.children, normalizedSearch) : [];
    const folderMatches = folder.label.toLowerCase().includes(normalizedSearch);

    if (!folderMatches && children.length === 0) {
      return matches;
    }

    return [
      ...matches,
      {
        ...folder,
        children: children.length ? children : folder.children,
      },
    ];
  }, []);
}

function FolderIcon({ className, strokeWidth }: { className?: string; strokeWidth?: number }) {
  return <BookOpen className={className} strokeWidth={strokeWidth} />;
}

function getVisibleKnowledgeTags(normalizedSearch: string) {
  const tagCounts = new Map<string, number>();

  for (const source of knowledgeSources) {
    for (const tag of source.tags) {
      if (normalizedSearch && !tag.toLowerCase().includes(normalizedSearch)) continue;
      tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
    }
  }

  return Array.from(tagCounts, ([label, count]) => ({ label, count }));
}
