/**
 * Enhanced Authentication Modals - Enterprise-grade implementations
 * 
 * This module provides comprehensive modal components for authentication
 * with ORPC + Better Auth integration, Norwegian/English i18n support,
 * WCAG 2.1 AA accessibility compliance, and analytics tracking.
 * 
 * Features:
 * - ✅ ORPC + Better Auth integration with type-safe interfaces
 * - ✅ Bilingual i18n support (Norwegian/English) with SSR safety
 * - ✅ Comprehensive analytics with consent-aware tracking
 * - ✅ WCAG 2.1 AA accessibility compliance
 * - ✅ Enhanced UX following design laws (Fitts's Law, Von Restorff Effect, etc.)
 * - ✅ Type-safe architecture with proper error handling
 * - ✅ Real-time validation and enhanced user feedback
 * - ✅ Responsive design with multiple size variants
 * - ✅ Loading states and keyboard navigation support
 * 
 * Design Laws Implementation:
 * - Chunking: Information organized in digestible sections
 * - Aesthetic-Usability Effect: Clean, professional visual hierarchy
 * - Choice Overload: Simplified interaction patterns
 * - Cognitive Load: Reduced mental effort required
 * - Doherty Threshold: Responsive feedback under 400ms
 * - Fitts's Law: Adequately sized interactive elements
 * - Von Restorff Effect: Visual distinction for important elements
 * 
 * @example Basic Modal Usage
 * ```tsx
 * import { AuthModal } from '@/components/auth/modals';
 * 
 * function MyComponent() {
 *   const [isOpen, setIsOpen] = useState(false);
 *   
 *   const handleSuccess = (result) => {
 *     console.log('Authentication successful:', result);
 *   };
 *   
 *   return (
 *     <AuthModal
 *       isOpen={isOpen}
 *       onClose={() => setIsOpen(false)}
 *       mode="signin"
 *       onSuccess={handleSuccess}
 *       enableAnalytics={true}
 *       trackingId="main-auth-modal"
 *     />
 *   );
 * }
 * ```
 * 
 * @example Quick Auth Modal
 * ```tsx
 * import { QuickAuthModal } from '@/components/auth/modals';
 * 
 * <QuickAuthModal
 *   mode="signup"
 *   isOpen={showSignup}
 *   onClose={() => setShowSignup(false)}
 *   trackingId="quick-signup"
 * />
 * ```
 * 
 * @example Custom Modal with Content
 * ```tsx
 * import { CustomAuthModal } from '@/components/auth/modals';
 * 
 * <CustomAuthModal
 *   isOpen={showCustom}
 *   onClose={() => setShowCustom(false)}
 *   title="Custom Authentication"
 *   description="Specialized authentication flow"
 *   size="lg"
 * >
 *   <YourCustomContent />
 * </CustomAuthModal>
 * ```
 * 
 * @example Modal Management Hook
 * ```tsx
 * import { useAuthModal } from '@/components/auth/modals';
 * 
 * function App() {
 *   const { openAuthModal, closeAuthModal, isOpen, authData } = useAuthModal();
 *   
 *   const handleLogin = () => {
 *     openAuthModal('signin', {
 *       redirectTo: '/dashboard',
 *       trackingId: 'header-login',
 *       onSuccess: (result) => console.log('Login success:', result),
 *     });
 *   };
 *   
 *   return (
 *     <button onClick={handleLogin}>
 *       Sign In
 *     </button>
 *   );
 * }
 * ```
 * 
 * @example Loading Modal
 * ```tsx
 * import { AuthLoadingModal } from '@/components/auth/modals';
 * 
 * <AuthLoadingModal
 *   isOpen={isProcessing}
 *   title="Processing Authentication"
 *   message="Please wait while we verify your credentials..."
 *   showCancelButton={true}
 *   onCancel={() => setIsProcessing(false)}
 *   trackingId="auth-processing"
 * />
 * ```
 */

// Core modal components with enhanced features
export {
  AuthModal,
  QuickAuthModal,
  CustomAuthModal,
  AuthLoadingModal,
  useAuthModal,
} from './AuthModal';

// Confirmation modal for sensitive operations
export {
  ConfirmationModal,
} from './ConfirmationModal';

/**
 * Modal Size Configuration Guide
 * 
 * - 'sm': max-w-md (448px) - Simple forms, quick actions
 * - 'md': max-w-lg (512px) - Standard auth forms
 * - 'lg': max-w-4xl (896px) - Complex multi-step processes (default)
 * - 'xl': max-w-6xl (1152px) - Enterprise dashboards, detailed flows
 */
export const MODAL_SIZES = {
  sm: 'max-w-md',
  md: 'max-w-lg', 
  lg: 'max-w-4xl',
  xl: 'max-w-6xl',
} as const;

/**
 * Analytics Event Names for Tracking
 * 
 * Use these constants for consistent event naming across your application
 */
export const MODAL_ANALYTICS_EVENTS = {
  // Modal lifecycle
  MODAL_OPENED: 'modal_opened',
  MODAL_CLOSED: 'modal_closed',
  MODAL_ESCAPED: 'modal_escaped',
  MODAL_OVERLAY_CLICKED: 'modal_overlay_clicked',
  
  // Authentication events
  AUTH_SUCCESS: 'auth_success',
  AUTH_ERROR: 'auth_error',
  AUTH_STARTED: 'auth_started',
  AUTH_CANCELLED: 'auth_cancelled',
  
  // User interactions
  USER_INTERACTION: 'user_interaction',
  FORM_VALIDATION_ERROR: 'form_validation_error',
  HELP_ACCESSED: 'help_accessed',
  
  // Loading states
  LOADING_MODAL_SHOWN: 'loading_modal_shown',
  LOADING_MODAL_CANCELLED: 'loading_modal_cancelled',
} as const;

/**
 * Accessibility Configuration
 * 
 * WCAG 2.1 AA compliance features included:
 * - Keyboard navigation (Tab, Shift+Tab, Escape)
 * - Screen reader support (ARIA labels, roles, descriptions)
 * - Focus management and trap
 * - High contrast support
 * - Reduced motion preferences
 * - Semantic HTML structure
 */
export const ACCESSIBILITY_FEATURES = {
  KEYBOARD_SUPPORT: 'Full keyboard navigation',
  SCREEN_READER: 'ARIA labels and semantic structure',
  FOCUS_MANAGEMENT: 'Automatic focus trap and restoration',
  HIGH_CONTRAST: 'Supports system contrast preferences',
  REDUCED_MOTION: 'Respects prefers-reduced-motion',
  COLOR_INDEPENDENCE: 'Information not conveyed by color alone',
} as const;

/**
 * Design Laws Implementation Summary
 * 
 * The modal components implement various UX design laws for optimal user experience:
 */
export const DESIGN_LAWS_IMPLEMENTATION = {
  CHUNKING: 'Information grouped in logical sections',
  AESTHETIC_USABILITY: 'Clean design enhances perceived usability',
  CHOICE_OVERLOAD: 'Limited options to reduce decision paralysis',
  COGNITIVE_LOAD: 'Minimized mental effort through clear interfaces',
  DOHERTY_THRESHOLD: 'Feedback provided within 400ms',
  FITTS_LAW: 'Touch targets meet minimum size requirements',
  VON_RESTORFF_EFFECT: 'Important elements visually distinguished',
  FEEDBACK_PRINCIPLE: 'Clear feedback for all user actions',
  CONSISTENCY: 'Uniform patterns across all modal types',
  ERROR_PREVENTION: 'Validation and confirmation for destructive actions',
} as const;

/**
 * Enhanced Modal Components Export Summary
 * =====================================
 * 
 * Comprehensive authentication modal system with enterprise features:
 * 
 * 🔄 Core Components:
 * - AuthModal: Primary auth modal with full feature set
 * - QuickAuthModal: Simplified modal for quick authentication
 * - CustomAuthModal: Customizable modal with advanced configuration
 * - AuthLoadingModal: Loading state modal for auth processes
 * - useAuthModal: Hook for programmatic modal management
 * 
 * 🎯 Confirmation Modals:
 * - ConfirmationModal: Base confirmation modal with variants
 * 
 * 📊 Configuration Constants:
 * - MODAL_SIZES: Size variants for different use cases
 * - MODAL_ANALYTICS_EVENTS: Standardized event naming
 * - ACCESSIBILITY_FEATURES: WCAG 2.1 AA compliance features
 * - DESIGN_LAWS_IMPLEMENTATION: UX principles implementation guide
 * 
 * 🎨 Design Laws Applied:
 * - Chunking, Aesthetic-Usability Effect, Choice Overload
 * - Cognitive Load, Doherty Threshold, Fitts's Law
 * - Von Restorff Effect, Feedback Principle, Consistency
 * 
 * 🔒 Type Safety:
 * - Full TypeScript support with proper interfaces
 * - ORPC + Better Auth integration types
 * - Enhanced error handling with structured error types
 * 
 * 🌍 Internationalization:
 * - Norwegian/English bilingual support
 * - SSR-safe language switching
 * - Fallback translation system
 * 
 * ♿ Accessibility:
 * - WCAG 2.1 AA compliant
 * - Full keyboard navigation
 * - Screen reader optimized
 * - Focus management
 * 
 * 📈 Analytics:
 * - Consent-aware tracking
 * - GDPR compliant
 * - Comprehensive event logging
 * - Performance monitoring
 */
