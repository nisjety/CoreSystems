
'use client';

import { useCompatibleLanguage } from '@/components/core/contexts/GlobalLanguageContext';
import { cn } from '../utils';
import { getNavLabel, sharedNavItems } from '../config/nav-items';

export function ExpandedNavigation({ 
  activeItem,
  onNavigate 
}: { 
  activeItem?: string;
  onNavigate?: (item: string) => void;
}) {
  const { sidebarLocale } = useCompatibleLanguage();

  return (
    <div className="space-y-1 w-full">
      {sharedNavItems.map((item) => {
        const IconComponent = item.icon;
        const isActive = activeItem === item.id;
        const label = getNavLabel(item.labelKey, item.defaultLabel, sidebarLocale);

        return (
          <button
            key={item.id}
            onClick={() => onNavigate?.(item.id)}
            className={cn(
              'w-full flex items-center gap-3 px-4 py-2.5 rounded-lg transition-all',
              'text-left text-sm',
              isActive
                ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 font-medium'
                : 'text-gray-700 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 font-normal'
            )}
          >
            <IconComponent className="w-5 h-5 flex-shrink-0" />
            <span>{label}</span>
          </button>
        );
      })}
    </div>
  );
}
