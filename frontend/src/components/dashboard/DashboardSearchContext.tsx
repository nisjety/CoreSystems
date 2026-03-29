'use client';

import React, { createContext, useContext, useEffect, useMemo, useState } from 'react';

type SearchTriggerSource = 'navbar' | 'sidebar';
const RECENT_SEARCHES_STORAGE_KEY = 'dashboard-global-search-recent';
const MAX_RECENT_SEARCHES = 5;

interface DashboardSearchContextValue {
  isGlobalSearchOpen: boolean;
  globalSearchQuery: string;
  recentQueries: string[];
  lastSearchTrigger: SearchTriggerSource;
  openGlobalSearch: (source?: SearchTriggerSource, prefill?: string) => void;
  closeGlobalSearch: () => void;
  registerRecentQuery: (query: string) => void;
  setGlobalSearchQuery: (query: string) => void;
}

const DashboardSearchContext = createContext<DashboardSearchContextValue | undefined>(undefined);

export function DashboardSearchProvider({ children }: { children: React.ReactNode }) {
  const [isGlobalSearchOpen, setIsGlobalSearchOpen] = useState(false);
  const [globalSearchQuery, setGlobalSearchQuery] = useState('');
  const [recentQueries, setRecentQueries] = useState<string[]>([]);
  const [lastSearchTrigger, setLastSearchTrigger] = useState<SearchTriggerSource>('navbar');

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    const storedQueries = window.localStorage.getItem(RECENT_SEARCHES_STORAGE_KEY);
    if (!storedQueries) {
      return;
    }

    try {
      const parsedQueries = JSON.parse(storedQueries);
      if (Array.isArray(parsedQueries)) {
        setRecentQueries(parsedQueries.filter((query): query is string => typeof query === 'string'));
      }
    } catch {
      window.localStorage.removeItem(RECENT_SEARCHES_STORAGE_KEY);
    }
  }, []);

  useEffect(() => {
    if (typeof window === 'undefined') {
      return;
    }

    try {
      window.localStorage.setItem(RECENT_SEARCHES_STORAGE_KEY, JSON.stringify(recentQueries));
    } catch {
      // Ignore storage failures so search remains usable in restricted contexts.
    }
  }, [recentQueries]);

  useEffect(() => {
    const isEditableTarget = (eventTarget: EventTarget | null) => {
      if (!(eventTarget instanceof HTMLElement)) {
        return false;
      }

      if (eventTarget.isContentEditable) {
        return true;
      }

      const tagName = eventTarget.tagName.toLowerCase();
      return tagName === 'input' || tagName === 'textarea' || tagName === 'select';
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (isEditableTarget(event.target)) {
        return;
      }

      const isSearchShortcut = (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k';
      const isSlashShortcut = !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && event.key === '/';

      if (!isSearchShortcut && !isSlashShortcut) {
        return;
      }

      event.preventDefault();
      setLastSearchTrigger('navbar');
      setIsGlobalSearchOpen(true);
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const value = useMemo<DashboardSearchContextValue>(() => ({
    isGlobalSearchOpen,
    globalSearchQuery,
    recentQueries,
    lastSearchTrigger,
    openGlobalSearch: (source = 'navbar', prefill) => {
      setLastSearchTrigger(source);
      if (typeof prefill === 'string') {
        setGlobalSearchQuery(prefill);
      }
      setIsGlobalSearchOpen(true);
    },
    closeGlobalSearch: () => setIsGlobalSearchOpen(false),
    registerRecentQuery: (query) => {
      const normalizedQuery = query.trim();
      if (!normalizedQuery) {
        return;
      }

      setRecentQueries((previousQueries) => {
        const nextQueries = [normalizedQuery, ...previousQueries.filter((item) => item !== normalizedQuery)];
        return nextQueries.slice(0, MAX_RECENT_SEARCHES);
      });
    },
    setGlobalSearchQuery,
  }), [globalSearchQuery, isGlobalSearchOpen, lastSearchTrigger, recentQueries]);

  return (
    <DashboardSearchContext.Provider value={value}>
      {children}
    </DashboardSearchContext.Provider>
  );
}

export function useDashboardSearch() {
  const context = useContext(DashboardSearchContext);
  if (!context) {
    throw new Error('useDashboardSearch must be used within a DashboardSearchProvider');
  }

  return context;
}