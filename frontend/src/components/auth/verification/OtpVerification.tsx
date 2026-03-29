'use client';

import React, { useState, useEffect } from 'react';
import { 
  Shield, 
  Mail, 
  Smartphone, 
  RefreshCw, 
  CheckCircle, 
  AlertTriangle,
  Clock
} from 'lucide-react';
import { Button } from '../ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { Badge } from '../ui/badge';
import { 
  useSendEmailOtp, 
  useVerifyEmailOtp, 
  useSendSmsOtp, 
  useVerifySmsOtp 
} from '../lib/api/auth-provider-hooks';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface OtpVerificationProps {
  /**
   * Additional CSS classes
   */
  className?: string;
  
  /**
   * Verification method (email or sms)
   */
  method: 'email' | 'sms';
  
  /**
   * Email address for email OTP (required if method is email)
   */
  email?: string;
  
  /**
   * Phone number for SMS OTP (required if method is sms)
   */
  phoneNumber?: string;
  
  /**
   * Callback fired when verification is successful
   */
  onVerificationSuccess?: (data: unknown) => void;
  
  /**
   * Callback fired when verification fails
   */
  onVerificationError?: (error: string) => void;
  
  /**
   * Auto-send OTP on component mount
   */
  autoSend?: boolean;
}

/**
 * OtpVerification Component
 * 
 * General-purpose OTP verification with:
 * - ✅ ORPC integration with Better Auth backend
 * - ✅ Email and SMS OTP support
 * - ✅ Auto-resend functionality with countdown
 * - ✅ Real-time validation and error handling
 * - ✅ Accessibility (WCAG 2.1 AA compliant)
 * - ✅ Design law compliance (clear feedback, error states)
 * - ✅ Norwegian phone number formatting
 * - ✅ Comprehensive status indicators
 * 
 * @example
 * ```tsx
 * <OtpVerification
 *   method="email"
 *   email="user@example.com"
 *   onVerificationSuccess={(data) => console.log('Verified:', data)}
 *   autoSend={true}
 * />
 * ```
 */
export function OtpVerification({
  className = '',
  method,
  email = '',
  phoneNumber = '',
  onVerificationSuccess,
  onVerificationError,
  autoSend = false,
}: OtpVerificationProps) {
  const { isNorwegian } = useLanguageSwitch();
  const [code, setCode] = useState('');
  const [isCodeSent, setIsCodeSent] = useState(false);
  const [countdown, setCountdown] = useState(0);
  const [step, setStep] = useState<'send' | 'verify'>('send');

  // Hooks for OTP operations
  const sendEmailOtp = useSendEmailOtp();
  const verifyEmailOtp = useVerifyEmailOtp();
  const sendSmsOtp = useSendSmsOtp();
  const verifySmsOtp = useVerifySmsOtp();

  // Auto-send OTP on mount if enabled
  useEffect(() => {
    if (autoSend && !isCodeSent) {
      handleSendCode();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoSend, isCodeSent]);

  // Countdown timer
  useEffect(() => {
    if (countdown > 0) {
      const timer = setTimeout(() => setCountdown(countdown - 1), 1000);
      return () => clearTimeout(timer);
    }
  }, [countdown]);

  const handleSendCode = async () => {
    try {
      if (method === 'email') {
        await sendEmailOtp.mutateAsync({ email });
      } else {
        await sendSmsOtp.mutateAsync({ phoneNumber });
      }
      
      setIsCodeSent(true);
      setStep('verify');
      setCountdown(60); // 60 second cooldown
      console.log(`${method.toUpperCase()} OTP sent successfully`);
    } catch (error: unknown) {
      const errorMessage = (error as Error)?.message || `Failed to send ${method.toUpperCase()} OTP`;
      onVerificationError?.(errorMessage);
      console.error('Failed to send OTP:', error);
    }
  };

  const handleVerifyCode = async () => {
    if (!code.trim()) return;

    try {
      let result;
      if (method === 'email') {
        result = await verifyEmailOtp.mutateAsync({ email, code: code.trim() });
      } else {
        result = await verifySmsOtp.mutateAsync({ phoneNumber, code: code.trim() });
      }
      
      onVerificationSuccess?.(result);
      console.log(`${method.toUpperCase()} OTP verified successfully`);
    } catch (error: unknown) {
      const errorMessage = (error as Error)?.message || `Invalid ${method.toUpperCase()} verification code`;
      onVerificationError?.(errorMessage);
      console.error('Failed to verify OTP:', error);
    }
  };

  const handleResendCode = async () => {
    if (countdown > 0) return;
    
    setCode('');
    await handleSendCode();
  };

  const formatPhoneNumber = (phone: string) => {
    // Norwegian phone number formatting
    if (phone.startsWith('+47')) {
      const number = phone.slice(3);
      if (number.length === 8) {
        return `+47 ${number.slice(0, 3)} ${number.slice(3, 5)} ${number.slice(5)}`;
      }
    }
    return phone;
  };

  const getMethodIcon = () => {
    switch (method) {
      case 'email': return <Mail className="w-5 h-5" />;
      case 'sms': return <Smartphone className="w-5 h-5" />;
      default: return <Shield className="w-5 h-5" />;
    }
  };

  const getMethodName = () => {
    switch (method) {
      case 'email': return isNorwegian ? 'E-post' : 'Email';
      case 'sms': return 'SMS';
      default: return 'OTP';
    }
  };

  const getDestination = () => {
    if (method === 'email') {
      return email;
    } else {
      return formatPhoneNumber(phoneNumber);
    }
  };

  // Loading state
  if (sendEmailOtp.isPending || sendSmsOtp.isPending || verifyEmailOtp.isPending || verifySmsOtp.isPending) {
    return (
      <Card className={`w-full max-w-md mx-auto ${className}`}>
        <CardHeader className="text-center">
          <div className="mx-auto mb-4 w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
            <RefreshCw className="w-6 h-6 text-primary animate-spin" />
          </div>
          <CardTitle>
            {sendEmailOtp.isPending || sendSmsOtp.isPending 
              ? (isNorwegian ? 'Sender kode' : 'Sending Code')
              : (isNorwegian ? 'Verifiserer kode' : 'Verifying Code')
            }
          </CardTitle>
          <CardDescription>
            {isNorwegian ? 'Vennligst vent mens vi' : 'Please wait while we'} {sendEmailOtp.isPending || sendSmsOtp.isPending 
              ? (isNorwegian ? 'sender' : 'send') 
              : (isNorwegian ? 'verifiserer' : 'verify')
            } {isNorwegian ? 'din' : 'your'} {getMethodName()}{isNorwegian ? '-kode...' : ' code...'}
          </CardDescription>
        </CardHeader>
      </Card>
    );
  }

  return (
    <div className={`space-y-6 ${className}`}>
      {/* Header */}
      <Card>
        <CardHeader className="text-center">
          <div className="mx-auto mb-4 w-12 h-12 rounded-full bg-primary/10 flex items-center justify-center">
            {getMethodIcon()}
          </div>
          <CardTitle className="flex items-center justify-center gap-2">
            {getMethodName()}{isNorwegian ? '-verifisering' : ' Verification'}
            {isCodeSent && (
              <Badge variant="outline" className="bg-green-100 text-green-800">
                <CheckCircle className="w-3 h-3 mr-1" />
                {isNorwegian ? 'Kode sendt' : 'Code sent'}
              </Badge>
            )}
          </CardTitle>
          <CardDescription>
            {step === 'send' 
              ? (isNorwegian 
                  ? `Vi sender en verifiseringskode til din ${method === 'email' ? 'e-postadresse' : 'telefonnummer'}` 
                  : `We'll send a verification code to your ${method === 'email' ? 'email address' : 'phone number'}`
                )
              : (isNorwegian 
                  ? `Skriv inn verifiseringskoden sendt til ${getDestination()}`
                  : `Enter the verification code sent to ${getDestination()}`
                )
            }
          </CardDescription>
        </CardHeader>
      </Card>

      {/* Step 1: Send Code */}
      {step === 'send' && (
        <Card>
          <CardHeader>
            <CardTitle className="text-lg">{isNorwegian ? 'Send verifiseringskode' : 'Send Verification Code'}</CardTitle>
            <CardDescription>
              {isNorwegian ? 'Bekreft din' : 'Confirm your'} {method === 'email' ? (isNorwegian ? 'e-postadresse' : 'email address') : (isNorwegian ? 'telefonnummer' : 'phone number')} {isNorwegian ? 'for å motta en verifiseringskode' : 'to receive a verification code'}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="p-4 bg-muted/50 border rounded-lg">
              <div className="flex items-center gap-3">
                {getMethodIcon()}
                <div>
                  <p className="font-medium">{getDestination()}</p>
                  <p className="text-sm text-muted-foreground">
                    {method === 'email' 
                      ? (isNorwegian ? 'Vi sender en 6-sifret kode til denne e-postadressen' : 'We\'ll send a 6-digit code to this email address')
                      : (isNorwegian ? 'Vi sender en 6-sifret kode via SMS' : 'We\'ll send a 6-digit code via SMS')
                    }
                  </p>
                </div>
              </div>
            </div>

            <Button onClick={handleSendCode} className="w-full">
              <Shield className="w-4 h-4 mr-2" />
              {isNorwegian ? 'Send verifiseringskode' : 'Send verification code'}
            </Button>

            {(sendEmailOtp.isError || sendSmsOtp.isError) && (
              <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                <div className="flex items-start gap-3">
                  <AlertTriangle className="w-4 h-4 text-destructive mt-0.5" />
                  <div className="text-sm">
                    <p className="font-medium text-destructive">{isNorwegian ? 'Kunne ikke sende kode' : 'Failed to send code'}</p>
                    <p className="text-destructive mt-1">
                      {sendEmailOtp.error?.message || sendSmsOtp.error?.message || (isNorwegian ? 'Vennligst prøv igjen.' : 'Please try again.')}
                    </p>
                  </div>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* Step 2: Verify Code */}
      {step === 'verify' && (
        <Card>
          <CardHeader>
            <CardTitle className="text-lg">{isNorwegian ? 'Skriv inn verifiseringskode' : 'Enter Verification Code'}</CardTitle>
            <CardDescription>
              {isNorwegian ? 'Sjekk' : 'Check your'} {method === 'email' ? (isNorwegian ? 'e-postinnboksen din' : 'email inbox') : (isNorwegian ? 'tekstmeldingene dine' : 'text messages')} {isNorwegian ? 'for den 6-sifrede koden' : 'for the 6-digit code'}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="code">{isNorwegian ? 'Verifiseringskode' : 'Verification Code'}</Label>
              <Input
                id="code"
                type="text"
                placeholder="000000"
                value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
                className="text-center text-lg font-mono tracking-widest"
                maxLength={6}
                autoComplete="one-time-code"
                autoFocus
              />
              <p className="text-xs text-muted-foreground">
                {isNorwegian ? 'Skriv inn den 6-sifrede koden sendt til' : 'Enter the 6-digit code sent to'} {getDestination()}
              </p>
            </div>

            <Button 
              onClick={handleVerifyCode} 
              disabled={code.length !== 6}
              className="w-full"
            >
              <CheckCircle className="w-4 h-4 mr-2" />
              {isNorwegian ? 'Verifiser kode' : 'Verify code'}
            </Button>

            {/* Resend Code */}
            <div className="text-center">
              <div className="text-sm text-muted-foreground mb-2">
                {isNorwegian ? 'Fikk du ikke koden?' : 'Didn\'t receive the code?'}
              </div>
              {countdown > 0 ? (
                <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
                  <Clock className="w-4 h-4" />
                  <span>{isNorwegian ? 'Send på nytt tilgjengelig om' : 'Resend available in'} {countdown}s</span>
                </div>
              ) : (
                <Button 
                  onClick={handleResendCode}
                  variant="outline"
                  size="sm"
                >
                  <RefreshCw className="w-4 h-4 mr-2" />
                  {isNorwegian ? 'Send kode på nytt' : 'Resend code'}
                </Button>
              )}
            </div>

            {(verifyEmailOtp.isError || verifySmsOtp.isError) && (
              <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                <div className="flex items-start gap-3">
                  <AlertTriangle className="w-4 h-4 text-destructive mt-0.5" />
                  <div className="text-sm">
                    <p className="font-medium text-destructive">{isNorwegian ? 'Verifisering mislyktes' : 'Verification failed'}</p>
                    <p className="text-destructive mt-1">
                      {verifyEmailOtp.error?.message || verifySmsOtp.error?.message || (isNorwegian ? 'Ugyldig verifiseringskode. Vennligst prøv igjen.' : 'Invalid verification code. Please try again.')}
                    </p>
                  </div>
                </div>
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
            {isNorwegian ? 'Sikkerhetstips' : 'Security Tips'}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="space-y-3 text-sm">
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{isNorwegian ? 'Verifiseringskoder utløper etter noen minutter av sikkerhetshensyn.' : 'Verification codes expire after a few minutes for security.'}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{isNorwegian ? 'Del aldri dine verifiseringskoder med noen.' : 'Never share your verification codes with anyone.'}</p>
            </div>
            <div className="flex items-start gap-3">
              <div className="w-2 h-2 rounded-full bg-primary mt-2 flex-shrink-0" />
              <p>{isNorwegian ? 'Hvis du ikke ba om denne koden, sikre kontoen din umiddelbart.' : 'If you didn\'t request this code, secure your account immediately.'}</p>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
