"use client";

import { useState, useEffect, useCallback } from 'react';
import { useAuth } from './use-auth';
import { toast } from 'sonner';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';

interface SecurityDevice { id: string; name: string; type: 'browser' | 'mobile' | 'desktop' | 'api' | 'unknown'; platform: string; browser?: string; os?: string; location?: { country: string; city: string; ip: string }; lastActive: Date; trusted: boolean; current: boolean; createdAt: Date; }
interface SecuritySession { id: string; deviceId: string; ipAddress: string; userAgent: string; location?: { country: string; city: string; region: string }; startedAt: Date; lastActive: Date; expiresAt: Date; active: boolean; current: boolean; }
interface AuditLogEntry { id: string; action: string; category: 'auth' | 'profile' | 'security' | 'data' | 'admin'; details: Record<string, unknown>; ipAddress: string; userAgent: string; location?: { country: string; city: string }; risk: 'low' | 'medium' | 'high'; timestamp: Date; success: boolean; }
interface SecuritySettings { sessionTimeout: number; maxConcurrentSessions: number; requireSecureHeaders: boolean; rememberDevices: boolean; deviceTrustDuration: number; requireDeviceApproval: boolean; maxFailedAttempts: number; lockoutDuration: number; requirePasswordChange: boolean; passwordChangeInterval: number; allowApiAccess: boolean; ipWhitelist: string[]; enableAuditLogging: boolean; dataRetentionDays: number; emailSecurityAlerts: boolean; smsSecurityAlerts: boolean; allowDataCollection: boolean; shareUsageData: boolean; allowMarketingEmails: boolean; }
interface SecurityPreferences { notifications: { email: boolean; sms: boolean; push: boolean }; privacy: { dataCollection: boolean; analytics: boolean; marketing: boolean }; advanced: { ipLogging: boolean; deviceFingerprinting: boolean; locationTracking: boolean }; }
interface UseSecuritySettingsReturn { settings: SecuritySettings | null; preferences: SecurityPreferences | null; devices: SecurityDevice[]; sessions: SecuritySession[]; auditLogs: AuditLogEntry[]; isLoading: boolean; error: string | null; updateSettings: (s: Partial<SecuritySettings>) => Promise<boolean>; updatePreferences: (p: Partial<SecurityPreferences>) => Promise<boolean>; resetToDefaults: () => Promise<boolean>; trustDevice: (id: string) => Promise<boolean>; untrustDevice: (id: string) => Promise<boolean>; removeDevice: (id: string) => Promise<boolean>; renameDevice: (id: string, name: string) => Promise<boolean>; terminateSession: (id: string) => Promise<boolean>; terminateAllSessions: (excludeCurrent?: boolean) => Promise<boolean>; extendSession: (id: string, minutes: number) => Promise<boolean>; changePassword: (current: string, next: string) => Promise<boolean>; enableTwoFactor: () => Promise<boolean>; disableTwoFactor: (code: string) => Promise<boolean>; generateRecoveryCodes: () => Promise<string[]>; addTrustedIP: (ip: string, description?: string) => Promise<boolean>; removeTrustedIP: (ip: string) => Promise<boolean>; getAuditLogs: (f?: { category?: string; startDate?: Date; endDate?: Date; limit?: number }) => Promise<AuditLogEntry[]>; exportData: (format: 'json' | 'csv') => Promise<string>; downloadAuditLog: (format: 'json' | 'csv') => Promise<void>; refresh: () => Promise<void>; isDeviceTrusted: (id: string) => boolean; isSessionActive: (id: string) => boolean; getSecurityScore: () => number; getRecommendations: () => string[]; }

 
const api = {
  getSettings: async (): Promise<SecuritySettings> => ({ sessionTimeout: 120, maxConcurrentSessions: 5, requireSecureHeaders: true, rememberDevices: true, deviceTrustDuration: 30, requireDeviceApproval: false, maxFailedAttempts: 5, lockoutDuration: 15, requirePasswordChange: false, passwordChangeInterval: 90, allowApiAccess: true, ipWhitelist: [], enableAuditLogging: true, dataRetentionDays: 180, emailSecurityAlerts: true, smsSecurityAlerts: false, allowDataCollection: true, shareUsageData: false, allowMarketingEmails: false }),
  getPreferences: async (): Promise<SecurityPreferences> => ({ notifications: { email: true, sms: false, push: true }, privacy: { dataCollection: true, analytics: false, marketing: false }, advanced: { ipLogging: true, deviceFingerprinting: false, locationTracking: true } }),
  updateSettings: async (_settings: Partial<SecuritySettings>) => true,
  updatePreferences: async (_prefs: Partial<SecurityPreferences>) => true,
  resetToDefaults: async () => true,
  getDevices: async (): Promise<SecurityDevice[]> => [],
  trustDevice: async (_deviceId: string) => true,
  untrustDevice: async (_deviceId: string) => true,
  removeDevice: async (_deviceId: string) => true,
  renameDevice: async (_deviceId: string, _name: string) => true,
  getSessions: async (): Promise<SecuritySession[]> => [],
  terminateSession: async (_sessionId: string) => true,
  terminateAllSessions: async (_excludeCurrent: boolean) => true,
  extendSession: async (_sessionId: string, _minutes: number) => true,
  changePassword: async (_current: string, _next: string) => true,
  enableTwoFactor: async () => true,
  disableTwoFactor: async (_code: string) => true,
  generateRecoveryCodes: async (): Promise<string[]> => ['RCODE-1', 'RCODE-2'],
  addTrustedIP: async (_ip: string, _description?: string) => true,
  removeTrustedIP: async (_ip: string) => true,
  getAuditLogs: async (_filters?: { category?: string; startDate?: Date; endDate?: Date; limit?: number }): Promise<AuditLogEntry[]> => [],
  exportData: async (_format: 'json' | 'csv') => '{}',
  downloadAuditLog: async (_format: 'json' | 'csv') => true
};
 

export function useSecuritySettings(): UseSecuritySettingsReturn {
  const { user } = useAuth();
  const { t } = useTranslationWithInterpolation();

  const [settings, setSettings] = useState<SecuritySettings | null>(null);
  const [preferences, setPreferences] = useState<SecurityPreferences | null>(null);
  const [devices, setDevices] = useState<SecurityDevice[]>([]);
  const [sessions, setSessions] = useState<SecuritySession[]>([]);
  const [auditLogs, setAuditLogs] = useState<AuditLogEntry[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSettings = useCallback(async () => {
    if (!user) return;
    setIsLoading(true); setError(null);
    try { const [s,p] = await Promise.all([api.getSettings(), api.getPreferences()]); setSettings(s); setPreferences(p); } catch (e) { setError(e instanceof Error ? e.message : 'Failed to load security settings'); } finally { setIsLoading(false); }
  }, [user]);

  const loadDevicesAndSessions = useCallback(async () => {
    if (!user) return;
  try { const [d,s] = await Promise.all([api.getDevices(), api.getSessions()]); setDevices(d); setSessions(s); } catch { /* noop */ }
  }, [user]);

  const updateSettings = useCallback(async (s: Partial<SecuritySettings>) => { setIsLoading(true); setError(null); try { await api.updateSettings(s); setSettings(p=>p?{...p,...s}:null); toast.success(t('auth.security.notifications.securitySettingsUpdated')); return true; } catch(e){ setError(e instanceof Error? e.message:'Failed to update settings'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Update security preferences
  const updatePreferences = useCallback(async (p: Partial<SecurityPreferences>) => { setIsLoading(true); setError(null); try { await api.updatePreferences(p); setPreferences(prev=>prev?{...prev,...p}:null); toast.success(t('auth.security.notifications.securityPreferencesUpdated')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to update preferences'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Reset to defaults
  const resetToDefaults = useCallback(async () => { setIsLoading(true); setError(null); try { await api.resetToDefaults(); await loadSettings(); toast.success(t('auth.security.notifications.settingsReset')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to reset settings'); return false;} finally{ setIsLoading(false);} }, [loadSettings, t]);

  // Trust device
  const trustDevice = useCallback(async (id: string) => { setIsLoading(true); setError(null); try { await api.trustDevice(id); setDevices(p=>p.map(d=>d.id===id?{...d,trusted:true}:d)); toast.success(t('auth.security.notifications.deviceTrusted')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to trust device'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Untrust device
  const untrustDevice = useCallback(async (id: string) => { setIsLoading(true); setError(null); try { await api.untrustDevice(id); setDevices(p=>p.map(d=>d.id===id?{...d,trusted:false}:d)); toast.success(t('auth.security.notifications.deviceUntrusted')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to untrust device'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Remove device
  const removeDevice = useCallback(async (id: string) => { setIsLoading(true); setError(null); try { await api.removeDevice(id); setDevices(p=>p.filter(d=>d.id!==id)); toast.success(t('auth.security.notifications.deviceRemoved')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to remove device'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Rename device
  const renameDevice = useCallback(async (id: string, name: string) => { setIsLoading(true); setError(null); try { await api.renameDevice(id,name); setDevices(p=>p.map(d=>d.id===id?{...d,name}:d)); toast.success(t('auth.security.notifications.deviceRenamed')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to rename device'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Terminate session
  const terminateSession = useCallback(async (id: string) => { setIsLoading(true); setError(null); try { await api.terminateSession(id); setSessions(p=>p.filter(s=>s.id!==id)); toast.success(t('auth.security.notifications.sessionTerminated')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to terminate session'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Terminate all sessions
  const terminateAllSessions = useCallback(async (excludeCurrent=true) => { setIsLoading(true); setError(null); try { await api.terminateAllSessions(excludeCurrent); setSessions(prev=> excludeCurrent ? prev.filter(s=>s.current): []); toast.success(t('auth.security.notifications.sessionsTerminated')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to terminate sessions'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Extend session
  const extendSession = useCallback(async (id: string, minutes: number) => { setIsLoading(true); setError(null); try { await api.extendSession(id,minutes); const expires=new Date(Date.now()+minutes*60000); setSessions(p=>p.map(s=>s.id===id?{...s,expiresAt:expires}:s)); toast.success(t('auth.security.notifications.sessionExtended')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to extend session'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Change password
  const changePassword = useCallback(async (current: string, next: string) => { setIsLoading(true); setError(null); try { await api.changePassword(current,next); toast.success(t('auth.security.notifications.passwordChanged')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to change password'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Enable two-factor authentication (localized toast)
  const enableTwoFactor = useCallback(async () => { setIsLoading(true); setError(null); try { await api.enableTwoFactor(); toast.success(t('auth.twoFactor.notifications.enableSuccess')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to enable 2FA'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Disable two-factor authentication (localized toast)
  const disableTwoFactor = useCallback(async (code: string) => { setIsLoading(true); setError(null); try { await api.disableTwoFactor(code); toast.success(t('auth.twoFactor.notifications.disableSuccess')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to disable 2FA'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Generate recovery codes (localized toast)
  const generateRecoveryCodes = useCallback(async () => { setIsLoading(true); setError(null); try { const codes = await api.generateRecoveryCodes(); toast.success(t('auth.twoFactor.notifications.backupCodesGenerated')); return codes; } catch(e){ setError(e instanceof Error? e.message:'Failed to generate recovery codes'); return []; } finally { setIsLoading(false);} }, [t]);

  // Add trusted IP
  const addTrustedIP = useCallback(async (ip: string, description?: string) => { setIsLoading(true); setError(null); try { await api.addTrustedIP(ip,description); toast.success(t('auth.security.notifications.trustedIpAdded')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to add trusted IP'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Remove trusted IP
  const removeTrustedIP = useCallback(async (ip: string) => { setIsLoading(true); setError(null); try { await api.removeTrustedIP(ip); toast.success(t('auth.security.notifications.trustedIpRemoved')); return true;} catch(e){ setError(e instanceof Error? e.message:'Failed to remove trusted IP'); return false;} finally{ setIsLoading(false);} }, [t]);

  // Get audit logs
  const getAuditLogs = useCallback(async (filters?: { category?: string; startDate?: Date; endDate?: Date; limit?: number }) => { setIsLoading(true); setError(null); try { const logs = await api.getAuditLogs(filters); setAuditLogs(logs); return logs;} catch(e){ setError(e instanceof Error? e.message:'Failed to load audit logs'); return []; } finally { setIsLoading(false);} }, []);

  // Export data
  const exportData = useCallback(async (format: 'json' | 'csv') => { setIsLoading(true); setError(null); try { const data = await api.exportData(format); toast.success(t('auth.security.notifications.dataExported')); return data;} catch(e){ setError(e instanceof Error? e.message:'Failed to export data'); return '';} finally{ setIsLoading(false);} }, [t]);

  // Download audit log
  const downloadAuditLog = useCallback(async (format: 'json' | 'csv') => { setIsLoading(true); setError(null); try { await api.downloadAuditLog(format); toast.success(t('auth.security.notifications.auditLogDownloadStarted')); } catch(e){ setError(e instanceof Error? e.message:'Failed to download audit log'); } finally { setIsLoading(false);} }, [t]);

  // Refresh all data
  const refresh = useCallback(async () => { await Promise.all([loadSettings(), loadDevicesAndSessions()]); }, [loadSettings, loadDevicesAndSessions]);

  // Check if device is trusted
  const isDeviceTrusted = useCallback((id: string) => devices.some(d=>d.id===id && d.trusted), [devices]);

  // Check if session is active
  const isSessionActive = useCallback((id: string) => sessions.some(s=>s.id===id && s.active), [sessions]);

  // Calculate security score
  const getSecurityScore = useCallback(() => { if(!settings) return 0; let score=0; // @ts-expect-error conditional property
    if(user?.twoFactorEnabled) score+=25; if(settings.requirePasswordChange) score+=15; if(settings.sessionTimeout<=60) score+=15; if(settings.maxConcurrentSessions<=3) score+=10; if(settings.requireDeviceApproval) score+=15; if(settings.enableAuditLogging) score+=10; if(settings.emailSecurityAlerts) score+=10; return Math.min(100,score); }, [settings, user]);

  // Get security recommendations (2FA recommendation localized)
  const getRecommendations = useCallback(() => { const rec: string[] = []; // @ts-expect-error optional upstream property
    if(!user?.twoFactorEnabled) rec.push(t('auth.twoFactor.management.enablePrompt.title')); if(settings?.sessionTimeout && settings.sessionTimeout>120) rec.push('Consider reducing session timeout to 2 hours or less'); if(settings?.maxConcurrentSessions && settings.maxConcurrentSessions>5) rec.push('Limit concurrent sessions to reduce security risk'); if(!settings?.enableAuditLogging) rec.push('Enable audit logging to monitor account activity'); if(!settings?.emailSecurityAlerts) rec.push('Enable email security alerts for suspicious activity'); if(devices.some(d=>!d.trusted)) rec.push('Review and trust your devices for better security'); return rec; }, [settings, user, devices, t]);

  // Load data on mount and user change
  useEffect(()=>{ if(user){ loadSettings(); loadDevicesAndSessions(); } else { setSettings(null); setPreferences(null); setDevices([]); setSessions([]); setAuditLogs([]);} }, [user, loadSettings, loadDevicesAndSessions]);

  return {
    // State
    settings,
    preferences,
    devices,
    sessions,
    auditLogs,
    isLoading,
    error,
    
    // Settings management
    updateSettings,
    updatePreferences,
    resetToDefaults,
    
    // Device management
    trustDevice,
    untrustDevice,
    removeDevice,
    renameDevice,
    
    // Session management
    terminateSession,
    terminateAllSessions,
    extendSession,
    
    // Security actions
    changePassword,
    enableTwoFactor,
    disableTwoFactor,
    generateRecoveryCodes,
    
    // IP and access management
    addTrustedIP,
    removeTrustedIP,
    
    // Audit and monitoring
    getAuditLogs,
    exportData,
    downloadAuditLog,
    
    // Utilities
    refresh,
    isDeviceTrusted,
    isSessionActive,
    getSecurityScore,
    getRecommendations,
  };
}
