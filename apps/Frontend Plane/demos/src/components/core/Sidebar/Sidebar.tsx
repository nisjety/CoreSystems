'use client';

import React from 'react';
import { cn } from './utils';
import { MinimizedLogo } from './components/MinimizedLogo';
import { MinimizedNavigation } from './components/MinimizedNavigation';
import { MinimizedFooter } from './components/MinimizedFooter';
import { useSidebar } from '../shared/SidebarContext';
import { ExpandedNavigation } from './components/ExpandedNavigation';
import { useAuth } from '@/components/auth/hooks/use-auth';
import { Settings, Menu, X } from 'lucide-react';

/**
 * Sidebar Component - Clean, modern design for Help Center
 * Supports both minimized (icon-only) and expanded (full) views
 */
export default function SimpleSidebar() {
  const { isMinimized, toggleMinimize } = useSidebar();
  const { user } = useAuth() || { user: null };
  const [isMobile, setIsMobile] = React.useState(false);
  const [showMobileMenu, setShowMobileMenu] = React.useState(false);
  const [activeNavItem, setActiveNavItem] = React.useState('overview');

  React.useEffect(() => {
    const handleResize = () => {
      setIsMobile(window.innerWidth < 768);
    };

    handleResize();
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  const handleNavigate = (itemId: string) => {
    setActiveNavItem(itemId);
    if (isMobile) {
      setShowMobileMenu(false);
    }
  };

  return (
    <aside
      className={cn(
        'fixed left-0 top-0 z-50 h-screen',
        'bg-white dark:bg-gray-900',
        'border-r border-gray-200 dark:border-gray-800',
        'flex flex-col transition-all duration-300',
        isMinimized ? 'w-20' : 'w-64',
        isMobile && isMinimized && 'w-20',
        isMobile && !isMinimized && 'w-full md:w-64'
      )}
    >
      {/* Header/Logo Section */}
      <div
        className={cn(
          'flex items-center justify-between h-20 px-4 border-b border-gray-200 dark:border-gray-800',
          isMinimized ? 'justify-center' : 'justify-between'
        )}
      >
        {!isMinimized && (
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center flex-shrink-0">
              <span className="text-white font-bold text-sm">S</span>
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <h2 className="font-bold text-sm text-gray-900 dark:text-gray-100">Sales</h2>
                <span className="px-2 py-0.5 bg-pink-100 dark:bg-pink-900/30 text-pink-700 dark:text-pink-400 text-xs font-semibold rounded">
                  Pro
                </span>
              </div>
            </div>
          </div>
        )}

        {!isMinimized && !isMobile && (
          <button
            onClick={toggleMinimize}
            className="p-1 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors flex-shrink-0"
            title="Collapse"
          >
            <svg
              className="w-5 h-5 text-gray-600 dark:text-gray-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
            </svg>
          </button>
        )}

        {isMinimized && !isMobile && (
          <button
            onClick={toggleMinimize}
            className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
            title="Expand"
          >
            <div className="w-6 h-6 bg-gradient-to-br from-blue-500 to-purple-600 rounded flex items-center justify-center">
              <span className="text-white font-bold text-xs">AI</span>
            </div>
          </button>
        )}

        {isMobile && !isMinimized && (
          <button
            onClick={() => setShowMobileMenu(!showMobileMenu)}
            className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors flex-shrink-0"
          >
            {showMobileMenu ? (
              <X className="w-5 h-5 text-gray-600 dark:text-gray-400" />
            ) : (
              <Menu className="w-5 h-5 text-gray-600 dark:text-gray-400" />
            )}
          </button>
        )}
      </div>

      {/* Navigation Section */}
      {!isMinimized && (!isMobile || showMobileMenu) && (
        <nav className="flex-1 overflow-y-auto px-3 py-6">
          <p className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider px-1 mb-4">
            Menu
          </p>
          <ExpandedNavigation activeItem={activeNavItem} onNavigate={handleNavigate} />
        </nav>
      )}

      {/* Minimized Navigation */}
      {isMinimized && !isMobile && (
        <div className="flex-1 flex flex-col items-center gap-8 overflow-x-hidden overflow-y-hidden py-8">
          <MinimizedNavigation />
        </div>
      )}

      {/* Stats Section - Only in expanded view */}
      {!isMinimized && (!isMobile || showMobileMenu) && (
        <div className="px-3 py-4 border-t border-gray-200 dark:border-gray-800">
          <div className="bg-gray-50 dark:bg-gray-800/50 rounded-lg p-3 space-y-2">
            <div className="flex items-center justify-between text-xs">
              <span className="text-gray-600 dark:text-gray-400">Messages</span>
              <span className="font-semibold text-gray-900 dark:text-gray-100">75 / 100</span>
            </div>
            <div className="flex items-center justify-between text-xs">
              <span className="text-gray-600 dark:text-gray-400">Sources</span>
              <span className="font-semibold text-gray-900 dark:text-gray-100">230 / 320 MB</span>
            </div>
            <div className="pt-2 text-xs text-gray-500 dark:text-gray-400 border-t border-gray-200 dark:border-gray-700">
              Resets on May 12, 2025
            </div>
          </div>
        </div>
      )}

      {/* Footer Section */}
      <div className={cn('border-t border-gray-200 dark:border-gray-800 p-4')}>
        {isMinimized ? (
          <MinimizedFooter
            onProfileClick={() => {
              // Handle profile navigation
            }}
            onHelpClick={() => {
              // Handle help
            }}
            isSettingsActive={false}
          />
        ) : (
          <div className="space-y-3">
            <button className="w-full flex items-center gap-3 px-4 py-2.5 rounded-lg text-left font-medium text-sm text-gray-700 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 transition-all">
              <Settings className="w-5 h-5 flex-shrink-0" />
              <span>Settings</span>
            </button>
            {user && (
              <div className="pt-3 border-t border-gray-200 dark:border-gray-800">
                <div className="flex items-center gap-3 px-2 py-2">
                  <div className="w-9 h-9 bg-blue-500 rounded-full flex items-center justify-center flex-shrink-0 text-white font-semibold text-sm">
                    {user.name?.charAt(0)?.toUpperCase() || '?'}
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-medium text-gray-900 dark:text-gray-100 truncate">
                      {user.name || 'User'}
                    </p>
                    <p className="text-xs text-gray-500 dark:text-gray-400 truncate">
                      {user.email}
                    </p>
                  </div>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}
