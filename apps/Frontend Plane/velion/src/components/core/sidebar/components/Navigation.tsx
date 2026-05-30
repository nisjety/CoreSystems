'use client';

import React from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import { ChevronDown, ChevronRight, Plus } from 'lucide-react';
import { cn } from '../utils';
import {
  getNavLabel,
  isNavItemVisibleInCurrentBuild,
  isNavPathActive,
  type NavSubItem,
  type SharedNavItem,
  type SharedNavPanelGroup,
  type SharedNavPanelItem,
} from '../config/nav-items';
import { useCompatibleLanguage } from '@/components/core/contexts/GlobalLanguageContext';

interface ResolvedSubItem extends NavSubItem {
  label: string;
  badge?: string;
}

interface ResolvedPanelItem extends SharedNavPanelItem {
  label: string;
  subItems?: ResolvedSubItem[];
}

interface ResolvedPanelGroup extends Omit<SharedNavPanelGroup, 'items'> {
  label: string;
  items: ResolvedPanelItem[];
}

function NavPanelTabs({
  tabs,
  activeTabId,
  language,
  onTabChange,
}: {
  tabs: NonNullable<SharedNavItem['panelTabs']>;
  activeTabId: string | null;
  language: Parameters<typeof getNavLabel>[2];
  onTabChange: (id: string) => void;
}) {
  return (
    <div className="mb-4 flex items-center gap-5 border-b border-[#ECECF1] text-[14px] font-semibold tracking-[-0.015em]">
      {tabs.map((tab) => {
        const label = getNavLabel(tab.labelKey, tab.defaultLabel, language);
        const isActiveTab = activeTabId === tab.id;

        return (
          <button
            key={tab.id}
            type="button"
            onClick={() => onTabChange(tab.id)}
            className={cn(
              'pb-2.5 transition-colors',
              isActiveTab ? 'border-b-2 border-[#1C1C1E] text-[#1C1C1E]' : 'text-[#9B9EA8] hover:text-[#4A4C54]'
            )}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
}

function NavItem({
  item,
  isActive,
  isExpanded,
  pathname,
  onToggle,
  onNavigate,
  onPrefetch,
}: {
  item: ResolvedPanelItem;
  isActive: boolean;
  isExpanded: boolean;
  pathname: string | null;
  onToggle: () => void;
  onNavigate: (item: SharedNavPanelItem) => void;
  onPrefetch?: (href: string) => void;
}) {
  const Icon = item.icon;
  const isDisabled = item.status === 'coming-soon';
  const hasSubItems = Boolean(item.subItems?.length);

  return (
    <div>
      <button
        type="button"
        aria-expanded={hasSubItems ? isExpanded : undefined}
        onMouseEnter={() => {
          if (item.href) onPrefetch?.(item.href);
        }}
        onFocus={() => {
          if (item.href) onPrefetch?.(item.href);
        }}
        onClick={() => {
          if (isDisabled) {
            return;
          }

          if (hasSubItems) {
            onToggle();
            return;
          }

          if (item.href) {
            onNavigate(item);
          }
        }}
        disabled={isDisabled}
        className={cn(
          'group flex w-full items-center gap-3 rounded-xl px-2 py-2.5 text-left transition-colors duration-150',
          isActive
            ? 'bg-[#F0F1F5] text-[#1C1C1E]'
            : isDisabled
              ? 'text-[#C0C3CC]'
              : 'text-[#3A3C44] hover:bg-[#F6F7F9] hover:text-[#1C1C1E]'
        )}
      >
        <Icon
          className={cn(
            'h-4 w-4 shrink-0 transition-colors',
            isActive ? 'text-[#1C1C1E]' : isDisabled ? 'text-[#C0C3CC]' : 'text-[#6B6E78] group-hover:text-[#3A3C44]'
          )}
          strokeWidth={1.7}
        />

        <span className={cn(
          'flex-1 text-[14px] leading-5 tracking-[-0.01em]',
          isActive ? 'font-medium' : 'font-normal'
        )}>
          {item.label}
        </span>

        {isDisabled ? (
          <span className="text-[11px] font-medium text-[#B0B3BC]">Soon</span>
        ) : item.badge ? (
          <span className="rounded-full bg-[#F4E1CE] px-1.5 py-0.5 text-[11px] font-semibold text-[#8A4C16]">
            {item.badge}
          </span>
        ) : hasSubItems ? (
          <span className="flex h-5 w-5 items-center justify-center rounded text-[#9B9EA8] transition-colors group-hover:text-[#5A5C64]">
            {isExpanded
              ? <ChevronDown className="h-3.5 w-3.5" strokeWidth={2.2} />
              : <ChevronRight className="h-3.5 w-3.5" strokeWidth={2.2} />
            }
          </span>
        ) : null}
      </button>

      {isExpanded && hasSubItems && !isDisabled ? (
        <div className="relative ml-9 mb-1 mt-0.5" role="group" aria-label={`${item.label} sub-navigation`}>
          <div className="absolute bottom-1 left-0 top-1 w-px bg-[#E2E3E9]" />
          {(item.subItems ?? []).map((sub) => {
            const isActiveSub = isNavPathActive(pathname ?? '', sub.href);
            return (
              <Link
                key={sub.id}
                href={sub.href}
                className={cn(
                  'flex w-full items-center justify-between rounded-lg py-1.5 pl-4 pr-2 text-[13px] text-left transition-colors',
                  isActiveSub
                    ? 'bg-[#F0F1F5] font-medium text-[#1C1C1E]'
                    : 'text-[#4A4C54] hover:bg-[#F4F5F8] hover:text-[#1C1C1E]'
                )}
              >
                <span>{sub.label}</span>
                {sub.badge === 'live' ? (
                  <span className="size-2 shrink-0 rounded-full bg-amber-400" />
                ) : null}
              </Link>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}

function GroupHeader({
  group,
  isExpanded,
  onToggle,
}: {
  group: ResolvedPanelGroup;
  isExpanded: boolean;
  onToggle: () => void;
}) {
  if (group.showHeader === false) {
    return null;
  }

  return (
    <div className="mb-2 flex items-center justify-between px-1">
      <div className="flex items-center gap-2">
        <span className="text-[14px] font-semibold tracking-[-0.01em] text-[#1C1C1E]">{group.label}</span>
        {group.showAddButton ? (
          <button
            type="button"
            aria-label={`Add to ${group.label}`}
            className="flex h-7 w-7 items-center justify-center rounded-full border border-[#E6E7EC] bg-white text-[#1C1C1E] shadow-[0_1px_2px_rgba(28,28,30,0.06)] transition-colors hover:bg-[#F7F7F9]"
          >
            <Plus className="h-3.5 w-3.5" strokeWidth={2.2} />
          </button>
        ) : null}
      </div>

      {group.collapsible ? (
        <button
          type="button"
          aria-label={`${isExpanded ? 'Collapse' : 'Expand'} ${group.label}`}
          aria-expanded={isExpanded}
          onClick={onToggle}
          className="flex h-7 w-7 items-center justify-center rounded-full text-[#1C1C1E] transition-colors hover:bg-[#F4F5F8]"
        >
          {isExpanded
            ? <ChevronDown className="h-4 w-4" strokeWidth={2.1} />
            : <ChevronRight className="h-4 w-4" strokeWidth={2.1} />
          }
        </button>
      ) : null}
    </div>
  );
}

interface NavigationProps {
  section: SharedNavItem;
  activeItem?: string;
  onNavigate: (item: SharedNavPanelItem) => void;
  onPrefetch?: (href: string) => void;
  className?: string;
  searchQuery?: string;
}

function normalizeSearchText(value: string): string {
  return value
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase();
}

export function Navigation({
  section,
  activeItem,
  onNavigate,
  onPrefetch,
  className,
  searchQuery = '',
}: NavigationProps) {
  const { sidebarLocale } = useCompatibleLanguage();
  const language = sidebarLocale;
  const pathname = usePathname();
  const normalizedSearchQuery = normalizeSearchText(searchQuery.trim());
  const isSearchActive = normalizedSearchQuery.length > 0;
  const [activeTabId, setActiveTabId] = React.useState<string | null>(() => section.panelTabs?.[0]?.id ?? null);

  React.useEffect(() => {
    const firstTabId = section.panelTabs?.[0]?.id ?? null;
    setActiveTabId(firstTabId);
  }, [section.id, section.panelTabs]);

  const visibleGroups = React.useMemo<ResolvedPanelGroup[]>(() => {
    return section.panelGroups.reduce<ResolvedPanelGroup[]>((accumulator, group) => {
      const resolvedItems = group.items.reduce<ResolvedPanelItem[]>((itemsAccumulator, item) => {
        // U7-1: drop mock-only items (status: 'coming-soon') from the
        // rendered sidebar unless NEXT_PUBLIC_VELION_PREVIEW_ROUTES is set.
        // The existing empty-group skip below removes any panel-group
        // whose items all get filtered out, so the layout stays clean.
        if (!isNavItemVisibleInCurrentBuild(item)) {
          return itemsAccumulator;
        }

        if (!isSearchActive && activeTabId && item.tabId && item.tabId !== activeTabId) {
          return itemsAccumulator;
        }

        const label = getNavLabel(item.labelKey, item.defaultLabel, language);
        const description = item.description ?? '';
        const itemMatches = !isSearchActive
          || [label, description, item.href ?? ''].some((field) => normalizeSearchText(field).includes(normalizedSearchQuery));

        if (!itemMatches) {
          return itemsAccumulator;
        }

        return [
          ...itemsAccumulator,
          {
            ...item,
            label,
            subItems: item.subItems?.map((subItem) => ({
              ...subItem,
              label: getNavLabel(subItem.labelKey, subItem.defaultLabel, language),
            })),
          },
        ];
      }, []);

      if (resolvedItems.length === 0 && !group.forceVisible) {
        return accumulator;
      }

      return [
        ...accumulator,
        {
          ...group,
          label: getNavLabel(group.labelKey, group.defaultLabel, language),
          items: resolvedItems,
        },
      ];
    }, []);
  }, [activeTabId, isSearchActive, language, normalizedSearchQuery, section.panelGroups]);

  const [expandedGroups, setExpandedGroups] = React.useState<Set<string>>(() => {
    return new Set(
      section.panelGroups
        .filter((group) => group.defaultExpanded)
        .map((group) => group.id)
    );
  });

  React.useEffect(() => {
    setExpandedGroups(new Set(
      section.panelGroups
        .filter((group) => group.defaultExpanded)
        .map((group) => group.id)
    ));
  }, [section.id, section.panelGroups]);

  const [expandedItems, setExpandedItems] = React.useState<Set<string>>(() => {
    const initial = new Set<string>();
    if (activeItem) {
      initial.add(activeItem);
    }
    return initial;
  });

  React.useEffect(() => {
    if (!activeItem) {
      return;
    }

    setExpandedItems((prev) => {
      if (prev.has(activeItem)) {
        return prev;
      }

      const next = new Set(prev);
      next.add(activeItem);
      return next;
    });
  }, [activeItem]);

  const toggleExpanded = React.useCallback((id: string) => {
    setExpandedItems((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const toggleGroupExpanded = React.useCallback((id: string) => {
    setExpandedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const topGroups = React.useMemo(
    () => visibleGroups.filter((group) => !group.alignBottom),
    [visibleGroups]
  );

  const bottomGroups = React.useMemo(
    () => visibleGroups.filter((group) => group.alignBottom),
    [visibleGroups]
  );

  const isGroupVisible = React.useCallback((group: ResolvedPanelGroup) => {
    if (!group.collapsible || isSearchActive || expandedGroups.has(group.id)) {
      return true;
    }

    return group.items.some((item) => {
      if (item.id === activeItem) {
        return true;
      }

      return item.subItems?.some((subItem) => isNavPathActive(pathname ?? '', subItem.href)) ?? false;
    });
  }, [activeItem, expandedGroups, isSearchActive, pathname]);

  return (
    <nav className={cn('flex h-full flex-col overflow-x-hidden', className)} aria-label={`${section.defaultLabel} navigation`}>
      {section.panelTabs?.length ? (
        <NavPanelTabs
          tabs={section.panelTabs}
          activeTabId={activeTabId}
          language={language}
          onTabChange={setActiveTabId}
        />
      ) : null}

      {visibleGroups.length === 0 ? (
        <div className="px-1 py-4 text-sm text-[#9B9EA8]">
          {isSearchActive ? 'No matching pages' : 'Nothing is configured here yet'}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col">
          <div>
          {topGroups.map((group) => (
            <section key={group.id} className="mb-4 last:mb-0">
              <GroupHeader group={group} isExpanded={expandedGroups.has(group.id)} onToggle={() => toggleGroupExpanded(group.id)} />

              {!isGroupVisible(group) ? null : (
                <div className="space-y-0">
                {group.items.map((item) => {
                  const isActive = activeItem === item.id;
                  const hasActiveSubItem = item.subItems?.some((subItem) => isNavPathActive(pathname ?? '', subItem.href)) ?? false;
                  const isExpanded = expandedItems.has(item.id) || isActive || hasActiveSubItem;
                  return (
                    <NavItem
                      key={item.id}
                      item={item}
                      isActive={isActive}
                      isExpanded={isExpanded}
                      pathname={pathname}
                      onToggle={() => toggleExpanded(item.id)}
                      onNavigate={onNavigate}
                      onPrefetch={onPrefetch}
                    />
                  );
                })}
                </div>
              )}
            </section>
          ))}
          </div>

          {bottomGroups.length ? (
            <div className="mt-auto border-t border-[#ECECF1] pt-4">
              {bottomGroups.map((group) => (
                <section key={group.id} className="mb-0">
                  <GroupHeader group={group} isExpanded={expandedGroups.has(group.id)} onToggle={() => toggleGroupExpanded(group.id)} />
                  {!isGroupVisible(group) ? null : (
                    <div className="space-y-0">
                      {group.items.map((item) => {
                        const isActive = activeItem === item.id;
                        const hasActiveSubItem = item.subItems?.some((subItem) => isNavPathActive(pathname ?? '', subItem.href)) ?? false;
                        const isExpanded = expandedItems.has(item.id) || isActive || hasActiveSubItem;
                        return (
                          <NavItem
                            key={item.id}
                            item={item}
                            isActive={isActive}
                            isExpanded={isExpanded}
                            pathname={pathname}
                            onToggle={() => toggleExpanded(item.id)}
                            onNavigate={onNavigate}
                            onPrefetch={onPrefetch}
                          />
                        );
                      })}
                    </div>
                  )}
                </section>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </nav>
  );
}
