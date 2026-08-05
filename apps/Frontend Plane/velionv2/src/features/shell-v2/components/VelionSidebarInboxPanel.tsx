"use client";

import Link from "next/link";
import type { Route } from "next";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect, useState } from "react";
import { ChevronDown, ChevronRight, Plus } from "lucide-react";
import {
  inboxSidebarGroups,
  searchParamsFromInboxSlug,
  type SidebarGroup as InboxNavGroup,
  type SidebarItem as InboxNavItem,
  type SidebarSubItem as InboxNavSubItem,
} from "@/features/inbox-v2/lib/inbox-model";
import {
  SidebarPanelTitle,
  SidebarSearchField,
} from "@/features/shell-v2/components/VerevonSidebarPrimitives";
import { sidebarType } from "@/features/shell-v2/lib/sidebar-style";
import { cn } from "@/lib/utils";

export function InboxExpandedSidebarPanel({ onCollapse }: { onCollapse: () => void }) {
  return (
    <Suspense fallback={<InboxExpandedSidebarPanelFallback onCollapse={onCollapse} />}>
      <InboxExpandedSidebarPanelContent onCollapse={onCollapse} />
    </Suspense>
  );
}

function InboxExpandedSidebarPanelFallback({ onCollapse }: { onCollapse: () => void }) {
  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <SidebarPanelTitle onCollapse={onCollapse}>Inbox</SidebarPanelTitle>
      <SidebarSearchField
        ariaLabel="Filter inbox section"
        className="mb-5"
        value=""
        onChange={() => undefined}
      />
      <nav className="min-h-0 flex-1 overflow-y-auto pr-1" aria-label="Inbox navigation" />
    </div>
  );
}

function InboxExpandedSidebarPanelContent({ onCollapse }: { onCollapse: () => void }) {
  const { push } = useRouter();
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const activeSearchParams = (() => {
    const queryString = searchParams.toString();
    if (queryString) return new URLSearchParams(queryString);
    const slug = pathname.startsWith("/inbox/") ? pathname.split("/").slice(2).filter(Boolean) : [];
    return searchParamsFromInboxSlug(slug);
  })();
  const activeView = activeSearchParams.get("view") ?? "mine";
  const activeChannel = activeSearchParams.get("channel");
  const [searchQuery, setSearchQuery] = useState("");
  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>(() => (
    Object.fromEntries(inboxSidebarGroups.map((group) => [group.id, group.defaultExpanded]))
  ));
  const [expandedItems, setExpandedItems] = useState<Record<string, boolean>>(() => (
    getDefaultInboxExpandedItems()
  ));
  const normalizedSearch = searchQuery.trim().toLowerCase();
  const visibleGroups = getVisibleInboxGroups(normalizedSearch);
  const topVisibleGroups = [];
  const bottomVisibleGroups = [];

  for (const group of visibleGroups) {
    if (group.alignBottom) {
      bottomVisibleGroups.push(group);
    } else {
      topVisibleGroups.push(group);
    }
  }

  const activeGroupId = (
    inboxSidebarGroups.find((group) => group.items.some((item) => (
      isInboxItemActive(item, activeView, activeChannel) ||
      item.subItems?.some((subItem) => isInboxSubItemActive(subItem, activeView, activeChannel))
    )))?.id ?? null
  );

  const activeDropdownItemId = (
    inboxSidebarGroups
      .flatMap((group) => group.items)
      .find((item) => (
        item.subItems?.length &&
        (
          isInboxItemActive(item, activeView, activeChannel) ||
          item.subItems.some((subItem) => isInboxSubItemActive(subItem, activeView, activeChannel))
        )
      ))?.id ?? null
  );

  useEffect(() => {
    if (!activeGroupId) return;

    const timeout = window.setTimeout(() => {
      setExpandedGroups((current) => (
        current[activeGroupId] ? current : { ...current, [activeGroupId]: true }
      ));
    }, 0);

    return () => window.clearTimeout(timeout);
  }, [activeGroupId]);

  useEffect(() => {
    if (!activeDropdownItemId) return;

    const timeout = window.setTimeout(() => {
      setExpandedItems((current) => (
        current[activeDropdownItemId] ? current : { ...current, [activeDropdownItemId]: true }
      ));
    }, 0);

    return () => window.clearTimeout(timeout);
  }, [activeDropdownItemId]);

  function toggleGroup(groupId: string) {
    setExpandedGroups((current) => ({
      ...current,
      [groupId]: !current[groupId],
    }));
  }

  function toggleItem(itemId: string) {
    setExpandedItems((current) => ({
      ...current,
      [itemId]: !current[itemId],
    }));
  }

  return (
    <div className="flex min-w-0 flex-1 flex-col bg-[#F7F7F8] px-5 pb-5 pt-6 dark:bg-[#101114]">
      <SidebarPanelTitle onCollapse={onCollapse}>Inbox</SidebarPanelTitle>

      <SidebarSearchField
        ariaLabel="Filter inbox section"
        className="mb-5"
        value={searchQuery}
        onChange={setSearchQuery}
      />

      <nav className="min-h-0 flex-1 overflow-y-auto pr-1" aria-label="Inbox navigation">
        <div className="flex min-h-full flex-col gap-5">
          {topVisibleGroups.map((group) => (
            <InboxSidebarGroup
              key={group.id}
              activeChannel={activeChannel}
              activeView={activeView}
              group={group}
              expanded={normalizedSearch ? true : expandedGroups[group.id] ?? group.defaultExpanded}
              expandedItems={expandedItems}
              forceExpandItems={Boolean(normalizedSearch)}
              onAdd={() => {
                if (group.id === "inbox-ai-agent") push("/agents");
                if (group.id === "inbox-teammates") setExpandedGroups((current) => ({ ...current, [group.id]: true }));
              }}
              onToggle={() => toggleGroup(group.id)}
              onToggleItem={toggleItem}
            />
          ))}
          <div className="flex-1" />
          {bottomVisibleGroups.map((group) => (
            <InboxSidebarGroup
              key={group.id}
              activeChannel={activeChannel}
              activeView={activeView}
              group={group}
              expanded={normalizedSearch ? true : expandedGroups[group.id] ?? group.defaultExpanded}
              expandedItems={expandedItems}
              forceExpandItems={Boolean(normalizedSearch)}
              onToggle={() => toggleGroup(group.id)}
              onToggleItem={toggleItem}
            />
          ))}
        </div>
      </nav>
    </div>
  );
}

function InboxSidebarGroup({
  activeChannel,
  activeView,
  expanded,
  expandedItems,
  forceExpandItems,
  group,
  onAdd,
  onToggleItem,
  onToggle,
}: {
  activeChannel: string | null;
  activeView: string;
  expanded: boolean;
  expandedItems: Record<string, boolean>;
  forceExpandItems: boolean;
  group: InboxNavGroup;
  onAdd?: () => void;
  onToggleItem: (itemId: string) => void;
  onToggle: () => void;
}) {
  return (
    <section>
      <div className="mb-3 flex items-center justify-between px-1">
        <button
          type="button"
          onClick={onToggle}
          className="flex min-w-0 items-center gap-2 text-left"
          aria-expanded={expanded}
        >
          <h2 className={cn("truncate text-[#1C1C1E] dark:text-white", sidebarType.groupTitle)}>{group.label}</h2>
          <ChevronDown
            className={cn("size-4 shrink-0 text-[#17181C] transition-transform dark:text-[#C9D0DC]", expanded ? "" : "-rotate-90")}
            strokeWidth={2.1}
          />
        </button>
        {group.showAddButton ? (
          <button
            type="button"
            onClick={onAdd}
            className="grid size-7 place-items-center rounded-full border border-[#DDE1E8] bg-white text-[#111827] shadow-[0_1px_3px_rgba(16,24,40,0.12)] transition-colors hover:bg-[#F8FAFC] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:border-[#303238] dark:bg-[#17181C] dark:text-white"
            aria-label={`Add ${group.label.toLowerCase()}`}
            title={`Add ${group.label.toLowerCase()}`}
          >
            <Plus className="size-4" strokeWidth={1.9} />
          </button>
        ) : null}
      </div>

      {expanded ? (
        group.items.length ? (
          <div className="space-y-1">
            {group.items.map((item) => (
              <InboxSidebarItem
                key={item.id}
                active={isInboxItemActive(item, activeView, activeChannel)}
                activeChannel={activeChannel}
                activeView={activeView}
                expanded={forceExpandItems || Boolean(expandedItems[item.id])}
                item={item}
                onToggle={() => onToggleItem(item.id)}
              />
            ))}
          </div>
        ) : (
          <p className={cn("rounded-[10px] p-2 text-[#8B95A7]", sidebarType.secondary)}>{group.emptyLabel ?? "Ingen elementer."}</p>
        )
      ) : null}
    </section>
  );
}

function InboxSidebarItem({
  active,
  activeChannel,
  activeView,
  expanded,
  item,
  onToggle,
}: {
  active: boolean;
  activeChannel: string | null;
  activeView: string;
  expanded: boolean;
  item: InboxNavItem;
  onToggle: () => void;
}) {
  const Icon = item.icon;
  const hasSubItems = Boolean(item.subItems?.length);
  const subNavigationId = `inbox-sidebar-${item.id}-subitems`;

  if (hasSubItems) {
    return (
      <div>
        <div
          className={cn(
            "group flex h-9 w-full items-center rounded-[9px] transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] dark:hover:bg-white/5 dark:hover:text-white",
            active ? "bg-[#EFF1F5] text-[#111827] dark:bg-[#202229] dark:text-white" : "text-[#3F4652] dark:text-[#B7BEC9]",
          )}
        >
          <Link
            href={item.href as Route}
            aria-current={active ? "page" : undefined}
            className="flex h-full min-w-0 flex-1 items-center gap-2.5 rounded-l-[9px] px-2 text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB]"
          >
            <Icon className={cn("shrink-0 text-[#6F7786]", sidebarType.icon)} strokeWidth={1.75} />
            <span className={cn("min-w-0 flex-1 truncate", sidebarType.rowNormal)}>{item.label}</span>
            {item.badge ? (
              <span className="grid h-5 min-w-5 place-items-center rounded-full bg-[#FFE1C2] px-1.5 text-[11px] font-semibold text-[#B45713]">{item.badge}</span>
            ) : null}
          </Link>
          <button
            type="button"
            onClick={onToggle}
            aria-controls={subNavigationId}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Hide" : "Show"} ${item.label}`}
            title={`${expanded ? "Hide" : "Show"} ${item.label}`}
            className="mr-1 grid size-7 shrink-0 place-items-center rounded-[8px] text-[#9AA2AF] transition-colors hover:bg-white/70 hover:text-[#4F5663] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:hover:bg-white/10 dark:hover:text-white"
          >
            <ChevronRight className={cn("size-4 transition-transform", expanded ? "rotate-90" : "")} strokeWidth={1.9} />
          </button>
        </div>
        {expanded ? (
          <nav id={subNavigationId} className="relative mb-2 ml-[34px] mt-1.5" aria-label={`${item.label} subnavigation`}>
            <div className="absolute bottom-1 left-0 top-1 w-px bg-[#DDE1E8] dark:bg-[#2A2C31]" />
            {item.subItems?.map((subItem) => (
              <InboxSidebarSubItem
                key={subItem.id}
                active={isInboxSubItemActive(subItem, activeView, activeChannel)}
                item={subItem}
              />
            ))}
          </nav>
        ) : null}
      </div>
    );
  }

  return (
    <div>
      <Link
        href={item.href as Route}
        aria-current={active ? "page" : undefined}
        className={cn(
          "flex h-9 w-full items-center gap-2.5 rounded-[9px] px-2 text-left transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:hover:bg-white/5 dark:hover:text-white",
          active ? "bg-[#EFF1F5] text-[#111827] dark:bg-[#202229] dark:text-white" : "text-[#3F4652] dark:text-[#B7BEC9]",
        )}
      >
        <Icon className={cn("shrink-0 text-[#6F7786]", sidebarType.icon)} strokeWidth={1.75} />
        <span className={cn("min-w-0 flex-1 truncate", sidebarType.rowNormal)}>{item.label}</span>
        {item.badge ? (
          <span className="grid h-5 min-w-5 place-items-center rounded-full bg-[#FFE1C2] px-1.5 text-[11px] font-semibold text-[#B45713]">{item.badge}</span>
        ) : null}
        {item.trailing ? <ChevronRight className="size-4 shrink-0 text-[#9AA2AF]" strokeWidth={1.9} /> : null}
      </Link>
    </div>
  );
}

function InboxSidebarSubItem({ active, item }: { active: boolean; item: InboxNavSubItem }) {
  return (
    <Link
      href={item.href as Route}
      className={cn(
        "block w-full rounded-[7px] py-[6px] pl-4 pr-2 text-left transition-colors hover:bg-[#F0F2F5] hover:text-[#111827] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#2563EB] dark:hover:bg-white/5 dark:hover:text-white",
        sidebarType.rowNormal,
        active ? "text-[#111827] dark:text-white" : "text-[#4F5663] dark:text-[#B7BEC9]",
      )}
    >
      {item.label}
    </Link>
  );
}

function isInboxItemActive(item: InboxNavItem, activeView: string, activeChannel: string | null) {
  if (item.id === "mine") return activeView === "mine";
  if (item.id === "mentions") return activeView === "mentions";
  return !activeChannel && activeView === item.id;
}

function isInboxSubItemActive(item: InboxNavSubItem, activeView: string, activeChannel: string | null) {
  const href = new URL(item.href, "https://verevon.local");
  const itemView = href.searchParams.get("view") ?? "mine";
  const itemChannel = href.searchParams.get("channel");
  return activeView === itemView && (activeChannel ?? null) === (itemChannel ?? null);
}

function getDefaultInboxExpandedItems() {
  const expandedItems: Record<string, boolean> = {};

  for (const group of inboxSidebarGroups) {
    for (const item of group.items) {
      if (item.subItems?.length) {
        expandedItems[item.id] = item.id === "mine";
      }
    }
  }

  return expandedItems;
}

function getVisibleInboxGroups(normalizedSearch: string) {
  const groups = [];

  for (const group of inboxSidebarGroups) {
    const items = normalizedSearch
      ? group.items.filter((item) => (
        item.label.toLowerCase().includes(normalizedSearch) ||
        item.subItems?.some((subItem) => subItem.label.toLowerCase().includes(normalizedSearch))
      ))
      : group.items;

    if (items.length > 0 || group.emptyLabel) {
      groups.push({ ...group, items });
    }
  }

  return groups;
}
