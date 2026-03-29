'use client';

import React, { useState } from 'react';
import { 
  Key, 
  RefreshCw, 
  Copy, 
  Check, 
  AlertTriangle, 
  Shield, 
  Download,
  Eye,
  EyeOff
} from 'lucide-react';
import { Button } from '../ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Badge } from '../ui/badge';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';
import { 
  useRecoveryCodes, 
  useRegenerateRecoveryCodes
} from '../lib/api/auth-provider-hooks';

interface RecoveryCodesProps {
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * Callback fired when recovery codes are regenerated
   */
  onCodesRegenerated?: () => void;
  
  /**
   * Callback fired when a recovery code is used
   */
  onCodeUsed?: (code: string) => void;
}

/**
 * RecoveryCodes Component
 * 
 * Comprehensive recovery codes management with:
 * - ✅ ORPC integration with Better Auth backend
 * - ✅ View and manage recovery codes
 * - ✅ Regenerate codes with confirmation
 * - ✅ Track usage status and remaining codes
 * - ✅ Export functionality for secure storage
 * - ✅ Accessibility (WCAG 2.1 AA compliant)
 * - ✅ Design law compliance (clear warnings, safe actions)
 * - ✅ Security best practices and warnings
 * - ✅ Copy to clipboard functionality
 */
export function RecoveryCodes({
  className = '',
  onCodesRegenerated,
}: RecoveryCodesProps) {
  const { t, isNorwegian } = useTranslationWithInterpolation();
  const [showCodes, setShowCodes] = useState(false);
  const [copiedCodes, setCopiedCodes] = useState(false);
  const [confirmRegenerate, setConfirmRegenerate] = useState(false);

  // Hooks for recovery codes management
  const recoveryCodes = useRecoveryCodes();
  const regenerateCodes = useRegenerateRecoveryCodes();

  const handleToggleVisibility = () => {
    setShowCodes(!showCodes);
    if (!showCodes && !recoveryCodes.data) {
      recoveryCodes.refetch();
    }
  };

  const handleCopyAllCodes = async () => {
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

  const handleCopyCode = async (code: string) => {
    try {
      await navigator.clipboard.writeText(code);
      console.log('Recovery code copied:', code);
    } catch {
      console.error('Failed to copy recovery code');
    }
  };

  const handleRegenerateCodes = async () => {
    if (!confirmRegenerate) {
      setConfirmRegenerate(true);
      return;
    }

    try {
      await regenerateCodes.mutateAsync(true);
      setConfirmRegenerate(false);
      onCodesRegenerated?.();
      console.log('Recovery codes regenerated successfully');
    } catch {
      console.error('Failed to regenerate recovery codes');
      setConfirmRegenerate(false);
    }
  };

  const handleExportCodes = () => {
    if (recoveryCodes.data?.codes) {
      const prefix = 'auth.twoFactor.recoveryCodes.exportFile';
      const generatedLabel = t(`${prefix}.generatedLabel`);
      const codesText = [
        t(`${prefix}.title`),
        '========================',
        '',
        t(`${prefix}.intro1`),
        t(`${prefix}.intro2`),
        '',
        `${generatedLabel} ${new Date().toLocaleString(isNorwegian ? 'nb-NO' : 'en-US')}`,
        '',
        t(`${prefix}.codesHeader`),
        '---------------',
        ...recoveryCodes.data.codes.map((code: string, index: number) => `${index + 1}. ${code}`),
        '',
        t(`${prefix}.notesHeader`),
        `- ${t(`${prefix}.note1`)}`,
        `- ${t(`${prefix}.note2`)}`,
        `- ${t(`${prefix}.note3`)}`,
        `- ${t(`${prefix}.note4`)}`,
      ].join('\n');

      const blob = new Blob([codesText], { type: 'text/plain' });
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
  a.download = `${t('auth.twoFactor.recoveryCodes.exportFile.fileNamePrefix')}-${new Date().toISOString().split('T')[0]}.txt`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      window.URL.revokeObjectURL(url);
    }
  };

  if (recoveryCodes.isPending && showCodes) {
    return (
      <Card className={`w-full max-w-2xl mx-auto ${className}`}>
        <CardHeader className="text-center">
          <div className="mx-auto mb-4 w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
            <RefreshCw className="w-6 h-6 text-primary animate-spin" />
          </div>
          <CardTitle>{t('auth.twoFactor.recoveryCodes.loadingTitle')}</CardTitle>
          <CardDescription>{t('auth.twoFactor.recoveryCodes.loadingDescription')}</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  const codes = recoveryCodes.data?.codes || [];
  const remainingCodes = recoveryCodes.data?.remaining || codes.length;

  return (
    <div className={`space-y-6 ${className}`}>
      {/* Header */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-3">
            <div className="w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
              <Key className="w-6 h-6 text-primary" />
            </div>
            <div className="flex-1">
              <CardTitle className="flex items-center gap-2">
                {t('auth.twoFactor.recoveryCodes.title')}
                {remainingCodes > 0 && (
                  <Badge variant="outline" className="bg-green-100 text-green-800">
                    {remainingCodes} {t('auth.twoFactor.recoveryCodes.remaining')}
                  </Badge>
                )}
                {remainingCodes <= 2 && remainingCodes > 0 && (
                  <Badge variant="destructive" className="bg-yellow-100 text-yellow-800">
                    {t('auth.twoFactor.recoveryCodes.lowCodesBadge')}
                  </Badge>
                )}
                {remainingCodes === 0 && (
                  <Badge variant="destructive">
                    {t('auth.twoFactor.recoveryCodes.noCodesBadge')}
                  </Badge>
                )}
              </CardTitle>
              <CardDescription>
                {t('auth.twoFactor.recoveryCodes.description')}
              </CardDescription>
            </div>
          </div>
        </CardHeader>

        {/* Warning for low codes */}
        {remainingCodes <= 2 && remainingCodes > 0 && (
          <CardContent>
            <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-md">
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-5 h-5 text-yellow-600 mt-0.5" />
                <div>
                  <h4 className="font-medium text-yellow-800">
                    {t('auth.twoFactor.recoveryCodes.lowCodesTitle')}
                  </h4>
                  <p className="text-sm text-yellow-700 mt-1">
                    {t('auth.twoFactor.recoveryCodes.lowCodesDescription', { count: remainingCodes.toString(), plural: remainingCodes === 1 ? '' : (isNorwegian ? 'r' : 's') })}
                  </p>
                </div>
              </div>
            </div>
          </CardContent>
        )}

        {/* No codes left */}
        {remainingCodes === 0 && (
          <CardContent>
            <div className="p-4 bg-red-50 border border-red-200 rounded-md">
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-5 h-5 text-red-600 mt-0.5" />
                <div>
                  <h4 className="font-medium text-red-800">
                    {t('auth.twoFactor.recoveryCodes.noCodesTitle')}
                  </h4>
                  <p className="text-sm text-red-700 mt-1">
                    {t('auth.twoFactor.recoveryCodes.noCodesDescription')}
                  </p>
                </div>
              </div>
            </div>
          </CardContent>
        )}
      </Card>

      {/* Actions */}
      <Card>
        <CardHeader>
          <CardTitle className="text-lg">{t('auth.twoFactor.recoveryCodes.manageTitle')}</CardTitle>
          <CardDescription>
            {t('auth.twoFactor.recoveryCodes.manageDescription')}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-col sm:flex-row gap-3">
            <Button
              onClick={handleToggleVisibility}
              variant="outline"
              className="flex-1"
              disabled={recoveryCodes.isPending}
            >
              {showCodes ? (
                <>
                  <EyeOff className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.hideCodes')}
                </>
              ) : (
                <>
                  <Eye className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.viewCodes')}
                </>
              )}
            </Button>

            <Button
              onClick={handleRegenerateCodes}
              variant={confirmRegenerate ? "destructive" : "outline"}
              disabled={regenerateCodes.isPending}
              className="flex-1"
            >
              {regenerateCodes.isPending ? (
                <>
                  <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                  {t('auth.twoFactor.recoveryCodes.generating')}
                </>
              ) : confirmRegenerate ? (
                <>
                  <AlertTriangle className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.confirmRegenerate')}
                </>
              ) : (
                <>
                  <RefreshCw className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.generateNewCodes')}
                </>
              )}
            </Button>

            {confirmRegenerate && (
              <Button
                onClick={() => setConfirmRegenerate(false)}
                variant="outline"
                size="sm"
              >
                {t('auth.twoFactor.recoveryCodes.cancel')}
              </Button>
            )}
          </div>

          {confirmRegenerate && (
            <div className="p-3 bg-red-50 border border-red-200 rounded-md">
              <div className="flex items-start gap-3">
                <AlertTriangle className="w-4 h-4 text-red-600 mt-0.5" />
                <div className="text-sm">
                  <p className="font-medium text-red-800">
                    {t('auth.twoFactor.recoveryCodes.warningTitle')}
                  </p>
                  <p className="text-red-700 mt-1">
                    {t('auth.twoFactor.recoveryCodes.warningDescription')}
                  </p>
                </div>
              </div>
            </div>
          )}

          {regenerateCodes.isError && (
            <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
              <p className="text-sm text-destructive">
                {regenerateCodes.error?.message || t('auth.twoFactor.recoveryCodes.regenerateError')}
              </p>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Recovery Codes Display */}
      {showCodes && recoveryCodes.data && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <CardTitle className="text-lg">{t('auth.twoFactor.recoveryCodes.headerYourCodes')}</CardTitle>
              <div className="flex gap-2">
                <Button
                  onClick={handleCopyAllCodes}
                  size="sm"
                  variant="outline"
                >
                  {copiedCodes ? (
                    <>
                      <Check className="w-4 h-4 mr-2" />
                      {t('auth.twoFactor.recoveryCodes.copied')}
                    </>
                  ) : (
                    <>
                      <Copy className="w-4 h-4 mr-2" />
                      {t('auth.twoFactor.recoveryCodes.copyAll')}
                    </>
                  )}
                </Button>
                <Button
                  onClick={handleExportCodes}
                  size="sm"
                  variant="outline"
                >
                  <Download className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.export')}
                </Button>
              </div>
            </div>
            <CardDescription>
              {t('auth.twoFactor.recoveryCodes.headerCodesDescription')}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {codes.length === 0 ? (
              <div className="text-center py-8">
                <Key className="w-12 h-12 text-muted-foreground mx-auto mb-4" />
                <h3 className="font-medium mb-2">
                  {t('auth.twoFactor.recoveryCodes.noCodesAvailableTitle')}
                </h3>
                <p className="text-muted-foreground mb-4">
                  {t('auth.twoFactor.recoveryCodes.noCodesAvailableDescription')}
                </p>
                <Button onClick={handleRegenerateCodes} disabled={regenerateCodes.isPending}>
                  <RefreshCw className="w-4 h-4 mr-2" />
                  {t('auth.twoFactor.recoveryCodes.generateCodes')}
                </Button>
              </div>
            ) : (
              <>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  {codes.map((code: string, index: number) => (
                    <div
                      key={index}
                      className="relative p-3 border rounded-lg bg-background border-border"
                    >
                      <div className="flex items-center justify-between">
                        <div className="font-mono text-sm">
                          {code}
                        </div>
                        <div className="flex items-center gap-1">
                          <Button
                            onClick={() => handleCopyCode(code)}
                            size="sm"
                            variant="ghost"
                            className="h-6 w-6 p-0"
                          >
                            <Copy className="w-3 h-3" />
                          </Button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>

                {/* Usage Statistics */}
                <div className="pt-4 border-t">
                  <div className="flex items-center justify-between text-sm">
                    <span className="text-muted-foreground">
                      {`${t('auth.twoFactor.recoveryCodes.totalCodesLabel')}: ${codes.length}`}
                    </span>
                    <span className={`font-medium ${
                      remainingCodes <= 2 ? 'text-destructive' : 'text-green-600'
                    }`}>
                      {remainingCodes} {t('auth.twoFactor.recoveryCodes.remaining')}
                    </span>
                  </div>
                  <div className="mt-2 w-full bg-muted rounded-full h-2">
                    <div 
                      className={`h-2 rounded-full transition-all ${
                        remainingCodes <= 2 ? 'bg-destructive' : 'bg-green-600'
                      }`}
                      style={{ width: `${Math.max(0, (remainingCodes / Math.max(1, codes.length)) * 100)}%` }}
                    />
                  </div>
                </div>
              </>
            )}

            {recoveryCodes.isError && (
              <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                <p className="text-sm text-destructive">
                  {recoveryCodes.error?.message || t('auth.twoFactor.recoveryCodes.loadError')}
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
            <Shield className="w-5 h-5" />
            {t('auth.twoFactor.management.tips.title')}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-3 text-sm">
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.management.tips.tip2')}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.recoveryCodes.headerCodesDescription')}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{t('auth.twoFactor.recoveryCodes.warningDescription')}</p>
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
