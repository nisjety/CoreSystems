// Enhanced core authentication components with enterprise-grade features:
// - Complete internationalization system with type-safe translation keys
// - Advanced form validation with real-time feedback and Zod schemas
// - Type-safe ORPC client integration with TanStack Query
// - Comprehensive toast notification system with Sonner
// - Enhanced loading states with visual indicators and accessibility
// - Provider-independent operation with graceful fallbacks
// - Complete Norwegian/English i18n with SSR safety
// - Advanced error handling and validation
// - Performance optimizations with React Query caching

// Note: Do not re-export AuthPage here to avoid circular deps with AuthPage importing from core/*

// Enhanced authentication tabs with:
// - Complete internationalization using the new i18n system
// - Advanced loading state management with visual feedback
// - Toast notification system with Sonner integration
// - Type-safe ORPC integration for mode switching
// - Complete accessibility improvements with ARIA labels
// - Icon support with loading animations and state indicators
// - SSR-safe Norwegian/English localization
// - Feature flags for conditional tab display
// - Performance optimizations and error boundaries
export { AuthTabs } from './AuthTabs'; 

// Enhanced authentication forms with:
// - Complete internationalization using the new i18n system
// - Advanced form validation system with Zod schemas and react-hook-form
// - Type-safe ORPC client integration with TanStack Query
// - Real-time validation with debounced feedback
// - Password strength indicator with detailed security feedback
// - Field validation indicators with accessibility features
// - Toast notification feedback with Sonner
// - Enhanced loading states with skeleton components
// - SSR-safe Norwegian/English translations
// - Password visibility toggles with security considerations
// - SSO and Organization management with enterprise features
// - Complete error handling and recovery mechanisms
export { AuthForms } from './AuthForms';

// Re-export auth types for convenience
export type { AuthMode, AuthFormData, AuthFormErrors } from '../types/auth';
