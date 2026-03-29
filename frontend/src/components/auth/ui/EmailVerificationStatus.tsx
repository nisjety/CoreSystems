'use client';

import { useState, useEffect } from 'react';
import { 
  CheckCircle, 
  XCircle, 
  Clock, 
  Mail, 
  AlertTriangle, 
  RefreshCw,
  Shield,
  ExternalLink
} from 'lucide-react';
import { useTranslationWithInterpolation } from '../lib/i18n/hooks';

type VerificationStatus = 'verified' | 'pending' | 'expired' | 'failed' | 'not-sent' | 'loading';

interface EmailVerificationStatusProps {
  email: string;
  status: VerificationStatus;
  onResend?: () => Promise<void>;
  onRefresh?: () => Promise<void>;
  className?: string;
  showResendButton?: boolean;
  showRefreshButton?: boolean;
  autoRefresh?: boolean;
  refreshInterval?: number;
}

export function EmailVerificationStatus({
  email,
  status,
  onResend,
  onRefresh,
  className = '',
  showResendButton = true,
  showRefreshButton = true,
  autoRefresh = false,
  refreshInterval = 5000,
}: EmailVerificationStatusProps) {
  // Use i18n hook for language detection
  const { t } = useTranslationWithInterpolation();
  // language prop retained for API compatibility; isNorwegian covers rendering

  const [isResending, setIsResending] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastResent, setLastResent] = useState<Date | null>(null);
  const [resendCooldown, setResendCooldown] = useState(0);

  // Auto-refresh functionality
  useEffect(() => {
    if (!autoRefresh || !onRefresh || status === 'verified') return;

    const interval = setInterval(async () => {
      if (!isRefreshing) {
        setIsRefreshing(true);
        try {
          await onRefresh();
        } catch (error) {
          console.error('Auto-refresh failed:', error);
        } finally {
          setIsRefreshing(false);
        }
      }
    }, refreshInterval);

    return () => clearInterval(interval);
  }, [autoRefresh, onRefresh, refreshInterval, status, isRefreshing]);

  // Resend cooldown timer
  useEffect(() => {
    if (resendCooldown > 0) {
      const timer = setTimeout(() => {
        setResendCooldown(prev => prev - 1);
      }, 1000);
      return () => clearTimeout(timer);
    }
  }, [resendCooldown]);

  const handleResend = async () => {
    if (!onResend || isResending || resendCooldown > 0) return;

    setIsResending(true);
    try {
      await onResend();
      setLastResent(new Date());
      setResendCooldown(60); // 60 second cooldown
    } catch (error) {
      console.error('Resend failed:', error);
    } finally {
      setIsResending(false);
    }
  };

  const handleRefresh = async () => {
    if (!onRefresh || isRefreshing) return;

    setIsRefreshing(true);
    try {
      await onRefresh();
    } catch (error) {
      console.error('Refresh failed:', error);
    } finally {
      setIsRefreshing(false);
    }
  };

  const getStatusConfig = () => {
    const mapKey = (s: VerificationStatus): string => {
      switch (s) {
        case 'not-sent': return 'notSent';
        default: return s;
      }
    };
    const baseKey = `auth.emailVerification.status.${mapKey(status)}`;
    const title = t(`${baseKey}.title`);
    const message = t(`${baseKey}.message`);
    switch (status) {
      case 'verified':
        return {
          icon: CheckCircle,
          iconColor: 'text-green-500',
          bgColor: 'bg-green-50',
          borderColor: 'border-green-200',
          title,
          message,
          actionable: false,
        };
      case 'pending':
        return {
          icon: Clock,
          iconColor: 'text-yellow-500',
          bgColor: 'bg-yellow-50',
          borderColor: 'border-yellow-200',
          title,
          message,
          actionable: true,
        };
      case 'expired':
        return {
          icon: XCircle,
          iconColor: 'text-red-500',
          bgColor: 'bg-red-50',
          borderColor: 'border-red-200',
          title,
          message,
          actionable: true,
        };
      case 'failed':
        return {
          icon: AlertTriangle,
          iconColor: 'text-red-500',
          bgColor: 'bg-red-50',
          borderColor: 'border-red-200',
          title,
          message,
          actionable: true,
        };
      case 'not-sent':
        return {
          icon: Mail,
          iconColor: 'text-gray-500',
          bgColor: 'bg-gray-50',
          borderColor: 'border-gray-200',
          title,
          message,
          actionable: true,
        };
      case 'loading':
        return {
          icon: RefreshCw,
          iconColor: 'text-blue-500',
          bgColor: 'bg-blue-50',
          borderColor: 'border-blue-200',
          title,
          message,
          actionable: false,
        };
      default:
        return {
          icon: AlertTriangle,
          iconColor: 'text-gray-500',
          bgColor: 'bg-gray-50',
          borderColor: 'border-gray-200',
          title,
          message,
          actionable: true,
        };
    }
  };

  const config = getStatusConfig();
  const Icon = config.icon;

  // Localized interface texts
  const it = {
    refreshStatus: t('auth.emailVerification.interface.refreshStatus'),
    email: t('auth.emailVerification.interface.emailLabel'),
    sending: t('auth.emailVerification.interface.sending'),
    resendIn: t('auth.emailVerification.interface.resendIn'),
    sendVerification: t('auth.emailVerification.interface.sendVerification'),
    resend: t('auth.emailVerification.interface.resend'),
    lastSent: t('auth.emailVerification.interface.lastSent'),
    emailNotFound: t('auth.emailVerification.interface.emailNotFound'),
    checkSpam: t('auth.emailVerification.interface.checkSpam'),
    checkCorrect: t('auth.emailVerification.interface.checkCorrect'),
    waitDelivery: t('auth.emailVerification.interface.waitDelivery'),
  };

  return (
    <div className={`p-4 rounded-lg border ${config.bgColor} ${config.borderColor} ${className}`}>
      <div className="flex items-start gap-3">
        <div className={`mt-0.5 ${config.iconColor}`}>
          <Icon className={`w-5 h-5 ${status === 'loading' || isRefreshing ? 'animate-spin' : ''}`} />
        </div>
        
        <div className="flex-1 min-w-0">
          <div className="flex items-center justify-between mb-1">
            <h3 className="font-medium text-foreground">{config.title}</h3>
            {showRefreshButton && onRefresh && config.actionable && (
              <button
                onClick={handleRefresh}
                disabled={isRefreshing}
                className="text-muted-foreground hover:text-foreground p-1 focus:outline-none focus:ring-2 focus:ring-ring rounded transition-colors"
                aria-label={it.refreshStatus}
              >
                <RefreshCw className={`w-4 h-4 ${isRefreshing ? 'animate-spin' : ''}`} />
              </button>
            )}
          </div>
          
          <p className="text-sm text-muted-foreground mb-2">{config.message}</p>
          
          <div className="text-xs text-muted-foreground mb-3">
            <strong>{it.email}</strong> {email}
          </div>

          {config.actionable && showResendButton && onResend && (
            <div className="flex items-center gap-3">
              <button
                onClick={handleResend}
                disabled={isResending || resendCooldown > 0}
                className="px-3 py-1.5 text-sm bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
              >
                {isResending ? (
                  <>
                    <RefreshCw className="w-4 h-4 mr-2 animate-spin" />
                    {it.sending}
                  </>
                ) : resendCooldown > 0 ? (
                  `${it.resendIn} ${resendCooldown}s`
                ) : (
                  <>
                    <Mail className="w-4 h-4 mr-2" />
                    {status === 'not-sent' ? it.sendVerification : it.resend}
                  </>
                )}
              </button>
              
              {lastResent && (
                <span className="text-xs text-muted-foreground">
                  {it.lastSent}{' '}
                  {lastResent.toLocaleTimeString(Intl.DateTimeFormat().resolvedOptions().locale, {
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit'
                  })}
                </span>
              )}
            </div>
          )}

          {status === 'pending' && (
            <div className="mt-3 p-2 bg-blue-50 border border-blue-200 rounded text-xs">
              <div className="flex items-start gap-2">
                <Shield className="w-3 h-3 text-blue-500 mt-0.5 flex-shrink-0" />
                <div className="text-blue-700">
                  <p className="font-medium mb-1">{it.emailNotFound}</p>
                  <ul className="space-y-1 text-blue-600">
                    <li>{it.checkSpam}</li>
                    <li>• {'Verify that'} {email} {it.checkCorrect}</li>
                    <li>{it.waitDelivery}</li>
                  </ul>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// Compact status indicator
interface EmailStatusBadgeProps {
  status: VerificationStatus;
  className?: string;
}

export function EmailStatusBadge({ 
  status, 
  className = '',
}: EmailStatusBadgeProps) {
  // Use i18n hook for language detection
  const { t } = useTranslationWithInterpolation();
  // no need for currentLanguage after i18n refactor

  const getStatusDisplay = () => {
    const mapKey = (s: VerificationStatus): string => {
      switch (s) {
        case 'not-sent': return 'notSent';
        default: return s;
      }
    };
    const statusText = t(`auth.emailVerification.badges.${mapKey(status)}`);
    switch (status) {
      case 'verified':
        return {
          text: statusText,
          className: 'bg-green-100 text-green-800 border-green-200',
          icon: CheckCircle,
        };
      case 'pending':
        return {
          text: statusText,
          className: 'bg-yellow-100 text-yellow-800 border-yellow-200',
          icon: Clock,
        };
      case 'expired':
      case 'failed':
        return {
          text: statusText,
          className: 'bg-red-100 text-red-800 border-red-200',
          icon: XCircle,
        };
      case 'not-sent':
        return {
          text: statusText,
          className: 'bg-gray-100 text-gray-800 border-gray-200',
          icon: Mail,
        };
      case 'loading':
        return {
          text: statusText,
          className: 'bg-blue-100 text-blue-800 border-blue-200',
          icon: RefreshCw,
        };
      default:
        return {
          text: statusText,
          className: 'bg-gray-100 text-gray-800 border-gray-200',
          icon: AlertTriangle,
        };
    }
  };

  const { text, className: statusClassName, icon: Icon } = getStatusDisplay();

  return (
    <span className={`inline-flex items-center gap-1 px-2 py-1 text-xs font-medium border rounded ${statusClassName} ${className}`}>
      <Icon className={`w-3 h-3 ${status === 'loading' ? 'animate-spin' : ''}`} />
      {text}
    </span>
  );
}

// Email verification help component
interface EmailVerificationHelpProps {
  className?: string;
}

export function EmailVerificationHelp({ 
  className = '',
}: EmailVerificationHelpProps) {
  // Use i18n hook for language detection
  const { t } = useTranslationWithInterpolation();
  // no need for currentLanguage after i18n refactor

  const reasons = [
    t('auth.emailVerification.help.reason1'),
    t('auth.emailVerification.help.reason2'),
    t('auth.emailVerification.help.reason3'),
    t('auth.emailVerification.help.reason4')
  ];

  return (
    <div className={`p-4 bg-blue-50 border border-blue-200 rounded-lg ${className}`}>
      <div className="flex items-start gap-3">
        <Shield className="w-5 h-5 text-blue-600 mt-0.5 flex-shrink-0" />
        <div>
          <h3 className="font-medium text-blue-900 mb-2">{t('auth.emailVerification.help.title')}</h3>
          <ul className="text-sm text-blue-800 space-y-1">
            {reasons.map((reason, index) => (
              <li key={index}>• {reason}</li>
            ))}
          </ul>
          <div className="mt-3 pt-3 border-t border-blue-300">
            <p className="text-xs text-blue-700">
              {t('auth.emailVerification.help.needHelp')}{' '}
              <a 
                href={t('auth.emailVerification.help.supportUrl') || '/support/email-verification'} 
                className="font-medium hover:underline"
                target="_blank"
                rel="noopener noreferrer"
              >
                {t('auth.emailVerification.help.supportGuide')}
                <ExternalLink className="w-3 h-3 inline ml-1" />
              </a>
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}