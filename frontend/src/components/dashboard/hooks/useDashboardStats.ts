import { useQuery } from '@tanstack/react-query';

export interface DashboardStats {
  memberCount: number | null;
  sourceCount: number | null;
  documentCount: number | null;
  crawledPages: number | null;
  crawlStatus: 'idle' | 'running' | 'done' | 'error' | null;
  lastCrawlAt: string | null;
}

export function useDashboardStats() {
  return useQuery<DashboardStats | null>({
    queryKey: ['dashboard-stats'],
    queryFn: () =>
      fetch('/api/dashboard/stats').then((r) => (r.ok ? r.json() : null)),
  });
}

export function getCardStat(id: string, stats: DashboardStats | null): string | null {
  if (!stats) return null;

  switch (id) {
    case 'team':
      return stats.memberCount != null ? `${stats.memberCount} med.` : null;
    case 'sources':
      return stats.sourceCount != null ? `${stats.sourceCount} kilder` : null;
    case 'documents':
      return stats.documentCount != null ? `${stats.documentCount} dok.` : null;
    case 'knowledge':
      return stats.crawledPages != null ? `${stats.crawledPages} sider` : null;
    default:
      return null;
  }
}
