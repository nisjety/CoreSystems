'use client';

import React, { useMemo } from 'react';
import {
  isServer,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query';

interface QueryProviderProps {
  children: React.ReactNode;
}

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // With SSR, we usually want to set some default staleTime
        // above 0 to avoid refetching immediately on the client
        staleTime: 60 * 1000,
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
        // Prevent webpack module factory issues by ensuring stable error boundaries
        throwOnError: false,
      },
      mutations: {
        retry: 1,
        // Prevent webpack module factory issues in error scenarios
        throwOnError: false,
      },
    },
  });
}

let browserQueryClient: QueryClient | undefined = undefined;

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
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  );
}
