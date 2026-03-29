import { useState, useCallback } from 'react';
import { useRouter } from 'next/navigation';
import { AuthFormData, AuthFormErrors, AuthMode } from '../types/auth';
import { validateAuthForm } from '../utils/validation';

interface UseAuthFormProps {
  mode?: AuthMode;
  onSubmit?: (data: AuthFormData) => void | Promise<void>;
  initialData?: Partial<AuthFormData>;
  redirectTo?: string;
}

interface UseAuthFormReturn {
  formData: AuthFormData;
  errors: Partial<AuthFormErrors>;
  isLoading: boolean;
  isValid: boolean;
  mode: AuthMode;
  setMode: (mode: AuthMode) => void;
  setField: (field: keyof AuthFormData, value: string) => void;
  setErrors: (errors: Partial<AuthFormErrors>) => void;
  clearError: (field: keyof AuthFormErrors) => void;
  clearAllErrors: () => void;
  handleSubmit: (e: React.FormEvent) => Promise<void>;
  reset: () => void;
  validateField: (field: keyof AuthFormData) => boolean;
  validateForm: () => boolean;
}

const initialFormData: AuthFormData = {
  email: '',
  password: '',
  confirmPassword: '',
  name: '',
  ssoEmail: '',
  ssoDomain: '',
  orgName: '',
  orgSlug: '',
  inviteEmail: '',
  inviteRole: 'member',
  appName: '',
  redirectURL: '',
};

const initialErrors: Partial<AuthFormErrors> = {};

export function useAuthForm({
  mode = 'signin',
  onSubmit,
  initialData = {},
  redirectTo = '/',
}: UseAuthFormProps = {}): UseAuthFormReturn {
  const router = useRouter();
  const [formData, setFormData] = useState<AuthFormData>({
    ...initialFormData,
    ...initialData,
  });
  const [errors, setErrorsState] = useState<Partial<AuthFormErrors>>(initialErrors);
  const [isLoading, setIsLoading] = useState(false);
  const [currentMode, setCurrentMode] = useState<AuthMode>(mode);

  const setField = useCallback((field: keyof AuthFormData, value: string) => {
    setFormData(prev => ({ ...prev, [field]: value }));
    // Clear error when user starts typing
    if (errors[field as keyof AuthFormErrors]) {
      setErrorsState(prev => ({ ...prev, [field]: undefined }));
    }
  }, [errors]);

  const setErrors = useCallback((newErrors: Partial<AuthFormErrors>) => {
    setErrorsState(prev => ({ ...prev, ...newErrors }));
  }, []);

  const clearError = useCallback((field: keyof AuthFormErrors) => {
    setErrorsState(prev => ({ ...prev, [field]: undefined }));
  }, []);

  const clearAllErrors = useCallback(() => {
    setErrorsState({});
  }, []);

  const validateField = useCallback((field: keyof AuthFormData): boolean => {
    const validationResult = validateAuthForm(formData, currentMode as 'signin' | 'signup' | 'forgot-password');
    const fieldError = validationResult.errors[field as keyof AuthFormErrors];
    
    if (fieldError) {
      setErrorsState(prev => ({ 
        ...prev, 
        [field]: fieldError
      }));
      return false;
    } else {
      clearError(field as keyof AuthFormErrors);
      return true;
    }
  }, [formData, currentMode, clearError]);

  const validateForm = useCallback((): boolean => {
    const validationResult = validateAuthForm(formData, currentMode as 'signin' | 'signup' | 'forgot-password');
    setErrorsState(validationResult.errors);
    return validationResult.isValid;
  }, [formData, currentMode]);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    
    if (!validateForm()) {
      return;
    }

    setIsLoading(true);
    try {
      // If onSubmit is provided, use it (for custom handling)
      if (onSubmit) {
        await onSubmit(formData);
      } else {
        // Default behavior: use ORPC client
        const { authORPCClient } = await import('../lib/orpc/client');
        
        if (currentMode === 'signin') {
          const result = await authORPCClient.signIn({
            email: formData.email,
            password: formData.password,
            rememberMe: false, // Could be added as a form field
          });
          
          if (result.success) {
            console.log('Sign-in successful:', result);
            
            // Check if user needs onboarding
            try {
              const { onboardingService } = await import('@/components/onboarding/services/onboarding-service');
              const needsOnboarding = await onboardingService.needsOnboarding();
              
              if (needsOnboarding) {
                console.log('🔄 User needs onboarding, redirecting...');
                await onboardingService.startOnboarding();
                router.push('/onboarding/profile');
              } else {
                console.log('✅ User onboarding complete, redirecting to:', redirectTo);
                router.push(redirectTo);
              }
            } catch (error) {
              console.error('Error checking onboarding status:', error);
              // Fallback to redirectTo if check fails
              router.push(redirectTo);
            }
          }
        } else if (currentMode === 'signup') {
          const result = await authORPCClient.signUp({
            name: formData.name,
            email: formData.email,
            password: formData.password,
            confirmPassword: formData.confirmPassword,
          });
          
          if (result.success) {
            console.log('Sign-up successful:', result);
            router.push('/onboarding/profile');
          }
        }
      }
    } catch (error) {
      console.error('Form submission error:', error);
      setErrors({
        email: error instanceof Error ? error.message : 'An error occurred during submission'
      });
    } finally {
      setIsLoading(false);
    }
  }, [formData, validateForm, onSubmit, setErrors, currentMode]);

  const reset = useCallback(() => {
    setFormData({ ...initialFormData, ...initialData });
    setErrorsState({});
    setIsLoading(false);
  }, [initialData]);

  const setMode = useCallback((newMode: AuthMode) => {
    setCurrentMode(newMode);
    clearAllErrors();
  }, [clearAllErrors]);

  const isValid = Object.keys(errors).length === 0 && 
    formData.email.length > 0 && 
    formData.password.length > 0 &&
    (currentMode === 'signin' || 
     (currentMode === 'signup' && formData.confirmPassword.length > 0));

  return {
    formData,
    errors,
    isLoading,
    isValid,
    mode: currentMode,
    setMode,
    setField,
    setErrors,
    clearError,
    clearAllErrors,
    handleSubmit,
    reset,
    validateField,
    validateForm,
  };
}