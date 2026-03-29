import React from 'react';
import { useLanguageSwitch } from '../lib/i18n/hooks';

interface SecurityStatsProps {
  className?: string;
  companyName?: string;
}

export function SecurityStats({ className = '', companyName = 'ID-Knuten' }: SecurityStatsProps) {
  const { isNorwegian } = useLanguageSwitch();
  return (
    <div className={className}>
      <h3 className="text-sm font-semibold text-foreground mb-3">
        {isNorwegian ? 'Sikkerhetsstatistikk' : 'Security Metrics'}
      </h3>
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-background/50 backdrop-blur-sm rounded-lg p-3 border border-border/50">
          <div className="text-lg font-bold text-foreground">99.9%</div>
          <div className="text-xs text-muted-foreground">
            {isNorwegian ? 'Oppetid' : 'Uptime'}
          </div>
        </div>
        <div className="bg-background/50 backdrop-blur-sm rounded-lg p-3 border border-border/50">
          <div className="text-lg font-bold text-foreground">256-bit</div>
          <div className="text-xs text-muted-foreground">
            {isNorwegian ? 'Kryptering' : 'Encryption'}
          </div>
        </div>
        <div className="bg-background/50 backdrop-blur-sm rounded-lg p-3 border border-border/50">
          <div className="text-lg font-bold text-foreground">SOC 2</div>
          <div className="text-xs text-muted-foreground">
            {isNorwegian ? 'Sertifisert' : 'Certified'}
          </div>
        </div>
        <div className="bg-background/50 backdrop-blur-sm rounded-lg p-3 border border-border/50">
          <div className="text-lg font-bold text-foreground">24/7</div>
          <div className="text-xs text-muted-foreground">
            {isNorwegian ? 'Overvåking' : 'Monitoring'}
          </div>
        </div>
      </div>
      
      {/* Footer */}
      <div className="mt-4 text-center">
        <p className="text-xs text-muted-foreground">
          {isNorwegian 
            ? `Beskyttet av ${companyName} Sikkerhetsplattform`
            : `Protected by ${companyName} Security Platform`
          }
        </p>
      </div>
    </div>
  );
}