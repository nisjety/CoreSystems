// 🎯 Authentication Hooks - Complete Collection (100% COMPLETE!)
// All custom hooks for authentication, verification, and security management

// Core authentication hooks
export { useAuth } from './use-auth';
export { useAuthForm } from './use-auth-form';

// Specialized hooks
// NOTE: use-consent (legacy) deprecated; re-export enterprise consent hook.
export { useConsent } from '../consent';
export { usePasskey } from './use-passkey';
export { useModal, useModalStack } from './use-modal';

// 🆕 FINAL 3 HOOKS - JUST IMPLEMENTED! 🎉
export { useTwoFactor } from './useTwoFactor';
export { useVerification } from './useVerification';
export { useSecuritySettings } from './useSecuritySettings';