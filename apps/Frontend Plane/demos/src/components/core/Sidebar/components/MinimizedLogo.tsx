'use client';

import { useSidebar } from '../../shared/SidebarContext';

export function MinimizedLogo() {
  const { toggleMinimize } = useSidebar();

  return (
    <button
      type="button"
      className="w-14 h-14 flex items-center justify-center hover:scale-105 transition-all p-1 rounded-xl focus:outline-none focus:ring-2 focus:ring-aquatiq-blue/40"
      onClick={toggleMinimize}
      title="Expand sidebar"
      aria-label="Expand sidebar"
    >
      <img
        src="/logo.png"
        alt="Aquatiq"
        className="w-full h-full object-contain"
      />
    </button>
  );
}
