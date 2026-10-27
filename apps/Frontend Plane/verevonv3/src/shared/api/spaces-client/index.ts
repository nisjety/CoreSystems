// Barrel for the Space API client (P-SPA1). Callers keep importing
// '@/shared/api/spaces-client'. The pre-split client lives in ./core unchanged;
// a Space slice adds its calls in spaces-client/<feature>.ts and one
// `export * from './<feature>'` line below (keep the list sorted after core).
export * from './core'
export * from './membership-writer'
