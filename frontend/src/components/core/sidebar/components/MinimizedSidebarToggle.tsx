'use client';

import { PanelLeftClose, PanelLeftOpen } from 'lucide-react';
import { useSidebar } from '../../shared/SidebarContext';
import { SimpleTooltip } from '../ui/simple-tooltip';

export function MinimizedSidebarToggle() {
  const { isMinimized, isMobile, toggleMinimize } = useSidebar();

  if (isMobile) {
    return null;
  }

  const Icon = isMinimized ? PanelLeftOpen : PanelLeftClose;
  const label = isMinimized ? 'Expand sidebar' : 'Collapse sidebar';

  return (
    <SimpleTooltip content={label} placement="right" delay={200}>
      <button
        type="button"
        className="flex h-7 w-7 items-center justify-center rounded-lg transition-colors hover:bg-black/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-[#DD7A1F]/40 focus-visible:ring-offset-1"
        onClick={toggleMinimize}
        title={label}
        aria-label={label}
      >
        <Icon className="h-4 w-4 text-[#666A73]" strokeWidth={1.9} />
      </button>
    </SimpleTooltip>
  );
}