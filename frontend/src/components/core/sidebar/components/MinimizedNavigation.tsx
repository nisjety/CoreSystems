'use client';

import { usePathname, useRouter } from 'next/navigation';
import clsx from 'clsx';
import { useCompatibleLanguage } from '../../contexts/GlobalLanguageContext';
import { useSidebar } from '../../shared/SidebarContext';
import { useDashboardSearch } from '@/components/dashboard/DashboardSearchContext';
import { getActiveSidebarSection, getNavLabel, sharedNavItems } from '../config/nav-items';
import { SimpleTooltip } from '../ui/simple-tooltip';

export function MinimizedNavigation() {
  const pathname = usePathname();
  const router = useRouter();
  const { sidebarLocale } = useCompatibleLanguage();
  const { expandSidebar } = useSidebar();
  const { isGlobalSearchOpen, openGlobalSearch } = useDashboardSearch();

  const allItems = sharedNavItems.map((item) => ({
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

  function NavItem({ item }: { item: typeof allItems[0] }) {
    const Icon = item.icon;
    const isSearchItem = item.id === 'search';
    const isActive = item.id === activeId || (isSearchItem && isGlobalSearchOpen);

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

  return (
    <nav className="flex h-full w-full flex-col items-center px-3">
      {/* Main nav group — top */}
      <div className="flex w-full flex-col items-center gap-0.5">
        {mainItems.map((item) => (
          <NavItem key={item.id} item={item} />
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
              <NavItem key={item.id} item={item} />
            ))}
          </div>
        </>
      )}
    </nav>
  );
}
