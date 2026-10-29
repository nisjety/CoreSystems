'use client';

import { usePathname } from 'next/navigation';
import Link from 'next/link';
import clsx from 'clsx';
import { sharedNavItems } from '../config/nav-items';
import { SimpleTooltip } from '../ui/simple-tooltip';
import { LayoutGrid, FileText, MessageSquare, Settings } from 'lucide-react';

export function MinimizedNavigation() {
  const pathname = usePathname();

  const mainItems = [
    { id: 'overview', icon: LayoutGrid, label: 'Overview', href: '/dashboard' },
    { id: 'content', icon: FileText, label: 'Content', href: '/content' },
    { id: 'custom-answers', icon: MessageSquare, label: 'Custom answers', href: '/answers' },
    { id: 'settings', icon: Settings, label: 'Settings', href: '/settings' },
  ];

  return (
    <nav className="flex flex-col items-center gap-4">
      {mainItems.map((item) => {
        const Icon = item.icon;
        const isActive = pathname === item.href || pathname.startsWith(item.href + '/');
        
        return (
          <SimpleTooltip key={item.id} content={item.label} placement="right" delay={200}>
            <Link
              href={item.href}
              className={clsx(
                'w-12 h-12 flex items-center justify-center rounded-lg transition-all',
                isActive
                  ? 'text-white hover:shadow-lg'
                  : 'text-gray-400 hover:bg-gray-100 hover:text-blue-600'
              )}
              style={isActive ? { backgroundColor: 'var(--aquatiq-blue)' } : undefined}
            >
              <Icon className="w-6 h-6" />
            </Link>
          </SimpleTooltip>
        );
      })}
    </nav>
  );
}
