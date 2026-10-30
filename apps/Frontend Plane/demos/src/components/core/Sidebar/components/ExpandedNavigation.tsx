'use client';

import { FileText, Settings, MessageSquare, LayoutGrid, Zap, Boxes, Lock, Users } from 'lucide-react';
import { cn } from '../utils';

interface SidebarNavItemProps {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  active?: boolean;
  onClick?: () => void;
}

export function ExpandedNavigation({ 
  activeItem,
  onNavigate 
}: { 
  activeItem?: string;
  onNavigate?: (item: string) => void;
}) {
  const navigationItems = [
    {
      id: 'overview',
      icon: LayoutGrid,
      label: 'Overview',
    },
    {
      id: 'content',
      icon: FileText,
      label: 'Content',
    },
    {
      id: 'custom-answers',
      icon: MessageSquare,
      label: 'Custom answers',
    },
    {
      id: 'settings',
      icon: Settings,
      label: 'Settings',
    },
  ];

  const secondaryItems = [
    {
      id: 'playground',
      icon: LayoutGrid,
      label: 'Playground',
    },
    {
      id: 'activity',
      icon: Zap,
      label: 'Activity',
    },
    {
      id: 'analytics',
      icon: LayoutGrid,
      label: 'Analytics',
    },
    {
      id: 'sources',
      icon: Boxes,
      label: 'Sources',
    },
    {
      id: 'actions',
      icon: Zap,
      label: 'Actions',
    },
    {
      id: 'integrations',
      icon: Boxes,
      label: 'Integrations',
    },
    {
      id: 'deploy',
      icon: Lock,
      label: 'Deploy',
    },
    {
      id: 'contacts',
      icon: Users,
      label: 'Contacts',
    },
    {
      id: 'agent-settings',
      icon: Settings,
      label: 'Agent settings',
    },
  ];

  return (
    <div className="space-y-6 w-full">
      {/* Main Navigation */}
      <div className="space-y-1">
        {navigationItems.map((item) => {
          const IconComponent = item.icon;
          const isActive = activeItem === item.id;

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
              <span>{item.label}</span>
            </button>
          );
        })}
      </div>

      {/* Secondary Navigation */}
      <div className="space-y-1 pt-2 border-t border-gray-200 dark:border-gray-700">
        {secondaryItems.map((item) => {
          const IconComponent = item.icon;
          const isActive = activeItem === item.id;

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
              <span>{item.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
