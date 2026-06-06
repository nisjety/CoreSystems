"use client";

import Link from "next/link";
import type { Route } from "next";
import { usePathname } from "next/navigation";
import { Suspense, useDeferredValue, useState, type ReactNode } from "react";
import {
  ChevronDown,
  ChevronRight,
  CircleUserRound,
  MessageSquarePlus,
  PanelLeftClose,
  PanelLeftOpen,
  Trash2,
} from "lucide-react";
import { AgentsExpandedSidebarPanel } from "@/features/shell-v2/components/VelionSidebarAgentsPanel";
import { InboxExpandedSidebarPanel } from "@/features/shell-v2/components/VelionSidebarInboxPanel";
import { KnowledgeExpandedSidebarPanel } from "@/features/shell-v2/components/VelionSidebarKnowledgePanel";
import {
  AccountExpandedSidebarPanel,
  SettingsExpandedSidebarPanel,
} from "@/features/shell-v2/components/VelionSidebarSettingsPanels";
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from "@/features/shell-v2/components/VelionSidebarPrimitives";
import { sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import {
  useVelionChatWorkspaceSafe,
  type ChatSession,
  type ChatWorkspaceValue,
} from "@/features/chat-v2/lib/chat-workspace";
import { formatRelative } from "@/features/chat-v2/lib/chat-format";
import type { VelionRoute } from "@/features/shell-v2/lib/shell-data";
import {
  getSidebarSectionForPath,
  isSidebarPathActive,
  sidebarSearchAction,
  sidebarSections,
  type SidebarPanelGroup,
  type SidebarPanelItem,
  type SidebarSection,
} from "@/features/shell-v2/lib/sidebar-navigation";
import { TopLayerTooltip } from "@/features/shell-v2/components/TopLayerTooltip";
import { cn } from "@/lib/utils";

export const SIDEBAR_MINIMIZED_WIDTH = 60;
export const SIDEBAR_EXPANDED_WIDTH = 320;

const MINI_NAV_BUTTON_CLASS =
  "relative flex h-9 w-full items-center justify-center rounded-[10px] transition-colors duration-150";

type VelionSidebarProps = {
  activeRoute: VelionRoute;
  expanded: boolean;
  expandedWidth?: number;
  expansionLocked?: boolean;
  onExpandedChange: (expanded: boolean) => void;
  onOpenSearch: () => void;
};

export function VelionSidebar({
  activeRoute,
  expanded,
  expandedWidth = SIDEBAR_EXPANDED_WIDTH,
  expansionLocked = false,
  onExpandedChange,
  onOpenSearch,
}: VelionSidebarProps) {
  const pathname = usePathname();
  const activeSection = getSidebarSectionForPath(pathname, activeRoute);
  const mainSections = sidebarSections.filter((section) => !section.pinnedBottom);
  const pinnedSections = sidebarSections.filter((section) => section.pinnedBottom);
  const accountActive = pathname === "/account" || pathname.startsWith("/account/");

  const openSection = () => onExpandedChange(true);

  return (
    <aside
      className={cn(
        "velion-sidebar-themed fixed bottom-0 left-0 top-14 z-[var(--velion-z-sidebar)] hidden overflow-hidden transition-[width,background-color] duration-300 ease-out md:block",
        sidebarType.root,
      )}
      style={{ width: expanded ? expandedWidth : SIDEBAR_MINIMIZED_WIDTH }}
      aria-label="Primary navigation"
    >
      <div className="relative z-10 flex h-full">
        <div className="flex h-full w-[60px] shrink-0 flex-col bg-transparent">
          <div className="flex flex-col items-center gap-1 px-3 pb-1 pt-3">
            {!expansionLocked ? (
              <MiniActionButton
                label={expanded ? "Collapse sidebar" : "Expand sidebar"}
                active={false}
                onClick={() => onExpandedChange(!expanded)}
              >
                {expanded ? (
                  <PanelLeftClose className="size-[18px]" strokeWidth={1.75} />
                ) : (
                  <PanelLeftOpen className="size-[18px]" strokeWidth={1.75} />
                )}
              </MiniActionButton>
            ) : null}
          </div>

          <nav className="flex min-h-0 flex-1 flex-col items-center p-3" aria-label="Workspace sections">
            <div className="flex w-full flex-col items-center gap-0.5">
              {mainSections.map((section) => (
                <MiniSectionLink
                  key={section.id}
                  section={section}
                  active={!accountActive && activeSection.id === section.id}
                  onOpen={openSection}
                />
              ))}
            </div>

            <div className="flex-1" />
          </nav>

          <div className="flex flex-col items-center gap-2 px-3 pb-16">
            <div className="mb-1 w-6 border-t border-[#DDDDE0] dark:border-[#2A2C31]" />
            <MiniAccountLink active={accountActive} onOpen={openSection}>
              <CircleUserRound className="size-[18px]" strokeWidth={1.65} />
            </MiniAccountLink>
            {pinnedSections.map((section) => (
              <MiniSectionLink
                key={section.id}
                section={section}
                active={!accountActive && activeSection.id === section.id}
                onOpen={openSection}
              />
            ))}
            <MiniActionButton
              label={sidebarSearchAction.label}
              active={false}
              onClick={() => {
                onOpenSearch();
                onExpandedChange(true);
              }}
            >
              <sidebarSearchAction.icon className="size-[18px]" strokeWidth={1.65} />
            </MiniActionButton>
          </div>
        </div>

        {expanded ? (
          <>
            <div
              aria-hidden="true"
              className="my-3 w-px shrink-0 self-stretch bg-gradient-to-b from-transparent via-[#D9DCE3]/80 to-transparent dark:via-[#2A2C31]"
            />
            <ExpandedSidebarPanel
              key={activeSection.id}
              activeSection={activeSection}
              pathname={pathname}
              onCollapse={() => onExpandedChange(false)}
            />
          </>
        ) : null}
      </div>

    </aside>
  );
}

function ExpandedSidebarPanel({
  activeSection,
  pathname,
  onCollapse,
}: {
  activeSection: SidebarSection;
  pathname: string;
  onCollapse: () => void;
}) {
  const chatWorkspace = useVelionChatWorkspaceSafe();
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearchQuery = useDeferredValue(searchQuery);
  const [activeTabId, setActiveTabId] = useState<string | null>(() => activeSection.panelTabs?.[0]?.id ?? null);

  if (activeSection.id === "messages" && chatWorkspace) {
    return (
      <ChatExpandedSidebarPanel
        chat={chatWorkspace}
        onCollapse={onCollapse}
      />
    );
  }

  if (activeSection.id === "inbox") {
    return (
      <Suspense fallback={null}>
        <InboxExpandedSidebarPanel onCollapse={onCollapse} />
      </Suspense>
    );
  }

  if (activeSection.id === "agents") {
    return <AgentsExpandedSidebarPanel onCollapse={onCollapse} />;
  }

  if (activeSection.id === "knowledge") {
    return <KnowledgeExpandedSidebarPanel onCollapse={onCollapse} />;
  }

  if (pathname === "/account" || pathname.startsWith("/account/")) {
    return <AccountExpandedSidebarPanel onCollapse={onCollapse} />;
  }

  if (activeSection.id === "settings") {
    return <SettingsExpandedSidebarPanel onCollapse={onCollapse} />;
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-transparent px-5 pb-5 pt-4">
      <SidebarPanelTitle onCollapse={onCollapse}>{activeSection.label}</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel="Filter sidebar section"
        className="mb-4"
        placeholder="Filtrer denne seksjonen"
        value={searchQuery}
        onChange={setSearchQuery}
      />

      {activeSection.panelTabs?.length ? (
        <div className={cn("mb-4 flex items-center gap-5 border-b border-[#ECECF1] dark:border-[#2A2C31]", sidebarType.rowStrong)}>
          {activeSection.panelTabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTabId(tab.id)}
              title={tab.label}
              className={cn(
                "pb-2.5 transition-colors",
                activeTabId === tab.id
                  ? "velion-sidebar-tab-active border-b-2 text-[#1C1C1E] dark:text-white"
                  : "text-[#9B9EA8] hover:text-[#4A4C54] dark:hover:text-[#D0D6E0]",
              )}
            >
              {tab.label}
            </button>
          ))}
        </div>
      ) : null}

      <SidebarPanelNavigation
        section={activeSection}
        pathname={pathname}
        activeTabId={activeTabId}
        searchQuery={deferredSearchQuery}
      />
    </div>
  );
}

function ChatExpandedSidebarPanel({
  chat,
  onCollapse,
}: {
  chat: ChatWorkspaceValue;
  onCollapse: () => void;
}) {
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearchQuery = useDeferredValue(searchQuery);
  const normalizedQuery = normalizeSearchText(deferredSearchQuery.trim());
  const filteredSessions = normalizedQuery
    ? chat.sessions.filter((session) => (
      normalizeSearchText(`${session.title} ${session.preview}`).includes(normalizedQuery)
    ))
    : chat.sessions;

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-transparent px-5 pb-5 pt-4">
      <SidebarPanelTitle onCollapse={onCollapse}>Chat</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel="Search conversations"
        className="mb-4"
        placeholder="Search conversations"
        value={searchQuery}
        onChange={setSearchQuery}
      />

      <button
        type="button"
        onClick={chat.startNewChat}
        className={cn(
          "mb-4 flex h-10 w-full items-center gap-2.5 rounded-[10px] bg-[#F0F1F5] px-3 text-left text-[#1C1C1E] transition-colors hover:bg-[#EAEBEF] dark:bg-[#23252A] dark:text-white dark:hover:bg-[#2A2D34]",
          sidebarType.rowStrong,
        )}
        aria-label="New chat"
        title="New chat"
      >
        <span className="grid size-7 shrink-0 place-items-center rounded-[9px] bg-white text-[#1C1C1E] shadow-sm dark:bg-[#17181C] dark:text-white">
          <MessageSquarePlus className={sidebarType.icon} strokeWidth={1.75} />
        </span>
        Ny samtale
      </button>

      <nav className="min-h-0 flex-1 overflow-y-auto" aria-label="Chat conversations">
        {filteredSessions.length > 0 ? (
          <div className="space-y-1">
            {filteredSessions.map((session) => (
              <ChatSessionButton
                key={session.id}
                active={session.id === chat.activeSessionId}
                session={session}
                onSelect={() => chat.selectSession(session.id)}
              />
            ))}
          </div>
        ) : (
          <div className={cn("rounded-[16px] border border-dashed border-[#DFE0E6] bg-white/55 p-4 text-[#7d828a] dark:border-[#2A2C31] dark:bg-[#17181C]/60 dark:text-[#8A909B]", sidebarType.secondary)}>
            {chat.sessions.length > 0 ? "No conversations match this search." : "No conversations yet."}
          </div>
        )}
      </nav>

      {chat.sessions.length > 0 ? (
        <button
          type="button"
          onClick={chat.clearHistory}
          className="mt-3 flex h-10 w-full items-center justify-center gap-2 rounded-[13px] text-[12px] font-semibold text-[#8A8D96] transition hover:bg-white hover:text-[#26282f] dark:text-[#7A808B] dark:hover:bg-[#181A1F] dark:hover:text-white"
          aria-label="Clear local history"
          title="Clear local history"
        >
          <Trash2 className="size-3.5" />
          Clear local history
        </button>
      ) : null}
    </div>
  );
}

function ChatSessionButton({
  active,
  session,
  onSelect,
}: {
  active: boolean;
  session: ChatSession;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        "w-full rounded-[14px] px-3 py-2.5 text-left transition-colors",
        active
          ? "bg-white shadow-sm ring-1 ring-[#E9E9EC] dark:bg-[#191B20] dark:ring-[#2A2C31]"
          : "hover:bg-white/70 dark:hover:bg-[#181A1F]",
      )}
    >
      <span className={cn("block truncate text-[#26282f] dark:text-white", sidebarType.rowStrong)}>{session.title}</span>
      <span className={cn("mt-1 block truncate text-[#7d828a] dark:text-[#8A909B]", sidebarType.secondary)}>{session.preview}</span>
      <span className="mt-2 block text-[10px] font-medium text-[#A1A5AE] dark:text-[#6F7682]">{formatRelative(session.updatedAt)}</span>
    </button>
  );
}

function SidebarPanelNavigation({
  section,
  pathname,
  activeTabId,
  searchQuery,
}: {
  section: SidebarSection;
  pathname: string;
  activeTabId: string | null;
  searchQuery: string;
}) {
  const normalizedSearchQuery = normalizeSearchText(searchQuery.trim());
  const isSearchActive = normalizedSearchQuery.length > 0;
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(() => getDefaultExpandedGroups(section));
  const [expandedItems, setExpandedItems] = useState<Set<string>>(() => getActiveItemIds(section, pathname));
  const activeItemIds = getActiveItemIds(section, pathname);

  const visibleGroups = section.panelGroups.reduce<SidebarPanelGroup[]>((groups, group) => {
    const items = group.items.filter((item) => {
      if (!isSearchActive && activeTabId && item.tabId && item.tabId !== activeTabId) {
        return false;
      }

      if (!isSearchActive) {
        return true;
      }

      return [item.label, item.description, item.href].some((value) => normalizeSearchText(value).includes(normalizedSearchQuery));
    });

    if (items.length === 0) {
      return groups;
    }

    return [...groups, { ...group, items }];
  }, []);

  const topGroups = visibleGroups.filter((group) => !group.alignBottom);
  const bottomGroups = visibleGroups.filter((group) => group.alignBottom);

  const toggleGroup = (groupId: string) => {
    setExpandedGroups((current) => {
      const next = new Set(current);
      if (next.has(groupId)) {
        next.delete(groupId);
      } else {
        next.add(groupId);
      }
      return next;
    });
  };

  const toggleItem = (itemId: string) => {
    setExpandedItems((current) => {
      const next = new Set(current);
      if (next.has(itemId)) {
        next.delete(itemId);
      } else {
        next.add(itemId);
      }
      return next;
    });
  };

  if (visibleGroups.length === 0) {
    return (
      <div className={cn("rounded-2xl border border-dashed border-[#E2E3E9] bg-white/70 px-4 py-6 text-center text-[#8A8D96] dark:border-[#2B2D33] dark:bg-[#17181C]/70 dark:text-[#AEB4C0]", sidebarType.rowNormal)}>
        Ingen treff i denne seksjonen.
      </div>
    );
  }

  return (
    <nav className="flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto" aria-label={`${section.label} navigation`}>
      <div>
        {topGroups.map((group) => (
          <SidebarGroup
            key={group.id}
            group={group}
            pathname={pathname}
            activeItemIds={activeItemIds}
            expandedGroups={expandedGroups}
            expandedItems={expandedItems}
            isSearchActive={isSearchActive}
            onToggleGroup={toggleGroup}
            onToggleItem={toggleItem}
          />
        ))}
      </div>

      {bottomGroups.length ? (
        <div className="mt-auto border-t border-[#ECECF1] pt-4 dark:border-[#2A2C31]">
          {bottomGroups.map((group) => (
            <SidebarGroup
              key={group.id}
              group={group}
              pathname={pathname}
              activeItemIds={activeItemIds}
              expandedGroups={expandedGroups}
              expandedItems={expandedItems}
              isSearchActive={isSearchActive}
              onToggleGroup={toggleGroup}
              onToggleItem={toggleItem}
            />
          ))}
        </div>
      ) : null}
    </nav>
  );
}

function SidebarGroup({
  group,
  pathname,
  activeItemIds,
  expandedGroups,
  expandedItems,
  isSearchActive,
  onToggleGroup,
  onToggleItem,
}: {
  group: SidebarPanelGroup;
  pathname: string;
  activeItemIds: Set<string>;
  expandedGroups: Set<string>;
  expandedItems: Set<string>;
  isSearchActive: boolean;
  onToggleGroup: (groupId: string) => void;
  onToggleItem: (itemId: string) => void;
}) {
  const groupExpanded = !group.collapsible || isSearchActive || expandedGroups.has(group.id);

  return (
    <section className="mb-4 last:mb-0">
      {group.showHeader === false ? null : (
        <div className="mb-2 flex items-center justify-between px-1">
          <span className={cn("text-[#1C1C1E] dark:text-[#F7F8F8]", sidebarType.groupTitle)}>{group.label}</span>
          {group.collapsible ? (
            <button
              type="button"
              onClick={() => onToggleGroup(group.id)}
              className="flex size-7 items-center justify-center rounded-full text-[#1C1C1E] transition-colors hover:bg-[#F4F5F8] dark:text-[#D0D6E0] dark:hover:bg-[#23252A]"
              aria-expanded={groupExpanded}
              aria-label={`${groupExpanded ? "Collapse" : "Expand"} ${group.label}`}
              title={`${groupExpanded ? "Collapse" : "Expand"} ${group.label}`}
            >
              {groupExpanded ? <ChevronDown className="size-4" strokeWidth={2.1} /> : <ChevronRight className="size-4" strokeWidth={2.1} />}
            </button>
          ) : null}
        </div>
      )}

      {groupExpanded ? (
        <div className="space-y-0">
          {group.items.map((item) => (
            <SidebarPanelLink
              key={item.id}
              item={item}
              pathname={pathname}
              expanded={expandedItems.has(item.id) || activeItemIds.has(item.id)}
              onToggle={() => onToggleItem(item.id)}
            />
          ))}
        </div>
      ) : null}
    </section>
  );
}

function SidebarPanelLink({
  item,
  pathname,
  expanded,
  onToggle,
}: {
  item: SidebarPanelItem;
  pathname: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  const Icon = item.icon;
  const active = isSidebarPathActive(pathname, item.href, item.aliases);
  const hasSubItems = Boolean(item.subItems?.length);

  if (hasSubItems) {
    return (
      <div>
        <button
          type="button"
          onClick={onToggle}
          className={cn(
            "group flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left transition-colors duration-150",
            active
              ? "velion-sidebar-panel-active text-[#1C1C1E] dark:text-white"
              : "text-[#3A3C44] hover:bg-[#F6F7F9] hover:text-[#1C1C1E] dark:text-[#D0D6E0] dark:hover:bg-[#191A1F] dark:hover:text-white",
          )}
          aria-expanded={expanded}
          title={item.label}
        >
          <PanelItemIcon icon={Icon} active={active} />
          <span className={cn("min-w-0 flex-1 truncate", active ? sidebarType.row : sidebarType.rowNormal)}>{item.label}</span>
          {expanded ? <ChevronDown className="size-3.5 text-[#9B9EA8]" strokeWidth={2.2} /> : <ChevronRight className="size-3.5 text-[#9B9EA8]" strokeWidth={2.2} />}
        </button>
        {expanded ? (
          <nav className="relative mb-1 ml-[34px] mt-1.5" aria-label={`${item.label} sub-navigation`}>
            <div className="absolute bottom-1 left-0 top-1 w-px bg-[#E2E3E9] dark:bg-[#2A2C31]" />
            {item.subItems?.map((subItem) => {
              const subActive = isSidebarPathActive(pathname, subItem.href);
              return (
                <Link
                  key={subItem.id}
                  href={subItem.href as Route}
                  aria-current={subActive ? "page" : undefined}
                  className={cn(
                    "flex w-full items-center rounded-[7px] py-[6px] pl-4 pr-2 text-left transition-colors",
                    subActive ? sidebarType.row : sidebarType.rowNormal,
                    subActive
                      ? "velion-sidebar-panel-active text-[#1C1C1E] dark:text-white"
                      : "text-[#4A4C54] hover:bg-[#F4F5F8] hover:text-[#1C1C1E] dark:text-[#AEB4C0] dark:hover:bg-[#191A1F] dark:hover:text-white",
                  )}
                >
                  {subItem.label}
                </Link>
              );
            })}
          </nav>
        ) : null}
      </div>
    );
  }

  return (
    <Link
      href={item.href as Route}
      aria-current={active ? "page" : undefined}
      className={cn(
        "group flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left transition-colors duration-150",
        active
          ? "velion-sidebar-panel-active text-[#1C1C1E] dark:text-white"
          : "text-[#3A3C44] hover:bg-[#F6F7F9] hover:text-[#1C1C1E] dark:text-[#D0D6E0] dark:hover:bg-[#191A1F] dark:hover:text-white",
      )}
    >
      <PanelItemIcon icon={Icon} active={active} />
      <span className={cn("min-w-0 flex-1 truncate", active ? sidebarType.row : sidebarType.rowNormal)}>{item.label}</span>
    </Link>
  );
}

function PanelItemIcon({ icon: Icon, active }: { icon: SidebarPanelItem["icon"]; active: boolean }) {
  return (
    <Icon
      className={cn(
        "shrink-0 transition-colors",
        sidebarType.icon,
        active ? "text-[#1C1C1E] dark:text-white" : "text-[#6B6E78] group-hover:text-[#3A3C44] dark:text-[#AEB4C0] dark:group-hover:text-white",
      )}
      strokeWidth={1.7}
    />
  );
}

function MiniSectionLink({
  section,
  active,
  onOpen,
}: {
  section: SidebarSection;
  active: boolean;
  onOpen: () => void;
}) {
  const Icon = section.icon;

  return (
    <MiniTooltip label={section.label}>
      <Link
        href={section.href as Route}
        onClick={onOpen}
        aria-current={active ? "page" : undefined}
        aria-label={section.label}
        title={section.label}
        className={cn(
          MINI_NAV_BUTTON_CLASS,
          active
            ? "velion-sidebar-mini-active"
            : "text-[#9B9EA8] hover:bg-[#EBEBEB] hover:text-[#3A3C44] dark:text-[#8A8F98] dark:hover:bg-[#191A1F] dark:hover:text-white",
        )}
      >
        <Icon className="size-[18px]" strokeWidth={active ? 2 : 1.6} />
      </Link>
    </MiniTooltip>
  );
}

function MiniAccountLink({
  active,
  children,
  onOpen,
}: {
  active: boolean;
  children: ReactNode;
  onOpen: () => void;
}) {
  return (
    <MiniTooltip label="Account">
      <Link
        href={"/account" as Route}
        onClick={onOpen}
        aria-current={active ? "page" : undefined}
        aria-label="Account"
        title="Account"
        className={cn(
          MINI_NAV_BUTTON_CLASS,
          active
            ? "velion-sidebar-mini-active"
            : "text-[#9B9EA8] hover:bg-[#EBEBEB] hover:text-[#3A3C44] dark:text-[#8A8F98] dark:hover:bg-[#191A1F] dark:hover:text-white",
        )}
      >
        {children}
      </Link>
    </MiniTooltip>
  );
}

function MiniActionButton({
  active,
  children,
  label,
  onClick,
}: {
  active: boolean;
  children: ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <MiniTooltip label={label}>
      <button
        type="button"
        onClick={onClick}
        aria-label={label}
        title={label}
        className={cn(
          MINI_NAV_BUTTON_CLASS,
          active
            ? "velion-sidebar-mini-active"
            : "text-[#9B9EA8] hover:bg-[#EBEBEB] hover:text-[#3A3C44] dark:text-[#8A8F98] dark:hover:bg-[#191A1F] dark:hover:text-white",
        )}
      >
        {children}
      </button>
    </MiniTooltip>
  );
}

function MiniTooltip({ children, label }: { children: ReactNode; label: string }) {
  return (
    <TopLayerTooltip className="w-full" label={label} placement="right">
      {children}
    </TopLayerTooltip>
  );
}

function getDefaultExpandedGroups(section: SidebarSection) {
  const expandedGroups = new Set<string>();

  for (const group of section.panelGroups) {
    if (group.defaultExpanded) expandedGroups.add(group.id);
  }

  return expandedGroups;
}

function getActiveItemIds(section: SidebarSection, pathname: string) {
  const activeItemIds = new Set<string>();

  for (const group of section.panelGroups) {
    for (const item of group.items) {
      if (isSidebarPathActive(pathname, item.href, item.aliases)) {
        activeItemIds.add(item.id);
      }
    }
  }

  return activeItemIds;
}

function normalizeSearchText(value: string) {
  return value
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase();
}
