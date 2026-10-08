import type { HomeTab } from '@/shared/actions/home-tab'

// The tab set is owned by the URL contract, so the two cannot drift apart.
export type DashboardTab = HomeTab
export type SearchResultTab = 'Info' | 'Videos' | 'Map' | 'Images' | 'Shopping'
export type ImagesStatus = 'idle' | 'loading' | 'loaded' | 'error'

export const dashboardTabs: DashboardTab[] = ['Chat', 'Søk', 'Crawl']
export const searchResultTabs: SearchResultTab[] = ['Info', 'Videos', 'Map', 'Images', 'Shopping']
