// Core components
export * from './core';

// Provider components
export * from './providers';

// Verification components
export * from './verification';

// Consent components
export * from './consent';

// Unified Auth System - New comprehensive implementation
export { 
  AuthPage, 
  ProtectedAuthPage, 
  AdminAuthPage,
  type AuthPageProps
} from './AuthPage';

// Re-export AuthMode from central types file for consumers
export type { AuthMode } from './types/auth';
