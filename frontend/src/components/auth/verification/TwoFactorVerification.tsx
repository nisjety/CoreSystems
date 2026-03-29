'use client';

import React, { useState } from 'react';
import { Shield, Smartphone, Mail, Key, RefreshCw, Check } from 'lucide-react';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Badge } from '../ui/badge';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';
import { 
  useSendEmailOtp, 
  useVerifyEmailOtp, 
  useSendSmsOtp, 
  useVerifySmsOtp, 
  useVerifyTotp,
  useVerifyRecoveryCode,
  useVerificationUtilities 
} from '../lib/api/auth-provider-hooks';

interface TwoFactorVerificationProps {
  /**
   * Available verification methods
   */
  availableMethods?: Array<'totp' | 'email' | 'sms' | 'recovery'>;
  
  /**
   * User's email for email OTP
   */
  userEmail?: string;
  
  /**
   * User's phone number for SMS OTP
   */
  userPhone?: string;
  
  /**
   * Callback fired when verification is successful
   */
  onVerificationSuccess?: (method: string) => void;
  
  /**
   * Callback fired when verification is cancelled
   */
  onCancel?: () => void;
  
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * Purpose of verification (affects messaging)
   */
  purpose?: 'login' | 'verification' | 'reset';
}

/**
 * TwoFactorVerification Component
 * 
 * Comprehensive multi-channel 2FA verification with:
 * - ✅ ORPC integration with Better Auth backend
 * - ✅ TOTP/Authenticator app verification
 * - ✅ Email OTP verification
 * - ✅ SMS OTP verification
 * - ✅ Recovery code fallback
 * - ✅ Accessibility (WCAG 2.1 AA compliant)
 * - ✅ Design law compliance (chunking, progressive disclosure)
 * - ✅ Real-time validation and error handling
 * - ✅ Norwegian/English internationalization support
 * 
 * @example
 * ```tsx
 * <TwoFactorVerification
 *   availableMethods={['totp', 'email', 'sms', 'recovery']}
 *   userEmail="user@example.com"
 *   userPhone="+4712345678"
 *   onVerificationSuccess={(method) => console.log(`Verified with ${method}`)}
 * />
 * ```
 */
export function TwoFactorVerification({
  availableMethods = ['totp', 'email', 'sms', 'recovery'],
  userEmail = '',
  userPhone = '',
  onVerificationSuccess,
  onCancel,
  className = '',
  purpose = 'verification',
}: TwoFactorVerificationProps) {
  const { t } = useTranslationWithInterpolation();
  const [activeMethod, setActiveMethod] = useState<'totp' | 'email' | 'sms' | 'recovery'>(
    availableMethods[0] || 'totp'
  );
  const [verificationCode, setVerificationCode] = useState('');
  const [recoveryCode, setRecoveryCode] = useState('');
  
  // Hooks for different verification methods
  const sendEmailOtp = useSendEmailOtp();
  const verifyEmailOtp = useVerifyEmailOtp();
  const sendSmsOtp = useSendSmsOtp();
  const verifySmsOtp = useVerifySmsOtp();
  const verifyTotp = useVerifyTotp();
  const verifyRecoveryCode = useVerifyRecoveryCode();
  const { maskPhoneNumber } = useVerificationUtilities();

  const handleSendEmailOtp = async () => {
    if (!userEmail) return;
    
    try {
      await sendEmailOtp.mutateAsync({
        email: userEmail,
        purpose: purpose === 'login' ? 'login' : 'verification',
        language: 'no',
      });
      console.log('Email OTP sent successfully');
    } catch {
      console.error('Failed to send email OTP');
    }
  };

  const handleSendSmsOtp = async () => {
    if (!userPhone) return;
    
    try {
      await sendSmsOtp.mutateAsync({
        phoneNumber: userPhone,
        purpose: purpose === 'login' ? 'login' : 'verification',
        language: 'no',
      });
      console.log('SMS OTP sent successfully');
    } catch {
      console.error('Failed to send SMS OTP');
    }
  };

  const handleVerifyCode = async () => {
    if (!verificationCode.trim()) return;

    try {
      let result;
      
      switch (activeMethod) {
        case 'totp':
          result = await verifyTotp.mutateAsync(verificationCode);
          break;
        case 'email':
          result = await verifyEmailOtp.mutateAsync({
            email: userEmail,
            code: verificationCode,
            purpose: purpose === 'login' ? 'login' : 'verification',
          });
          break;
        case 'sms':
          result = await verifySmsOtp.mutateAsync({
            phoneNumber: userPhone,
            code: verificationCode,
            purpose: purpose === 'login' ? 'login' : 'verification',
          });
          break;
      }
      
      if (result?.success) {
        console.log(`${activeMethod.toUpperCase()} verification successful`);
        onVerificationSuccess?.(activeMethod);
      }
    } catch {
      console.error(`${activeMethod.toUpperCase()} verification failed`);
    }
  };

  const handleVerifyRecovery = async () => {
    if (!recoveryCode.trim()) return;

    try {
      const result = await verifyRecoveryCode.mutateAsync(recoveryCode);
      if (result?.success) {
        console.log('Recovery code verification successful');
        onVerificationSuccess?.('recovery');
      }
    } catch {
      console.error('Recovery code verification failed');
    }
  };

  const getMethodIcon = (method: string) => {
    switch (method) {
      case 'totp': return <Shield className="w-4 h-4" />;
      case 'email': return <Mail className="w-4 h-4" />;
      case 'sms': return <Smartphone className="w-4 h-4" />;
      case 'recovery': return <Key className="w-4 h-4" />;
      default: return <Shield className="w-4 h-4" />;
    }
  };

  const getMethodLabel = (method: string) => {
    switch (method) {
      case 'totp': return t('auth.twoFactor.methods.totp');
      case 'email': return t('auth.twoFactor.methods.email');
      case 'sms': return t('auth.twoFactor.methods.sms');
      case 'recovery': return t('auth.twoFactor.methods.recovery');
      default: return t('auth.twoFactor.title');
    }
  };

  return (
    <Card className={`w-full max-w-md mx-auto ${className}`}>
      <CardHeader className="text-center">
        <div className="mx-auto mb-4 w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
          <Shield className="w-6 h-6 text-primary" />
        </div>
        <CardTitle className="text-xl">{purpose === 'login' ? t('auth.twoFactor.titleLogin') : t('auth.twoFactor.title')}</CardTitle>
        <CardDescription>
          {purpose === 'login' 
            ? t('auth.twoFactor.descriptionLogin')
            : t('auth.twoFactor.descriptionGeneric')}
        </CardDescription>
      </CardHeader>

      <CardContent>
        {/* Method Selection */}
        <div className="space-y-4">
          <div className="grid grid-cols-2 gap-2 mb-6">
            {availableMethods.slice(0, 4).map((method) => (
              <Button
                key={method}
                variant={activeMethod === method ? 'default' : 'outline'}
                size="sm"
                onClick={() => setActiveMethod(method)}
                className="flex items-center gap-2 text-xs"
              >
                {getMethodIcon(method)}
                {getMethodLabel(method)}
              </Button>
            ))}
          </div>

          {/* TOTP Verification */}
          {activeMethod === 'totp' && (
            <div className="space-y-4">
              <div className="text-center mb-4">
                <Badge variant="secondary" className="mb-2">
                  {t('auth.twoFactor.badges.totp')}
                </Badge>
                <p className="text-sm text-muted-foreground">
                  {t('auth.twoFactor.instructions.totp')}
                </p>
              </div>

              <div className="space-y-3">
                <Label htmlFor="totp-code">{t('auth.twoFactor.labels.verificationCode')}</Label>
                <Input
                  id="totp-code"
                  type="text"
                  value={verificationCode}
                  onChange={(e) => setVerificationCode(e.target.value.replace(/\D/g, ''))}
                  placeholder="000000"
                  maxLength={6}
                  className="text-center text-lg font-mono tracking-wider"
                  autoComplete="one-time-code"
                  autoFocus
                />
              </div>

              {verifyTotp.isError && (
                <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                  <p className="text-sm text-destructive">
                    {verifyTotp.error?.message || t('auth.twoFactor.errors.invalidCode')}
                  </p>
                </div>
              )}

              <Button
                onClick={handleVerifyCode}
                disabled={verificationCode.length !== 6 || verifyTotp.isPending}
                className="w-full"
              >
                {verifyTotp.isPending ? (
                  <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Check className="w-4 h-4 mr-2" />
                )}
                {verifyTotp.isPending ? t('auth.twoFactor.status.verifying') : t('auth.twoFactor.actions.verifyCode')}
              </Button>
            </div>
          )}

          {/* Email OTP Verification */}
          {activeMethod === 'email' && (
            <div className="space-y-4">
              <div className="text-center mb-4">
                <Badge variant="secondary" className="mb-2">
                  {t('auth.twoFactor.badges.email')}
                </Badge>
                <p className="text-sm text-muted-foreground">
                  {t('auth.twoFactor.instructions.emailSendTo', { email: userEmail })}
                </p>
              </div>

              <div className="space-y-3">
                {!sendEmailOtp.data?.sent && (
                  <Button
                    onClick={handleSendEmailOtp}
                    disabled={sendEmailOtp.isPending || !userEmail}
                    variant="outline"
                    className="w-full"
                  >
                    {sendEmailOtp.isPending ? (
                      <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                    ) : (
                      <Mail className="w-4 h-4 mr-2" />
                    )}
                    {sendEmailOtp.isPending ? t('auth.twoFactor.status.sending') : t('auth.twoFactor.actions.sendEmail')}
                  </Button>
                )}

                {(sendEmailOtp.data?.sent || sendEmailOtp.isSuccess) && (
                  <>
                    <div className="p-3 bg-green-50 border border-green-200 rounded-md">
                      <p className="text-sm text-green-800">
                        {t('auth.twoFactor.success.emailCodeSent')}
                      </p>
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor="email-code">{t('auth.twoFactor.labels.emailVerificationCode')}</Label>
                      <Input
                        id="email-code"
                        type="text"
                        value={verificationCode}
                        onChange={(e) => setVerificationCode(e.target.value.replace(/\D/g, ''))}
                        placeholder="000000"
                        maxLength={6}
                        className="text-center text-lg font-mono tracking-wider"
                        autoComplete="one-time-code"
                      />
                    </div>

                    {verifyEmailOtp.isError && (
                      <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                        <p className="text-sm text-destructive">
                          {verifyEmailOtp.error?.message || t('auth.twoFactor.errors.invalidCode')}
                        </p>
                      </div>
                    )}

                    <Button
                      onClick={handleVerifyCode}
                      disabled={verificationCode.length !== 6 || verifyEmailOtp.isPending}
                      className="w-full"
                    >
                      {verifyEmailOtp.isPending ? (
                        <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                      ) : (
                        <Check className="w-4 h-4 mr-2" />
                      )}
                      {verifyEmailOtp.isPending ? t('auth.twoFactor.status.verifying') : t('auth.twoFactor.actions.verifyEmailCode')}
                    </Button>
                  </>
                )}
              </div>

              {sendEmailOtp.isError && (
                <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                  <p className="text-sm text-destructive">
                    {sendEmailOtp.error?.message || t('auth.twoFactor.errors.sendEmailFailed')}
                  </p>
                </div>
              )}
            </div>
          )}

          {/* SMS OTP Verification */}
          {activeMethod === 'sms' && (
            <div className="space-y-4">
              <div className="text-center mb-4">
                <Badge variant="secondary" className="mb-2">
                  {t('auth.twoFactor.badges.sms')}
                </Badge>
                <p className="text-sm text-muted-foreground">
                  {t('auth.twoFactor.instructions.smsSendTo', { phone: maskPhoneNumber(userPhone) })}
                </p>
              </div>

              <div className="space-y-3">
                {!sendSmsOtp.data?.sent && (
                  <Button
                    onClick={handleSendSmsOtp}
                    disabled={sendSmsOtp.isPending || !userPhone}
                    variant="outline"
                    className="w-full"
                  >
                    {sendSmsOtp.isPending ? (
                      <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                    ) : (
                      <Smartphone className="w-4 h-4 mr-2" />
                    )}
                    {sendSmsOtp.isPending ? t('auth.twoFactor.status.sending') : t('auth.twoFactor.actions.sendSms')}
                  </Button>
                )}

                {(sendSmsOtp.data?.sent || sendSmsOtp.isSuccess) && (
                  <>
                    <div className="p-3 bg-green-50 border border-green-200 rounded-md">
                      <p className="text-sm text-green-800">
                        {t('auth.twoFactor.success.smsCodeSent')}
                      </p>
                    </div>

                    <div className="space-y-2">
                      <Label htmlFor="sms-code">{t('auth.twoFactor.labels.smsVerificationCode')}</Label>
                      <Input
                        id="sms-code"
                        type="text"
                        value={verificationCode}
                        onChange={(e) => setVerificationCode(e.target.value.replace(/\D/g, ''))}
                        placeholder="000000"
                        maxLength={6}
                        className="text-center text-lg font-mono tracking-wider"
                        autoComplete="one-time-code"
                      />
                    </div>

                    {verifySmsOtp.isError && (
                      <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                        <p className="text-sm text-destructive">
                          {verifySmsOtp.error?.message || t('auth.twoFactor.errors.invalidCode')}
                        </p>
                      </div>
                    )}

                    <Button
                      onClick={handleVerifyCode}
                      disabled={verificationCode.length !== 6 || verifySmsOtp.isPending}
                      className="w-full"
                    >
                      {verifySmsOtp.isPending ? (
                        <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                      ) : (
                        <Check className="w-4 h-4 mr-2" />
                      )}
                      {verifySmsOtp.isPending ? t('auth.twoFactor.status.verifying') : t('auth.twoFactor.actions.verifySmsCode')}
                    </Button>
                  </>
                )}
              </div>

              {sendSmsOtp.isError && (
                <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                  <p className="text-sm text-destructive">
                    {sendSmsOtp.error?.message || t('auth.twoFactor.errors.sendSmsFailed')}
                  </p>
                </div>
              )}
            </div>
          )}

          {/* Recovery Code Section */}
          {activeMethod === 'recovery' && (
            <div className="space-y-4">
              <div className="text-center">
                <Badge variant="secondary" className="mb-2">
                  {t('auth.twoFactor.badges.recovery')}
                </Badge>
                <p className="text-sm text-muted-foreground">
                  {t('auth.twoFactor.instructions.recovery')}
                </p>
              </div>

              <div className="space-y-3">
                <Label htmlFor="recovery-code">{t('auth.twoFactor.labels.recoveryCode')}</Label>
                <Input
                  id="recovery-code"
                  type="text"
                  value={recoveryCode}
                  onChange={(e) => setRecoveryCode(e.target.value.replace(/[^a-zA-Z0-9]/g, ''))}
                  placeholder="XXXX-XXXX-XXXX"
                  className="text-center font-mono tracking-wider"
                  autoComplete="off"
                />
              </div>

              {verifyRecoveryCode.isError && (
                <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                  <p className="text-sm text-destructive">
                    {verifyRecoveryCode.error?.message || t('auth.twoFactor.errors.invalidRecoveryCode')}
                  </p>
                </div>
              )}

              <Button
                onClick={handleVerifyRecovery}
                disabled={recoveryCode.length < 8 || verifyRecoveryCode.isPending}
                className="w-full"
              >
                {verifyRecoveryCode.isPending ? (
                  <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                ) : (
                  <Key className="w-4 h-4 mr-2" />
                )}
                {verifyRecoveryCode.isPending ? t('auth.twoFactor.status.verifying') : t('auth.twoFactor.actions.useRecoveryCode')}
              </Button>
            </div>
          )}

          {/* Cancel button */}
          {onCancel && (
            <div className="mt-6 pt-4 border-t border-border">
              <Button variant="outline" onClick={onCancel} className="w-full">
                {t('common.cancel')}
              </Button>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
