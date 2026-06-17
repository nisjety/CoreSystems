import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import type { JSX } from 'solid-js'

// One shared query client keeps server-state caching consistent across every
// route surface. `gcTime` is deliberately long so a query's data survives the
// component unmounting on a tab/route switch (the default 5 min would drop it and
// force a refetch on return); individual queries set their own `staleTime`
// (e.g. 4h for web search / scrapes) to control when a refetch is worthwhile.
export const FOUR_HOURS_MS = 4 * 60 * 60 * 1000

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      staleTime: 30_000,
      gcTime: FOUR_HOURS_MS,
    },
  },
})

export function QueryProvider(props: { children: JSX.Element }) {
  return <QueryClientProvider client={queryClient}>{props.children}</QueryClientProvider>
}
