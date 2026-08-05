'use client';

import React, { useEffect, useMemo } from 'react';
import {
  dehydrate,
  hydrate,
  isServer,
  QueryClient,
  QueryClientProvider,
  type DehydratedState,
} from '@tanstack/react-query';

interface QueryProviderProps {
  children: React.ReactNode;
}

interface QueryCachePersistenceProps extends QueryProviderProps {
  queryClient: QueryClient;
}

type PersistedQueryCache = {
  timestamp: number;
  state: DehydratedState;
};

type PersistedQuery = DehydratedState['queries'][number];

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // SSR: 60s stale avoids refetching immediately on hydration
        staleTime: 60_000,
        // Keep unused query data for 5 minutes before GC
        gcTime: 5 * 60_000,
        // Don't refetch on window focus by default — explicit per-hook
        refetchOnWindowFocus: false,
        // Don't refetch on reconnect — WS handles real-time updates
        refetchOnReconnect: 'always',
        retry: (failureCount, error) => {
          // Don't retry on 4xx errors (except 408, 429)
          if (error && 'status' in error) {
            const status = error.status as number;
            if (status >= 400 && status < 500 && status !== 408 && status !== 429) {
              return false;
            }
          }
          return failureCount < 2;
        },
        retryDelay: (attemptIndex) => Math.min(1000 * 2 ** attemptIndex, 30000),
        throwOnError: false,
      },
      mutations: {
        retry: 1,
        throwOnError: false,
      },
    },
  });
}

let browserQueryClient: QueryClient | undefined = undefined;

const LEGACY_PERSISTED_QUERY_CACHE_KEYS = [
  'verevon.tanstack-query.cache.v1',
];
const PERSISTED_QUERY_CACHE_KEY = 'verevon.tanstack-query.cache.v2';
const PERSISTED_QUERY_CACHE_MAX_AGE = 24 * 60 * 60 * 1000;
const PERSISTED_QUERY_ROOT_KEYS = new Set([
  'knowledge',
  'inbox',
  'support',
  'settings',
]);

function getQueryClient() {
  if (isServer) {
    // Server: always make a new query client
    return makeQueryClient();
  } else {
    // Browser: make a new query client if we don't already have one
    // This is very important, so we don't re-make a new client if React
    // suspends during the initial render. This may not be needed if we
    // have a suspense boundary BELOW the creation of the query client
    if (!browserQueryClient) browserQueryClient = makeQueryClient();
    return browserQueryClient;
  }
}

function shouldPersistQuery(queryKey: readonly unknown[]) {
  const firstKey = queryKey[0];
  return typeof firstKey === 'string' && PERSISTED_QUERY_ROOT_KEYS.has(firstKey);
}

function sanitizePersistedState(state: DehydratedState): DehydratedState {
  return {
    ...state,
    mutations: [],
    queries: state.queries
      .filter((query) => {
        return shouldPersistQuery(query.queryKey) && query.state.status === 'success';
      })
      .map((query) => {
        const { promise: _promise, ...queryWithoutPromise } = query as PersistedQuery & {
          promise?: unknown;
        };
        return queryWithoutPromise as PersistedQuery;
      }),
  };
}

function removePersistedQueryCache() {
  if (isServer) {
    return;
  }

  try {
    window.localStorage.removeItem(PERSISTED_QUERY_CACHE_KEY);
    LEGACY_PERSISTED_QUERY_CACHE_KEYS.forEach((key) => {
      window.localStorage.removeItem(key);
    });
  } catch {
    // Ignore storage errors; React Query still works without persisted cache.
  }
}

function readPersistedQueryCache(): DehydratedState | undefined {
  if (isServer) {
    return undefined;
  }

  try {
    const serializedCache = window.localStorage.getItem(PERSISTED_QUERY_CACHE_KEY);
    if (!serializedCache) {
      return undefined;
    }

    const persistedCache = JSON.parse(serializedCache) as Partial<PersistedQueryCache>;
    if (
      typeof persistedCache.timestamp !== 'number' ||
      !persistedCache.state ||
      Date.now() - persistedCache.timestamp > PERSISTED_QUERY_CACHE_MAX_AGE
    ) {
      removePersistedQueryCache();
      return undefined;
    }

    const sanitizedState = sanitizePersistedState(persistedCache.state);
    return sanitizedState.queries.length > 0 ? sanitizedState : undefined;
  } catch {
    removePersistedQueryCache();
    return undefined;
  }
}

function writePersistedQueryCache(queryClient: QueryClient) {
  if (isServer) {
    return;
  }

  try {
    const dehydratedState = dehydrate(queryClient, {
      shouldDehydrateQuery: (query) => {
        return shouldPersistQuery(query.queryKey) && query.state.status === 'success';
      },
    });

    const sanitizedState = sanitizePersistedState(dehydratedState);

    if (sanitizedState.queries.length === 0) {
      removePersistedQueryCache();
      return;
    }

    const persistedCache: PersistedQueryCache = {
      timestamp: Date.now(),
      state: sanitizedState,
    };

    window.localStorage.setItem(
      PERSISTED_QUERY_CACHE_KEY,
      JSON.stringify(persistedCache),
    );
  } catch {
    removePersistedQueryCache();
  }
}

function QueryCachePersistence({ children, queryClient }: QueryCachePersistenceProps) {
  useEffect(() => {
    const persistedState = readPersistedQueryCache();
    if (persistedState) {
      try {
        hydrate(queryClient, persistedState);
      } catch {
        removePersistedQueryCache();
      }
    }

    void queryClient.resumePausedMutations();

    return queryClient.getQueryCache().subscribe(() => {
      writePersistedQueryCache(queryClient);
    });
  }, [queryClient]);

  return (
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  );
}

export function QueryProvider({ children }: QueryProviderProps) {
  // Use useMemo to ensure stable reference and prevent webpack module factory issues
  // eslint-disable-next-line react-hooks/preserve-manual-memoization -- QueryClient must be stable
  const queryClient = useMemo(() => {
    try {
      return getQueryClient();
    } catch (error) {
      // Handle webpack module factory errors during QueryClient creation
      if (
        error instanceof Error &&
        error.message.includes("Cannot read properties of undefined (reading 'call')")
      ) {
        console.warn('Webpack module factory error during QueryClient creation, retrying...');
        // Return a minimal QueryClient as fallback
        return new QueryClient({
          defaultOptions: {
            queries: { retry: false, throwOnError: false },
            mutations: { retry: false, throwOnError: false },
          },
        });
      }
      throw error;
    }
  }, []);

  return (
    <QueryCachePersistence queryClient={queryClient}>
      {children}
    </QueryCachePersistence>
  );
}
