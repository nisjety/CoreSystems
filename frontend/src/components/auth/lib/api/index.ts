// Export API client and hooks for easy importing
export { apiClient } from './auth-client';
export type { ApiClient } from './auth-client';

export {
  useUserProfile,
  useUpdateProfile,
  useEnable2FA,
  useDisable2FA,
  queryKeys,
} from './hooks';

// Export contracts for type safety
export { apiContract } from './contracts';
export type { ApiContract } from './contracts';
