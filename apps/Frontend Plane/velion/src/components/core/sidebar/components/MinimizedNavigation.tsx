'use client';

import { usePathname, useRouter } from 'next/navigation';
import clsx from 'clsx';
import { useCompatibleLanguage } from '../../contexts/GlobalLanguageContext';
import { useSidebar } from '../../shared/SidebarContext';
import { useDashboardSearch } from '@/components/dashboard/DashboardSearchContext';
import {
  getActiveSidebarSection,
  getNavLabel,
  isNavSectionVisibleInCurrentBuild,
  sharedNavItems,
} from '../config/nav-items';
import { SimpleTooltip } from '../ui/simple-tooltip';

type SearchTriggerSource = 'navbar' | 'sidebar';

type MinimizedNavItem = {
  id: string;
  href: string;
  icon: typeof sharedNavItems[number]['icon'];
  pinnedBottom: boolean;
  label: string;
};

interface NavItemProps {
  item: MinimizedNavItem;
  isActive: boolean;
  pathname: string;
  expandSidebar: () => void;
  openGlobalSearch: (source?: SearchTriggerSource, prefill?: string) => void;
  router: ReturnType<typeof useRouter>;
}

function NavItem({
  item,
  isActive,
  pathname,
  expandSidebar,
  openGlobalSearch,
  router,
}: NavItemProps) {
  const Icon = item.icon;
  const isSearchItem = item.id === 'search';

  const handleClick = () => {
    if (isSearchItem) {
      openGlobalSearch('sidebar');
      return;
    }

    expandSidebar();

    if (pathname !== item.href) {
      router.push(item.href);
    }
  };

  return (
    <SimpleTooltip content={item.label} placement="right" delay={200} containerClassName="block w-full">
      <button
        type="button"
        onClick={handleClick}
        onMouseEnter={() => {
          if (!isSearchItem) router.prefetch(item.href);
        }}
        onFocus={() => {
          if (!isSearchItem) router.prefetch(item.href);
        }}
        aria-label={item.label}
        className={clsx(
          'flex w-full items-center justify-center rounded-[10px] transition-colors duration-150',
          isActive
            ? 'text-[#1C1C1E]'
            : 'text-[#9B9EA8] hover:bg-[#EBEBEB] hover:text-[#3A3C44]'
        )}
        style={{ height: '36px' }}
      >
        <Icon
          className="h-[18px] w-[18px] transition-colors"
          strokeWidth={isActive ? 2 : 1.6}
        />
      </button>
    </SimpleTooltip>
  );
}

export function MinimizedNavigation() {
  const pathname = usePathname();
  const router = useRouter();
  const { sidebarLocale } = useCompatibleLanguage();
  const { expandSidebar } = useSidebar();
  const { isGlobalSearchOpen, openGlobalSearch } = useDashboardSearch();

  // U7-1: filter out sections whose landing page is mock-only OR whose
  // entire panel is `coming-soon`. The `isNavSectionVisibleInCurrentBuild`
  // helper short-circuits to `true` when the
  // `NEXT_PUBLIC_VELION_PREVIEW_ROUTES` flag is set, so stakeholder demos
  // still see every icon.
  const allItems = sharedNavItems
    .filter(isNavSectionVisibleInCurrentBuild)
    .map((item) => ({
      id: item.id,
      href: item.href,
      icon: item.icon,
      pinnedBottom: item.pinnedBottom ?? false,
      label: getNavLabel(item.labelKey, item.defaultLabel, sidebarLocale),
    }));

  const mainItems = allItems.filter((item) => !item.pinnedBottom);
  const pinnedItems = allItems.filter((item) => item.pinnedBottom);

  const activeSection = getActiveSidebarSection(pathname);
  const activeId = activeSection?.id;

  return (
    <nav className="flex h-full w-full flex-col items-center px-3">
      {/* Main nav group — top */}
      <div className="flex w-full flex-col items-center gap-0.5">
        {mainItems.map((item) => (
          <NavItem
            key={item.id}
            item={item}
            isActive={item.id === activeId || (item.id === 'search' && isGlobalSearchOpen)}
            pathname={pathname}
            expandSidebar={expandSidebar}
            openGlobalSearch={openGlobalSearch}
            router={router}
          />
        ))}
      </div>

      {/* Push pinned items to bottom */}
      <div className="flex-1" />

      {/* Divider + pinned bottom group */}
      {pinnedItems.length > 0 && (
        <>
          <div className="mb-2 w-6 border-t border-[#DDDDE0]" />
          <div className="flex w-full flex-col items-center gap-0.5">
            {pinnedItems.map((item) => (
              <NavItem
                key={item.id}
                item={item}
                isActive={item.id === activeId || (item.id === 'search' && isGlobalSearchOpen)}
                pathname={pathname}
                expandSidebar={expandSidebar}
                openGlobalSearch={openGlobalSearch}
                router={router}
              />
            ))}
          </div>
        </>
      )}
    </nav>
  );
}
