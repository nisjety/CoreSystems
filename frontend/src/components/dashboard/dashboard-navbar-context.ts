import {
  getActiveSidebarItem,
  getActiveSidebarSection,
  getNavLabel,
  isNavPathActive,
  sharedNavItems,
} from '@/components/core/sidebar/config/nav-items';
import { sidebarSections } from './sidebar-sections';

export type DashboardNavbarContext = {
  moduleLabel: string;
  moduleHref: string;
  tabLabel: string;
  tabHref: string;
};

function humanizeSegment(value: string): string {
  return value
    .split('-')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

export function resolveDashboardNavbarContext(pathname: string, locale: string): DashboardNavbarContext {
  const activeSection = getActiveSidebarSection(pathname) ?? sharedNavItems[0];
  const activeItem = getActiveSidebarItem(pathname);
  const activeSubItem = activeSection.panelGroups
    .flatMap((group) => group.items)
    .flatMap((item) => item.subItems ?? [])
    .find((subItem) => isNavPathActive(pathname, subItem.href));

  const moduleLabel = getNavLabel(activeSection.labelKey, activeSection.defaultLabel, locale);
  const moduleHref = activeSection.href;

  if (activeSubItem) {
    return {
      moduleLabel,
      moduleHref,
      tabLabel: getNavLabel(activeSubItem.labelKey, activeSubItem.defaultLabel, locale),
      tabHref: activeSubItem.href,
    };
  }

  if (activeItem) {
    return {
      moduleLabel,
      moduleHref,
      tabLabel: getNavLabel(activeItem.labelKey, activeItem.defaultLabel, locale),
      tabHref: activeItem.href ?? activeSection.href,
    };
  }

  const matchingSectionEntry = Object.entries(sidebarSections).find(([, section]) => (
    section.views.some((view) => isNavPathActive(pathname, view.href))
  ));

  if (matchingSectionEntry) {
    const [, section] = matchingSectionEntry;
    const currentView = section.views.find((view) => isNavPathActive(pathname, view.href)) ?? section.views[0];

    return {
      moduleLabel: section.eyebrow,
      moduleHref: section.views[0]?.href ?? pathname,
      tabLabel: currentView.label,
      tabHref: currentView.href,
    };
  }

  const segments = pathname.split('/').filter(Boolean);
  const lastSegment = segments.at(-1) ?? 'dashboard';

  return {
    moduleLabel,
    moduleHref,
    tabLabel: humanizeSegment(lastSegment),
    tabHref: pathname,
  };
}