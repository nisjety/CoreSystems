'use client';

import React, { useState } from 'react';
import { 
  Shield, 
  ShieldCheck, 
  ShieldX, 
  Smartphone, 
  Mail, 
  Key, 
  RefreshCw, 
  Settings,
  AlertTriangle,
  Eye,
  EyeOff,
  Copy,
  Check
} from 'lucide-react';
import { Button } from '../ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Badge } from '../ui/badge';
import { Label } from '../ui/label';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';
import { 
  useTwoFactorMethods, 
  useToggleTwoFactorMethod,
  useRecoveryCodes,
  useRegenerateRecoveryCodes
} from '../lib/api/auth-provider-hooks';
import { TotpSetup } from './TotpSetup';

// Simple switch component since UI library switch isn't available
interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  'aria-label'?: string;
}

function Switch({ checked, onCheckedChange, disabled = false, 'aria-label': ariaLabel }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={`
        relative inline-flex h-6 w-11 items-center rounded-full transition-colors
        focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2
        ${checked ? 'bg-primary' : 'bg-muted-foreground/30'}
        ${disabled ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}
      `}
    >
      <span
        className={`
          inline-block h-4 w-4 transform rounded-full bg-white transition-transform
          ${checked ? 'translate-x-6' : 'translate-x-1'}
        `}
      />
    </button>
  );
}

interface TwoFactorManagementProps {
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * User's email for context
   */
  userEmail?: string;
  
  /**
   * Callback fired when 2FA settings change
   */
  onSettingsChanged?: () => void;
}

/**
 * TwoFactorManagement Component
 * 
 * Comprehensive 2FA management interface with:
 * - ✅ ORPC integration with Better Auth backend
 * - ✅ Enable/disable 2FA methods (TOTP, Email OTP, SMS OTP)
 * - ✅ Recovery codes management
 * - ✅ Real-time status updates
 * - ✅ Accessibility (WCAG 2.1 AA compliant)
 * - ✅ Design law compliance (clear visual hierarchy, grouped functionality)
 * - ✅ Comprehensive error handling
 * - ✅ Security best practices display
 * 
 * @example
 * ```tsx
 * <TwoFactorManagement
 *   userEmail="user@example.com"
 *   onSettingsChanged={() => console.log('2FA settings updated')}
 * />
 * ```
 */
export function TwoFactorManagement({
  className = '',
  userEmail = '',
  onSettingsChanged,
}: TwoFactorManagementProps) {
  const { t } = useTranslationWithInterpolation();
  const [showRecoveryCodes, setShowRecoveryCodes] = useState(false);
  const [showTotpSetup, setShowTotpSetup] = useState(false);
  const [copiedCodes, setCopiedCodes] = useState(false);
  
  // Hooks for 2FA management
  const twoFactorMethods = useTwoFactorMethods();
  const toggleMethod = useToggleTwoFactorMethod();
  const recoveryCodes = useRecoveryCodes();
  const regenerateCodes = useRegenerateRecoveryCodes();

  const handleToggleMethod = async (methodId: string, enabled: boolean) => {
    try {
      await toggleMethod.mutateAsync({ methodId, enabled });
      onSettingsChanged?.();
  console.log(`2FA method ${methodId} ${enabled ? 'enabled' : 'disabled'}`);
    } catch {
      console.error(`Failed to ${enabled ? 'enable' : 'disable'} 2FA method ${methodId}`);
    }
  };

  const handleRegenerateCodes = async () => {
  if (window.confirm(t('auth.twoFactor.management.recovery.confirmRegenerate'))) {
      try {
        await regenerateCodes.mutateAsync(true);
        console.log('Recovery codes regenerated successfully');
      } catch {
        console.error('Failed to regenerate recovery codes');
      }
    }
  };

  const handleCopyRecoveryCodes = async () => {
    if (recoveryCodes.data?.codes) {
      try {
        const codesText = recoveryCodes.data.codes.join('\n');
        await navigator.clipboard.writeText(codesText);
        setCopiedCodes(true);
        console.log('Recovery codes copied to clipboard');
        setTimeout(() => setCopiedCodes(false), 2000);
      } catch {
        console.error('Failed to copy recovery codes');
      }
    }
  };

  const getMethodIcon = (type: string) => {
    switch (type) {
      case 'totp': return <Shield className="w-5 h-5" />;
      case 'email': return <Mail className="w-5 h-5" />;
      case 'sms': return <Smartphone className="w-5 h-5" />;
      default: return <Key className="w-5 h-5" />;
    }
  };

  const getMethodName = (type: string) => {
    switch (type) {
      case 'totp': return t('auth.twoFactor.methods.totp');
      case 'email': return t('auth.twoFactor.methods.email');
      case 'sms': return t('auth.twoFactor.methods.sms');
      default: return t('auth.twoFactor.methods.recovery');
    }
  };

  const getMethodDescription = (type: string) => {
    switch (type) {
      case 'totp': return t('auth.twoFactor.management.methodDescriptions.totp');
      case 'email': return t('auth.twoFactor.management.methodDescriptions.email');
      case 'sms': return t('auth.twoFactor.management.methodDescriptions.sms');
      default: return t('auth.twoFactor.management.methodDescriptions.default');
    }
  };

  if (showTotpSetup) {
    return (
      <div className={className}>
        <TotpSetup
          userEmail={userEmail}
          onSetupComplete={() => {
            setShowTotpSetup(false);
            twoFactorMethods.refetch();
            onSettingsChanged?.();
          }}
          onCancel={() => setShowTotpSetup(false)}
        />
      </div>
    );
  }

  if (twoFactorMethods.isPending) {
    return (
      <Card className={`w-full max-w-2xl mx-auto ${className}`}>
        <CardHeader className="text-center">
          <div className="mx-auto mb-4 w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
            <RefreshCw className="w-6 h-6 text-primary animate-spin" />
          </div>
          <CardTitle>{t('common.loading')}</CardTitle>
          <CardDescription>{t('auth.twoFactor.management.headerDescription')}</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  if (twoFactorMethods.isError) {
    return (
      <Card className={`w-full max-w-2xl mx-auto ${className}`}>
        <CardHeader className="text-center">
          <CardTitle className="text-destructive">{t('common.error')}</CardTitle>
          <CardDescription>{t('auth.twoFactor.management.toggleError')}</CardDescription>
        </CardHeader>
        <CardContent>
          <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md mb-4">
            <p className="text-sm text-destructive">
              {twoFactorMethods.error?.message || t('auth.totp.error.unexpected')}
            </p>
          </div>
          <Button onClick={() => twoFactorMethods.refetch()} className="w-full">
            <RefreshCw className="w-4 h-4 mr-2" />
            {t('auth.totp.error.retry')}
          </Button>
        </CardContent>
      </Card>
    );
  }

  const methods = twoFactorMethods.data?.methods || [];
  const enabledMethodsCount = methods.filter(m => m.isEnabled).length;
  const hasTotp = methods.some(m => m.type === 'totp');

  return (
    <div className={`space-y-6 ${className}`}>
      {/* Header */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-3">
            <div className="w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
              <Settings className="w-6 h-6 text-primary" />
            </div>
            <div className="flex-1">
              <CardTitle className="flex items-center gap-2">
                {t('auth.twoFactor.title')}
                {enabledMethodsCount > 0 ? (
                  <Badge variant="default" className="bg-green-100 text-green-800">
                    <ShieldCheck className="w-3 h-3 mr-1" />
                    {t('auth.twoFactor.management.active')}
                  </Badge>
                ) : (
                  <Badge variant="secondary" className="bg-yellow-100 text-yellow-800">
                    <ShieldX className="w-3 h-3 mr-1" />
                    {t('auth.twoFactor.management.inactive')}
                  </Badge>
                )}
              </CardTitle>
              <CardDescription>
                {t('auth.twoFactor.management.headerDescription')}
              </CardDescription>
            </div>
          </div>
        </CardHeader>

        {enabledMethodsCount === 0 && (
          <CardContent>
            <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-md">
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-5 h-5 text-yellow-600 mt-0.5" />
                <div>
                  <h4 className="font-medium text-yellow-800">
                    {t('auth.twoFactor.management.enablePrompt.title')}
                  </h4>
                  <p className="text-sm text-yellow-700 mt-1">
                    {t('auth.twoFactor.management.enablePrompt.description')}
                  </p>
                </div>
              </div>
            </div>
          </CardContent>
        )}
      </Card>

      {/* Available Methods */}
      <Card>
        <CardHeader>
          <CardTitle>{t('auth.twoFactor.management.availableMethods.title')}</CardTitle>
          <CardDescription>{t('auth.twoFactor.management.availableMethods.description')}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {methods.map((method) => (
            <div
              key={method.id}
              className="flex items-center justify-between p-4 border border-border rounded-lg"
            >
              <div className="flex items-center gap-3">
                <div className={`w-10 h-10 rounded-full flex items-center justify-center ${
                  method.isEnabled 
                    ? 'bg-green-100 text-green-600' 
                    : 'bg-muted text-muted-foreground'
                }`}>
                  {getMethodIcon(method.type)}
                </div>
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    <h3 className="font-medium">{getMethodName(method.type)}</h3>
                    {method.isEnabled && (
                      <Badge variant="outline" className="text-xs">
                        {t('auth.twoFactor.management.method.enabled')}
                      </Badge>
                    )}
                  </div>
                  <p className="text-sm text-muted-foreground">
                    {getMethodDescription(method.type)}
                  </p>
                  {method.lastUsed && (
                    <p className="text-xs text-muted-foreground mt-1">
                      {t('auth.twoFactor.management.method.lastUsedPrefix')}{new Date(method.lastUsed).toLocaleDateString()}
                    </p>
                  )}
                </div>
              </div>

              <div className="flex items-center gap-3">
                {method.type === 'totp' && !method.isEnabled && (
                  <Button
                    onClick={() => setShowTotpSetup(true)}
                    size="sm"
                    variant="outline"
                  >
                    {t('auth.twoFactor.management.method.setUp')}
                  </Button>
                )}
                
                <div className="flex items-center gap-2">
                  <Switch
                    checked={method.isEnabled}
                    onCheckedChange={(enabled: boolean) => handleToggleMethod(method.id, enabled)}
                    disabled={toggleMethod.isPending}
                    aria-label={`${method.isEnabled ? t('common.disable') : t('common.enable')} ${getMethodName(method.type)}`}
                  />
                  <Label className="sr-only">
                    {method.isEnabled ? t('common.disable') : t('common.enable')} {getMethodName(method.type)}
                  </Label>
                </div>
              </div>
            </div>
          ))}

          {!hasTotp && (
            <div className="p-4 border-2 border-dashed border-muted-foreground/25 rounded-lg">
              <div className="text-center">
                <Shield className="w-8 h-8 text-muted-foreground mx-auto mb-2" />
                <h3 className="font-medium mb-1">{t('auth.twoFactor.management.addAuthenticator.title')}</h3>
                <p className="text-sm text-muted-foreground mb-3">
                  {t('auth.twoFactor.management.addAuthenticator.description')}
                </p>
                <Button onClick={() => setShowTotpSetup(true)} size="sm">
                  <Shield className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.management.addAuthenticator.action')}
                </Button>
              </div>
            </div>
          )}

          {toggleMethod.isError && (
            <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
              <p className="text-sm text-destructive">
                {toggleMethod.error?.message || t('auth.twoFactor.management.toggleError')}
              </p>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Recovery Codes */}
      {enabledMethodsCount > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Key className="w-5 h-5" />
              {t('auth.twoFactor.management.recovery.title')}
            </CardTitle>
            <CardDescription>
              {t('auth.twoFactor.management.recovery.description')}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {!showRecoveryCodes ? (
              <div className="space-y-3">
                <div className="p-4 bg-muted/50 border rounded-lg">
                  <div className="flex items-start gap-3">
                    <Key className="w-5 h-5 text-muted-foreground mt-0.5" />
                    <div className="flex-1">
                      <h4 className="font-medium">{t('auth.twoFactor.management.recovery.introTitle')}</h4>
                      <p className="text-sm text-muted-foreground mt-1">{t('auth.twoFactor.management.recovery.introDescription')}</p>
                    </div>
                  </div>
                </div>
                
                <div className="flex gap-2">
                  <Button 
                    onClick={() => {
                      setShowRecoveryCodes(true);
                      if (!recoveryCodes.data) {
                        recoveryCodes.refetch();
                      }
                    }}
                    variant="outline"
                    className="flex-1"
                  >
                    <Eye className="w-4 h-4 mr-2" />
                    {t('auth.twoFactor.management.recovery.viewCodes')}
                  </Button>
                  <Button 
                    onClick={handleRegenerateCodes}
                    disabled={regenerateCodes.isPending}
                    variant="outline"
                    className="flex-1"
                  >
                    {regenerateCodes.isPending ? (
                      <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                    ) : (
                      <RefreshCw className="w-4 h-4 mr-2" />
                    )}
                    {t('auth.twoFactor.management.recovery.generateNew')}
                  </Button>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <h4 className="font-medium">{t('auth.twoFactor.management.recovery.yourCodes')}</h4>
                  <Button
                    onClick={() => setShowRecoveryCodes(false)}
                    size="sm"
                    variant="outline"
                  >
                    <EyeOff className="w-4 h-4 mr-2" />
                    {t('auth.twoFactor.management.recovery.hide')}
                  </Button>
                </div>

                {recoveryCodes.isPending && (
                  <div className="text-center py-8">
                    <RefreshCw className="w-6 h-6 animate-spin mx-auto mb-2" />
                    <p className="text-sm text-muted-foreground">
                      {t('auth.twoFactor.management.recovery.loading')}
                    </p>
                  </div>
                )}

                {recoveryCodes.data?.codes && (
                  <>
                    <div className="p-4 bg-muted/50 border rounded-lg">
                      <div className="grid grid-cols-2 gap-2 font-mono text-sm">
                        {recoveryCodes.data.codes.map((code, index) => (
                          <div
                            key={index}
                            className="p-2 bg-background border rounded text-center"
                          >
                            {code}
                          </div>
                        ))}
                      </div>
                    </div>

                    <div className="p-3 bg-yellow-50 border border-yellow-200 rounded-md">
                      <div className="flex items-start gap-3">
                        <AlertTriangle className="w-4 h-4 text-yellow-600 mt-0.5" />
                        <div className="text-sm">
                          <p className="font-medium text-yellow-800">
                            {t('auth.twoFactor.management.recovery.storeSafelyTitle')}
                          </p>
                          <p className="text-yellow-700 mt-1">
                            {t('auth.twoFactor.management.recovery.storeSafelyDescription')}
                          </p>
                        </div>
                      </div>
                    </div>

                    <div className="flex gap-2">
                      <Button
                        onClick={handleCopyRecoveryCodes}
                        variant="outline"
                        className="flex-1"
                      >
                        {copiedCodes ? (
                          <Check className="w-4 h-4 mr-2" />
                        ) : (
                          <Copy className="w-4 h-4 mr-2" />
                        )}
                        {copiedCodes ? t('auth.twoFactor.management.recovery.copied') : t('auth.twoFactor.management.recovery.copyCodes')}
                      </Button>
                      <Button
                        onClick={handleRegenerateCodes}
                        disabled={regenerateCodes.isPending}
                        variant="outline"
                        className="flex-1"
                      >
                        {regenerateCodes.isPending ? (
                          <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                        ) : (
                          <RefreshCw className="w-4 h-4 mr-2" />
                        )}
                        {t('auth.twoFactor.management.recovery.generateNew')}
                      </Button>
                    </div>
                  </>
                )}

                {recoveryCodes.isError && (
                  <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                    <p className="text-sm text-destructive">
                      {recoveryCodes.error?.message || t('auth.twoFactor.management.recovery.loadError')}
                    </p>
                  </div>
                )}
              </div>
            )}

            {regenerateCodes.isError && (
              <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                <p className="text-sm text-destructive">
                  {regenerateCodes.error?.message || t('auth.twoFactor.management.recovery.regenerateError')}
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* Security Tips */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <ShieldCheck className="w-5 h-5" />
            {t('auth.twoFactor.management.tips.title')}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-3 text-sm">
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.management.tips.tip1')}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.management.tips.tip2')}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.management.tips.tip3')}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.management.tips.tip4')}</p>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
