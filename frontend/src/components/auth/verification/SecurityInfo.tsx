import React from 'react';
import { CheckCircle2 } from 'lucide-react';
import { useTranslation } from '../lib/i18n/hooks';

interface SecurityInfoProps {
  className?: string;
}

export function SecurityInfo({ className = '' }: SecurityInfoProps) {
  const { t } = useTranslation();
  return (
    <div className={className}>
      <h2 className="text-lg font-bold text-foreground mb-3">
    {t('auth.security.title')}
      </h2>
      <div className="space-y-3">
        <div className="flex items-start gap-2">
          <CheckCircle2 className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
          <div>
            <h3 className="font-medium text-foreground text-sm">
      {t('auth.security.mfa.title')}
            </h3>
            <p className="text-xs text-muted-foreground">
      {t('auth.security.mfa.description')}
            </p>
          </div>
        </div>
        
        <div className="flex items-start gap-2">
          <CheckCircle2 className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
          <div>
            <h3 className="font-medium text-foreground text-sm">
      {t('auth.security.passwordless.title')}
            </h3>
            <p className="text-xs text-muted-foreground">
      {t('auth.security.passwordless.description')}
            </p>
          </div>
        </div>
        
        <div className="flex items-start gap-2">
          <CheckCircle2 className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
          <div>
            <h3 className="font-medium text-foreground text-sm">
      {t('auth.security.sso.title')}
            </h3>
            <p className="text-xs text-muted-foreground">
      {t('auth.security.sso.description')}
            </p>
          </div>
        </div>
        
        <div className="flex items-start gap-2">
          <CheckCircle2 className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
          <div>
            <h3 className="font-medium text-foreground text-sm">
      {t('auth.security.gdpr.title')}
            </h3>
            <p className="text-xs text-muted-foreground">
      {t('auth.security.gdpr.description')}
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}