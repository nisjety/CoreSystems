'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { useAuth } from './use-auth';
import { toast } from 'sonner';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';

// Types
interface TwoFactorMethod {
  type: 'email' | 'sms' | 'totp';
  enabled: boolean;
  verified: boolean;
  primary: boolean;
  lastUsed?: Date;
}

interface TwoFactorSettings {
  isEnabled: boolean;
  primaryMethod?: 'email' | 'sms' | 'totp';
  methods: TwoFactorMethod[];
  backupCodes: string[];
  hasBackupCodes: boolean;
}

interface TotpSetup {
  secret: string;
  qrCode: string;
  backupCodes: string[];
  verificationCode?: string;
}

interface UseTwoFactorReturn {
  // State
  settings: TwoFactorSettings | null;
  isLoading: boolean;
  isSettingUp: boolean;
  error: string | null;
  
  // Setup operations
  initializeSetup: (method: 'email' | 'sms' | 'totp') => Promise<TotpSetup | null>;
  verifySetup: (code: string, method: 'email' | 'sms' | 'totp') => Promise<boolean>;
  completeSetup: () => Promise<boolean>;
  
  // Management operations
  enable: (method: 'email' | 'sms' | 'totp') => Promise<boolean>;
  disable: (method: 'email' | 'sms' | 'totp') => Promise<boolean>;
  setPrimaryMethod: (method: 'email' | 'sms' | 'totp') => Promise<boolean>;
  
  // Backup codes
  generateBackupCodes: () => Promise<string[]>;
  verifyBackupCode: (code: string) => Promise<boolean>;
  viewBackupCodes: () => Promise<string[]>;
  
  // Verification
  sendVerificationCode: (method: 'email' | 'sms') => Promise<boolean>;
  verifyCode: (code: string, method: 'email' | 'sms' | 'totp') => Promise<boolean>;
  
  // Recovery
  disableAllMethods: (recoveryCode: string) => Promise<boolean>;
  resetTwoFactor: () => Promise<boolean>;
  
  // Utilities
  refresh: () => Promise<void>;
  isMethodAvailable: (method: 'email' | 'sms' | 'totp') => boolean;
  getMethodStatus: (method: 'email' | 'sms' | 'totp') => TwoFactorMethod | null;
}

export function useTwoFactor(): UseTwoFactorReturn {
  const { user } = useAuth();
  const { t: tAuth } = useTranslationWithInterpolation();
  const [settings, setSettings] = useState<TwoFactorSettings | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isSettingUp, setIsSettingUp] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [setupData, setSetupData] = useState<TotpSetup | null>(null);

  // TODO: Replace with real auth client import
  /* Mock auth client (replace with real implementation) */
  const authClient = useMemo(() => ({
    twoFactor: {
      getSettings: async () => ({ data: { isEnabled: false, methods: [], backupCodes: [], hasBackupCodes: false } as TwoFactorSettings }),
       
      initializeSetup: async (_opts: { method: string }) => ({ data: { secret: '', qrCode: '', backupCodes: [] } }),
       
      verifySetup: async (_opts: { code: string; method: string }) => ({ data: { success: true } }),
       
      completeSetup: async (_opts: { verificationCode: string }) => ({ data: { success: true } }),
       
      enable: async (_opts: { method: string }) => ({ data: { success: true } }),
       
      disable: async (_opts: { method: string }) => ({ data: { success: true } }),
       
      setPrimary: async (_opts: { method: string }) => ({ data: { success: true } }),
      generateBackupCodes: async () => ({ data: { backupCodes: [] } }),
       
      verifyBackupCode: async (_opts: { code: string }) => ({ data: { success: true } }),
      getBackupCodes: async () => ({ data: { backupCodes: [] } }),
       
      sendCode: async (_opts: { method: string }) => ({ data: { success: true } }),
       
      verify: async (_opts: { code: string; method: string }) => ({ data: { success: true } }),
       
      disableAll: async (_opts: { recoveryCode: string }) => ({ data: { success: true } }),
      reset: async () => ({ data: { success: true } }),
    }
  }), []);
  // Load 2FA settings
  const loadSettings = useCallback(async () => {
    if (!user) return;
    
    setIsLoading(true);
    setError(null);
    
    try {
      const response = await authClient.twoFactor.getSettings();
      setSettings(response.data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load 2FA settings');
      console.error('Failed to load 2FA settings:', err);
    } finally {
      setIsLoading(false);
    }
  }, [user, authClient]);
  // Initialize setup for a method
  const initializeSetup = useCallback(async (method: 'email' | 'sms' | 'totp'): Promise<TotpSetup | null> => {
    setIsSettingUp(true);
    setError(null);
    
    try {
      const response = await authClient.twoFactor.initializeSetup({ method });
      const setup: TotpSetup = {
        secret: response.data.secret || '',
        qrCode: response.data.qrCode || '',
        backupCodes: response.data.backupCodes || [],
        verificationCode: undefined
      };
      
      setSetupData(setup);
      return setup;
    } catch (err) {
      console.error('Failed to initialize 2FA setup:', err);
      return null;
    } finally {
      setIsSettingUp(false);
    }
  }, [authClient]);

  // Verify setup code
  const verifySetup = useCallback(async (code: string, method: 'email' | 'sms' | 'totp'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.verifySetup({ code, method });
      
      if (setupData) {
        setSetupData({ ...setupData, verificationCode: code });
      }
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid verification code');
      console.error('Failed to verify setup code:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, setupData]);

  // Complete setup process
  const completeSetup = useCallback(async (): Promise<boolean> => {
    if (!setupData?.verificationCode) {
      setError('Verification code required to complete setup');
      return false;
    }
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.completeSetup({
        verificationCode: setupData.verificationCode
      });
      
      await loadSettings();
      setSetupData(null);
      toast.success(tAuth('auth.twoFactor.notifications.setupComplete'));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to complete 2FA setup');
      console.error('Failed to complete 2FA setup:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, setupData, loadSettings, tAuth]);

  // Enable a 2FA method
  const enable = useCallback(async (method: 'email' | 'sms' | 'totp'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.enable({ method });
      await loadSettings();
      toast.success(tAuth('auth.twoFactor.notifications.methodEnabled', { method: method.toUpperCase() }));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to enable ${method} 2FA`);
      console.error(`Failed to enable ${method} 2FA:`, err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Disable a 2FA method
  const disable = useCallback(async (method: 'email' | 'sms' | 'totp'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.disable({ method });
      await loadSettings();
  toast.success(tAuth('auth.twoFactor.notifications.methodDisabled', { method: method.toUpperCase() }));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to disable ${method} 2FA`);
      console.error(`Failed to disable ${method} 2FA:`, err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Set primary 2FA method
  const setPrimaryMethod = useCallback(async (method: 'email' | 'sms' | 'totp'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.setPrimary({ method });
      await loadSettings();
  toast.success(tAuth('auth.twoFactor.notifications.primarySet', { method: method.toUpperCase() }));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to set primary method');
      console.error('Failed to set primary method:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Generate new backup codes
  const generateBackupCodes = useCallback(async (): Promise<string[]> => {
    setIsLoading(true);
    setError(null);
    
    try {
      const response = await authClient.twoFactor.generateBackupCodes();
      await loadSettings();
  toast.success(tAuth('auth.twoFactor.notifications.backupCodesGenerated'));
      return response.data.backupCodes || [];
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate backup codes');
      console.error('Failed to generate backup codes:', err);
      return [];
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Verify backup code
  const verifyBackupCode = useCallback(async (code: string): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.verifyBackupCode({ code });
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid backup code');
      console.error('Failed to verify backup code:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient]);

  // View existing backup codes
  const viewBackupCodes = useCallback(async (): Promise<string[]> => {
    setIsLoading(true);
    setError(null);
    
    try {
      const response = await authClient.twoFactor.getBackupCodes();
      return response.data.backupCodes || [];
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to retrieve backup codes');
      console.error('Failed to retrieve backup codes:', err);
      return [];
    } finally {
      setIsLoading(false);
    }
  }, [authClient]);

  // Send verification code
  const sendVerificationCode = useCallback(async (method: 'email' | 'sms'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.sendCode({ method });
  toast.success(tAuth('auth.twoFactor.notifications.codeSentVia', { method }));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to send ${method} code`);
      console.error(`Failed to send ${method} code:`, err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, tAuth]);

  // Verify 2FA code
  const verifyCode = useCallback(async (code: string, method: 'email' | 'sms' | 'totp'): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.verify({ code, method });
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid verification code');
      console.error('Failed to verify code:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient]);

  // Disable all 2FA methods (emergency)
  const disableAllMethods = useCallback(async (recoveryCode: string): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.disableAll({ recoveryCode });
      await loadSettings();
  toast.success(tAuth('auth.twoFactor.notifications.allDisabled'));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to disable 2FA');
      console.error('Failed to disable all 2FA methods:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Reset 2FA completely
  const resetTwoFactor = useCallback(async (): Promise<boolean> => {
    setIsLoading(true);
    setError(null);
    
    try {
      await authClient.twoFactor.reset();
      await loadSettings();
      setSetupData(null);
  toast.success(tAuth('auth.twoFactor.notifications.resetSuccess'));
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to reset 2FA');
      console.error('Failed to reset 2FA:', err);
      return false;
    } finally {
      setIsLoading(false);
    }
  }, [authClient, loadSettings, tAuth]);

  // Refresh settings
  const refresh = useCallback(async (): Promise<void> => {
    await loadSettings();
  }, [loadSettings]);

  // Check if method is available
  const isMethodAvailable = useCallback((method: 'email' | 'sms' | 'totp'): boolean => {
    if (!settings) return false;
    
    const methodInfo = settings.methods.find(m => m.type === method);
    return methodInfo ? methodInfo.enabled : false;
  }, [settings]);

  // Get method status
  const getMethodStatus = useCallback((method: 'email' | 'sms' | 'totp'): TwoFactorMethod | null => {
    if (!settings) return null;
    
    return settings.methods.find(m => m.type === method) || null;
  }, [settings]);

  // Load settings on mount and user change
  useEffect(() => {
    if (user) {
      loadSettings();
    } else {
      setSettings(null);
      setSetupData(null);
    }
  }, [user, loadSettings]);

  return {
    // State
    settings,
    isLoading,
    isSettingUp,
    error,
    
    // Setup operations
    initializeSetup,
    verifySetup,
    completeSetup,
    
    // Management operations
    enable,
    disable,
    setPrimaryMethod,
    
    // Backup codes
    generateBackupCodes,
    verifyBackupCode,
    viewBackupCodes,
    
    // Verification
    sendVerificationCode,
    verifyCode,
    
    // Recovery
    disableAllMethods,
    resetTwoFactor,
    
    // Utilities
    refresh,
    isMethodAvailable,
    getMethodStatus,
  };
}
